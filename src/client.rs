//! Thin wrapper around the `aws` CLI: a profile-scoped client for shelling
//! out to `aws` subcommands and parsing their JSON output.

use anyhow::{Context, Result, bail};
use console::style;

use super::{
    aws_command::{AwsCliError, AwsCommand, DescribeInstancesOutput},
    logger::{bold, dim_under},
    logger_info, logger_success, logger_warn,
    spinner::with_spinner,
};

pub struct InstanceEntry {
    pub instance_id: String,
    pub name: String,
    pub state: String,
}

impl std::fmt::Display for InstanceEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {}",
            dim_under(&self.instance_id),
            style(&self.name).italic()
        )
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
    let caller_identity = with_spinner("fetching caller identity", || {
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
        logger_info!("profile {} is not logged in", bold(profile));
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
    /// instance, erroring if the instance doesn't exist.
    fn state(&self) -> Result<String> {
        logger_info!("checking instance {} state", dim_under(&self.instance_id));

        // (should only return one instance)
        let instances = describe_instances(&self.profile, Some(&self.instance_id))
            .with_context(|| format!("failed to describe instance {}", self.instance_id))?;

        instances
            .into_iter()
            .next()
            .map(|e| e.state)
            .with_context(|| format!("instance {} was not found", self.instance_id))
    }

    /// Starts this instance and waits until it reaches the `running`
    /// state.
    pub fn start_and_wait(&self, print_if_running: bool) -> Result<()> {
        if self.state()? == "running" {
            if print_if_running {
                logger_warn!(
                    "instance {} is already running",
                    dim_under(&self.instance_id)
                );
            }

            return Ok(());
        }

        with_spinner(
            format!("starting instance {}", dim_under(&self.instance_id)),
            || AwsCommand::start_instances(&self.profile, &self.instance_id),
        )?;

        with_spinner(
            format!(
                "waiting for instance {} to start",
                dim_under(&self.instance_id)
            ),
            || AwsCommand::wait_instance_running(&self.profile, &self.instance_id),
        )?;

        logger_success!(
            "instance {} is now {}.",
            dim_under(&self.instance_id),
            style("running").green()
        );
        Ok(())
    }

    /// Stops this instance and waits until it reaches the `stopped`
    /// state.
    pub fn stop_and_wait(&self, print_if_stopped: bool) -> Result<()> {
        if self.state()? == "stopped" {
            if print_if_stopped {
                logger_warn!(
                    "instance {} is already stopped",
                    dim_under(&self.instance_id)
                );
            }

            return Ok(());
        }

        with_spinner(
            format!("stopping instance {}", dim_under(&self.instance_id)),
            || AwsCommand::stop_instances(&self.profile, &self.instance_id),
        )?;

        with_spinner(
            format!(
                "waiting for instance {} to stop",
                dim_under(&self.instance_id)
            ),
            || AwsCommand::wait_instance_stopped(&self.profile, &self.instance_id),
        )?;

        logger_success!(
            "instance {} is now {}.",
            dim_under(&self.instance_id),
            style("stopped").red()
        );
        Ok(())
    }

    fn run_ssm_shell_script(&self, script: &str) -> Result<()> {
        let cmd_output = with_spinner(
            format!(
                "sending shell command to instance {}",
                dim_under(&self.instance_id)
            ),
            || AwsCommand::send_shell_script_command(&self.profile, &self.instance_id, script),
        )?;

        let cmd_id = cmd_output.command.command_id;

        with_spinner(
            format!(
                "waiting for the SSM command on {} to finish",
                dim_under(&self.instance_id)
            ),
            || AwsCommand::wait_command_executed(&self.profile, &cmd_id, &self.instance_id),
        )?;

        let invocation = with_spinner("getting SSM shell command result", || {
            AwsCommand::get_command_invocation(&self.profile, &cmd_id, &self.instance_id)
        })?;

        let resolve = |str: Option<String>| str.filter(|v| !v.is_empty());

        if invocation.status != "Success" {
            let mut err_msg = "SSM shell command unsuccessful".to_string();

            if let Some(cmd_stdout) = resolve(invocation.standard_output_content) {
                err_msg.push_str(&format!("\nstdout: {cmd_stdout}"));
            }

            if let Some(cmd_stderr) = resolve(invocation.standard_error_content) {
                err_msg.push_str(&format!("\nstderr: {cmd_stderr}"));
            }

            bail!("{err_msg}");
        }

        if let Some(cmd_stdout) = resolve(invocation.standard_output_content) {
            logger_info!("SSM shell stdout:");
            println!("{cmd_stdout}");
        }

        if let Some(cmd_stderr) = resolve(invocation.standard_error_content) {
            logger_warn!("SSM shell stderr:");
            println!("{cmd_stderr}");
        }

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
        // TODO: let's also add this to a shell script file and import it at compile time
        let script = format!(
            "if show=$(shutdown --show 2>&1); then \
                echo 'Shutdown already scheduled, leaving it as-is:'; \
                echo \"$show\"; \
             else \
                shutdown -h +{minutes} 'Auto-shutdown scheduled by awsome' && \
                echo 'Scheduled shutdown at {target_time} (in {minutes} minute(s)).'; \
             fi"
        );

        self.run_ssm_shell_script(&script)?;

        Ok(())
    }

    /// Installs `public_key` into the instance's `ec2-user`
    /// `authorized_keys` over SSM Run Command. Starts the instance first if
    /// it isn't already running (without printing an "already running"
    /// notice), since SSM Run Command requires it. Requires the SSM Agent
    /// and an instance profile granting SSM permissions.
    pub fn push_public_key(&self, script: &str) -> Result<()> {
        self.start_and_wait(false)?;

        logger_info!(
            "installing the public key on {} via SSM",
            dim_under(&self.instance_id)
        );

        self.run_ssm_shell_script(script)?;

        logger_success!("installed public key");

        Ok(())
    }
}

/// Opens an SSH-over-SSM tunnel to `instance_id` for use as an SSH
/// `ProxyCommand`. This runs inside the SSH transport: stdin/stdout carry the
/// SSH byte stream, so it must not write to stdout and must not launch an
/// interactive SSO login (which would corrupt the stream and can't read
/// input). If credentials are missing/expired it fails fast with guidance on
/// stderr so `ssh` reports a clean error.
pub fn start_ssh_proxy(profile: &str, instance_id: &str, port: u16) -> Result<()> {
    if !is_logged_in(profile)? {
        bail!("profile {profile} is not logged in");
    }

    AwsCommand::start_ssh_session(profile, instance_id, port)
}

pub fn list_profiles() -> Result<Vec<String>> {
    let stdout = AwsCommand::list_profiles()?;

    let profiles = parse_profiles(&stdout);

    if profiles.is_empty() {
        bail!("no AWS CLI profiles found, make sure it's configured");
    }

    Ok(profiles)
}

/// Parses `aws configure list-profiles` stdout into a list of profile
/// names, trimming each line and dropping blank ones.
fn parse_profiles(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect()
}

/// Runs `describe-instances`, optionally scoped to a single instance
/// ID, mapping the result down to `InstanceEntry`.
pub fn describe_instances(profile: &str, with_id: Option<&str>) -> Result<Vec<InstanceEntry>> {
    let out = with_spinner("fetching instances", || {
        AwsCommand::describe_instances(profile, with_id).context("failed to describe instances")
    })?;

    Ok(map_instances(out))
}

/// Flattens `describe-instances` JSON output into `InstanceEntry` values,
/// pulling each instance's `Name` tag (falling back to a placeholder when
/// there isn't one).
fn map_instances(out: DescribeInstancesOutput) -> Vec<InstanceEntry> {
    out.reservations
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
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_profiles_trims_and_drops_blank_lines() {
        let stdout = "default\n  james-bond  \n\n\tother-profile\n   \n";
        assert_eq!(
            parse_profiles(stdout),
            vec!["default", "james-bond", "other-profile"]
        );
    }

    #[test]
    fn parse_profiles_empty_input_yields_empty() {
        assert!(parse_profiles("").is_empty());
        assert!(parse_profiles("   \n\t\n").is_empty());
    }

    #[test]
    fn map_instances_extracts_name_tag_and_flattens_reservations() {
        let json = r#"{
            "Reservations": [
                {
                    "Instances": [
                        {
                            "InstanceId": "i-withname",
                            "State": { "Name": "running" },
                            "Tags": [
                                { "Key": "env", "Value": "dev" },
                                { "Key": "Name", "Value": "web-server" }
                            ]
                        }
                    ]
                },
                {
                    "Instances": [
                        {
                            "InstanceId": "i-noname",
                            "State": { "Name": "stopped" }
                        }
                    ]
                }
            ]
        }"#;
        let out: DescribeInstancesOutput = serde_json::from_str(json).unwrap();

        let entries = map_instances(out);
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].instance_id, "i-withname");
        assert_eq!(entries[0].name, "web-server");
        assert_eq!(entries[0].state, "running");

        assert_eq!(entries[1].instance_id, "i-noname");
        assert_eq!(entries[1].name, "(no Name tag)");
        assert_eq!(entries[1].state, "stopped");
    }

    #[test]
    fn map_instances_empty_output_yields_empty() {
        let out: DescribeInstancesOutput =
            serde_json::from_str(r#"{ "Reservations": [] }"#).unwrap();
        assert!(map_instances(out).is_empty());
    }
}
