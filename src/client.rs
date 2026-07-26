//! Thin wrapper around the `aws` CLI: a profile-scoped client for shelling
//! out to `aws` subcommands and parsing their JSON output.

use anyhow::{Context, Result, bail};
use console::style;

use super::{
    aws_command::{AwsCliError, AwsCommand, CommandInvocation},
    spinner::with_spinner,
};

pub struct InstanceEntry {
    pub instance_id: String,
    pub name: String,
    pub state: String,
}

impl std::fmt::Display for InstanceEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}) {}", self.instance_id, self.name)
    }
}

/// Checks whether the profile currently has valid credentials.
///
/// Returns `Ok(false)` only when the AWS CLI reports that the profile's
/// credentials are missing or expired (i.e. an SSO login is needed). Any
/// other failure - such as the `aws` executable not being installed, or
/// an unexpected CLI error - is propagated instead of being silently
/// treated as "not logged in".
pub fn is_logged_in(profile: &str) -> Result<bool> {
    let caller_identity = with_spinner("Fetching caller identity...", || {
        AwsCommand::get_caller_identity(profile)
    });

    if let Err(err) = caller_identity {
        return if AwsCliError::needs_login(&err) {
            Ok(false)
        } else {
            Err(err)
        };
    }

    Ok(true)
}

/// Ensures the profile has valid credentials, running an SSO login if not.
pub fn ensure_logged_in(profile: &str) -> Result<()> {
    if !is_logged_in(profile)? {
        println!("ℹ️  Profile `{profile}` is not logged in. Starting AWS SSO login...");
        AwsCommand::sso_login(profile)?;
    }

    Ok(())
}

pub struct Ec2Instance {
    profile: String,
    instance_id: String,
}

impl Ec2Instance {
    /// Creates a profile-scoped handle for controlling a single EC2
    /// instance.
    pub fn new_logged_in(profile: &str, instance_id: &str) -> Result<Self> {
        let (profile, instance_id) = (profile.to_string(), instance_id.to_string());

        ensure_logged_in(&profile)?;

        Ok(Self {
            profile,
            instance_id,
        })
    }

    /// Fetches the current state (e.g. "running", "stopped") of this
    /// instance, or `None` if it doesn't exist.
    pub fn state(&self) -> Result<Option<String>> {
        let entries = describe_instances(&self.profile, Some(&self.instance_id))
            .with_context(|| format!("failed to describe instance {}", &self.instance_id))?;

        Ok(entries.into_iter().next().map(|e| e.state))
    }

    /// Starts this instance and waits until it reaches the `running`
    /// state.
    pub fn start_and_wait(&self) -> Result<()> {
        println!("Starting instance {}...", &self.instance_id);
        AwsCommand::start_instances(&self.profile, &self.instance_id)?;

        with_spinner(
            &format!("Waiting for instance {} to start...", &self.instance_id),
            || AwsCommand::wait_instance_running(&self.profile, &self.instance_id),
        )?;

        println!(
            "Instance {} is now {}.",
            &self.instance_id,
            style("running").green()
        );
        Ok(())
    }

    /// Stops this instance and waits until it reaches the `stopped`
    /// state.
    pub fn stop_and_wait(&self) -> Result<()> {
        println!("Stopping instance {}...", &self.instance_id);
        AwsCommand::stop_instances(&self.profile, &self.instance_id)?;

        with_spinner(
            &format!("Waiting for instance {} to stop...", &self.instance_id),
            || AwsCommand::wait_instance_stopped(&self.profile, &self.instance_id),
        )?;

        println!(
            "Instance {} is now {}.",
            &self.instance_id,
            style("stopped").red()
        );
        Ok(())
    }

    /// Schedules an OS-level shutdown inside this instance in `minutes`
    /// minutes from now (`target_time` is only used for the printed
    /// message), via SSM Run Command (`AWS-RunShellScript`). First checks
    /// (in the same remote script) whether a shutdown is already pending
    /// via `shutdown --show`'s exit code (0 = a shutdown is scheduled,
    /// 1 = none is), and leaves it alone if so instead of scheduling a
    /// second one.
    ///
    /// Requires the instance to have the SSM Agent running and an
    /// instance profile with SSM permissions - if not, the send-command
    /// call itself will fail with a clear error from the CLI.
    pub fn schedule_shutdown(&self, minutes: i64, target_time: &str) -> Result<()> {
        let script = format!(
            "if show=$(shutdown --show 2>&1); then \
                echo 'Shutdown already scheduled, leaving it as-is:'; \
                echo \"$show\"; \
             else \
                shutdown -h +{minutes} 'Auto-shutdown scheduled by awsome' && \
                echo 'Scheduled shutdown at {target_time} (in {minutes} minute(s)).'; \
             fi"
        );

        println!(
            "Sending SSM command to schedule shutdown on {}...",
            &self.instance_id
        );
        let params = format!("commands=[\"{script}\"]");
        let send_output =
            AwsCommand::send_shell_script_command(&self.profile, &self.instance_id, &params)?;
        let command_id = send_output.command.command_id;

        let wait_result = with_spinner(
            &format!(
                "Waiting for the SSM command on {} to finish...",
                &self.instance_id
            ),
            || AwsCommand::wait_command_executed(&self.profile, &command_id, &self.instance_id),
        );

        let invocation: CommandInvocation =
            AwsCommand::get_command_invocation(&self.profile, &command_id, &self.instance_id)?;

        if let Some(stdout) = invocation.standard_output_content.as_deref() {
            let stdout = stdout.trim();
            if !stdout.is_empty() {
                println!("{stdout}");
            }
        }

        if wait_result.is_err() {
            bail!(
                "SSM command to schedule shutdown on {} did not succeed \
                 (status: {}): {}",
                &self.instance_id,
                invocation.status,
                invocation
                    .standard_error_content
                    .as_deref()
                    .unwrap_or_default()
                    .trim()
            );
        }

        Ok(())
    }
}

pub fn list_profiles() -> Result<Vec<String>> {
    let stdout = AwsCommand::list_profiles()?;

    let profiles: Vec<String> = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect();

    if profiles.is_empty() {
        bail!(
            "No AWS CLI profiles found. Run `aws configure` (or `aws configure sso`) \
             to set one up first."
        );
    }

    Ok(profiles)
}

/// Runs `describe-instances`, optionally scoped to a single instance
/// ID, mapping the result down to `InstanceEntry`.
pub fn describe_instances(profile: &str, with_id: Option<&str>) -> Result<Vec<InstanceEntry>> {
    let out = with_spinner("Fetching instances...", || {
        AwsCommand::describe_instances(profile, with_id).context("failed to describe instances")
    })?;

    let entries = out
        .reservations
        .into_iter()
        .flat_map(|r| r.instances)
        .map(|i| {
            let name = i
                .tags
                .as_deref()
                .unwrap_or_default()
                .iter()
                .find(|t| t.key == "Name")
                .map(|t| t.value.clone())
                .unwrap_or_else(|| "(no Name tag)".to_string());

            InstanceEntry {
                instance_id: i.instance_id,
                name,
                state: i.state.name,
            }
        })
        .collect();

    Ok(entries)
}
