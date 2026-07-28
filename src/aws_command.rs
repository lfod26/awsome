use std::process::{Command, Output};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;

/// Error from an `aws` CLI invocation that ran but exited unsuccessfully.
///
/// `needs_login` distinguishes the "the profile just needs an (SSO) login"
/// case from any other failure, so callers (e.g. the login check) can check
/// it instead of scraping stderr strings themselves.
#[derive(Debug)]
pub struct AwsCliError {
    pub command: String,
    pub stderr: String,
    /// Whether the CLI reported that the profile's credentials are missing
    /// or expired - i.e. an SSO login is needed.
    pub needs_login: bool,
}

impl AwsCliError {
    /// Classifies a failed command's `stderr`, setting `needs_login` if it
    /// matches a known "credentials missing/expired" message.
    ///
    /// The markers are the (lowercased) verbatim messages botocore emits for
    /// these conditions - `NoCredentialsError`, `SSOTokenLoadError`,
    /// `TokenRetrievalError` ("Token has expired and refresh failed"),
    /// `UnauthorizedSSOTokenError`, and the STS `ExpiredToken` server error.
    /// See botocore's exception definitions:
    /// <https://github.com/boto/botocore/blob/develop/botocore/exceptions.py>
    fn classify(command: String, stderr: String) -> Self {
        const MARKERS: [&str; 5] = [
            "sso session associated with this profile has expired",
            "error loading sso token",
            "token has expired",
            "the security token included in the request is expired",
            "unable to locate credentials",
        ];

        let lowercased = stderr.to_ascii_lowercase();
        let needs_login = MARKERS.iter().any(|marker| lowercased.contains(marker));

        Self {
            command,
            stderr,
            needs_login,
        }
    }

    /// Returns `true` if `err` is an [`AwsCliError`] reporting that the
    /// profile's credentials are missing/expired (i.e. needs an SSO login).
    /// Any other error - including non-`AwsCliError` failures, e.g. the
    /// `aws` executable not being found - returns `false`.
    pub fn needs_login(err: &anyhow::Error) -> bool {
        err.downcast_ref::<Self>().is_some_and(|e| e.needs_login)
    }
}

impl std::fmt::Display for AwsCliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            command, stderr, ..
        } = self;

        if self.needs_login {
            write!(
                f,
                "`{command}` failed (credentials missing or expired - an SSO login is needed): {stderr}"
            )
        } else {
            write!(f, "`{command}` failed: {stderr}")
        }
    }
}

impl std::error::Error for AwsCliError {}

// AWS CLI docs:
// https://docs.aws.amazon.com/cli/latest/reference/ec2/describe-instances.html
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeInstancesOutput {
    pub reservations: Vec<Reservation>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Reservation {
    pub instances: Vec<InstanceJson>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct InstanceJson {
    pub instance_id: String,
    pub state: InstanceState,
    pub tags: Option<Vec<Tag>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct InstanceState {
    pub name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Tag {
    pub key: String,
    pub value: String,
}

// AWS CLI docs:
// https://docs.aws.amazon.com/cli/latest/reference/ssm/send-command.html
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SendCommandOutput {
    pub command: SendCommandCommand,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SendCommandCommand {
    pub command_id: String,
}

// AWS CLI docs:
// https://docs.aws.amazon.com/cli/latest/reference/ssm/get-command-invocation.html
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CommandInvocation {
    pub status: String,
    pub standard_output_content: Option<String>,
    pub standard_error_content: Option<String>,
}

pub struct AwsCommand {
    cmd: Command,
}

impl AwsCommand {
    fn new() -> Self {
        Self {
            cmd: Command::new("aws"),
        }
    }

    fn add_profile(mut self, profile: &str) -> Self {
        self.cmd.arg("--profile").arg(profile);
        self
    }

    fn arg(mut self, arg: &str) -> Self {
        self.cmd.arg(arg);
        self
    }

    /// Renders the command roughly as it'd be typed on a shell, for use
    /// in error messages.
    fn command_line(&self) -> String {
        let program = self.cmd.get_program().to_string_lossy();
        let args = self
            .cmd
            .get_args()
            .map(|a| a.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        format!("{program} {args}")
    }

    /// Runs the underlying command, failing if the `aws` executable can't be
    /// found/launched at all (a plain context error), or - if it runs but
    /// exits unsuccessfully - with an [`AwsCliError`] describing the failure
    /// (including its stderr) that callers can match on.
    fn run(&mut self) -> Result<Output> {
        let output = self
            .cmd
            .output()
            .with_context(|| format!("failed to run `{}`", self.command_line()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(AwsCliError::classify(self.command_line(), stderr).into());
        }

        Ok(output)
    }

    fn output_text(mut self) -> Result<String> {
        let output = self.run()?;
        String::from_utf8(output.stdout)
            .with_context(|| format!("`{}` returned non-UTF-8 output", self.command_line()))
    }

    /// Runs the command with inherited stdio, streaming its output straight
    /// to the terminal and capturing nothing. Used for interactive commands
    /// like `aws sso login`, which print a verification URL/code and wait for
    /// the user to authenticate in a browser - the user must see that output
    /// live, so it can't be captured and printed only after the process ends.
    fn status(mut self) -> Result<()> {
        let status = self
            .cmd
            .status()
            .with_context(|| format!("failed to run `{}`", self.command_line()))?;

        if !status.success() {
            bail!("`{}` failed", self.command_line());
        }

        Ok(())
    }

    fn output_json<T: DeserializeOwned>(mut self) -> Result<T> {
        self.cmd.arg("--output").arg("json");
        let output = self.run()?;
        serde_json::from_slice(&output.stdout)
            .with_context(|| format!("failed to parse JSON output of `{}`", self.command_line()))
    }

    pub fn describe_instances(
        profile: &str,
        with_id: Option<&str>,
    ) -> Result<DescribeInstancesOutput> {
        let mut cmd = Self::new().arg("ec2").arg("describe-instances");

        if let Some(id) = with_id {
            cmd = cmd.arg("--instance-ids").arg(id);
        }

        cmd.add_profile(profile).output_json()
    }

    pub fn list_profiles() -> Result<String> {
        Self::new()
            .arg("configure")
            .arg("list-profiles")
            .output_text()
    }

    pub fn sso_login(profile: &str) -> Result<()> {
        Self::new()
            .arg("sso")
            .arg("login")
            .arg("--no-cli-pager")
            .add_profile(profile)
            .status()
    }

    /// Runs `sts get-caller-identity` for `profile`. Succeeds if the profile
    /// has valid credentials; otherwise fails - with
    /// [`AwsCliError::needs_login`] set if the CLI reports the credentials
    /// are missing or expired (see [`AwsCliError::classify`]).
    ///
    /// [`AwsCliError::needs_login`]: AwsCliError#structfield.needs_login
    pub fn get_caller_identity(profile: &str) -> Result<()> {
        Self::new()
            .arg("sts")
            .arg("get-caller-identity")
            .arg("--no-cli-pager")
            .add_profile(profile)
            .run()
            .map(|_| ())
    }

    pub fn start_instances(profile: &str, instance_id: &str) -> Result<String> {
        Self::new()
            .arg("ec2")
            .arg("start-instances")
            .arg("--instance-ids")
            .arg(instance_id)
            .arg("--no-cli-pager")
            .add_profile(profile)
            .output_text()
    }

    pub fn wait_instance_running(profile: &str, instance_id: &str) -> Result<String> {
        Self::new()
            .arg("ec2")
            .arg("wait")
            .arg("instance-running")
            .arg("--instance-ids")
            .arg(instance_id)
            .add_profile(profile)
            .output_text()
    }

    pub fn stop_instances(profile: &str, instance_id: &str) -> Result<String> {
        Self::new()
            .arg("ec2")
            .arg("stop-instances")
            .arg("--instance-ids")
            .arg(instance_id)
            .arg("--no-cli-pager")
            .add_profile(profile)
            .output_text()
    }

    pub fn wait_instance_stopped(profile: &str, instance_id: &str) -> Result<String> {
        Self::new()
            .arg("ec2")
            .arg("wait")
            .arg("instance-stopped")
            .arg("--instance-ids")
            .arg(instance_id)
            .add_profile(profile)
            .output_text()
    }

    pub fn send_shell_script_command(
        profile: &str,
        instance_id: &str,
        parameters: &str,
    ) -> Result<SendCommandOutput> {
        Self::new()
            .arg("ssm")
            .arg("send-command")
            .arg("--instance-ids")
            .arg(instance_id)
            .arg("--document-name")
            .arg("AWS-RunShellScript")
            .arg("--parameters")
            .arg(parameters)
            .add_profile(profile)
            .output_json()
    }

    pub fn wait_command_executed(
        profile: &str,
        command_id: &str,
        instance_id: &str,
    ) -> Result<String> {
        Self::new()
            .arg("ssm")
            .arg("wait")
            .arg("command-executed")
            .arg("--command-id")
            .arg(command_id)
            .arg("--instance-id")
            .arg(instance_id)
            .add_profile(profile)
            .output_text()
    }

    pub fn get_command_invocation(
        profile: &str,
        command_id: &str,
        instance_id: &str,
    ) -> Result<CommandInvocation> {
        Self::new()
            .arg("ssm")
            .arg("get-command-invocation")
            .arg("--command-id")
            .arg(command_id)
            .arg("--instance-id")
            .arg(instance_id)
            .add_profile(profile)
            .output_json()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(stderr: &str) -> AwsCliError {
        AwsCliError::classify(
            "aws sts get-caller-identity".to_string(),
            stderr.to_string(),
        )
    }

    #[test]
    fn classify_flags_each_credential_marker() {
        let markers = [
            "The SSO session associated with this profile has expired or is otherwise invalid.",
            "Error loading SSO Token: Token for does not exist",
            "Token has expired and refresh failed",
            "The security token included in the request is expired",
            "Unable to locate credentials. You can configure credentials by running \"aws configure\".",
        ];

        for marker in markers {
            assert!(
                err(marker).needs_login,
                "expected needs_login for stderr: {marker}"
            );
        }
    }

    #[test]
    fn classify_is_case_insensitive() {
        assert!(err("TOKEN HAS EXPIRED AND REFRESH FAILED").needs_login);
    }

    #[test]
    fn classify_ignores_unrelated_errors() {
        assert!(!err("An error occurred (InvalidInstanceID.NotFound)").needs_login);
    }

    #[test]
    fn classify_stores_command_and_stderr_verbatim() {
        let e = err("some failure");
        assert_eq!(e.command, "aws sts get-caller-identity");
        assert_eq!(e.stderr, "some failure");
    }

    #[test]
    fn needs_login_helper_matches_only_login_errors() {
        let login: anyhow::Error = err("Token has expired").into();
        assert!(AwsCliError::needs_login(&login));

        let other_cli: anyhow::Error = err("some unrelated failure").into();
        assert!(!AwsCliError::needs_login(&other_cli));

        let non_cli = anyhow::anyhow!("io error: file not found");
        assert!(!AwsCliError::needs_login(&non_cli));
    }

    #[test]
    fn display_distinguishes_login_from_plain_failure() {
        let login = err("Token has expired");
        let shown = login.to_string();
        assert!(shown.contains("an SSO login is needed"), "{shown}");
        assert!(shown.contains("Token has expired"), "{shown}");

        let plain = err("boom");
        let shown = plain.to_string();
        assert!(!shown.contains("SSO login"), "{shown}");
        assert!(shown.contains("boom"), "{shown}");
    }

    #[test]
    fn deserializes_describe_instances_output() {
        let json = r#"{
            "Reservations": [
                {
                    "Instances": [
                        {
                            "InstanceId": "i-aaa",
                            "State": { "Name": "running" },
                            "Tags": [
                                { "Key": "Name", "Value": "web-server" },
                                { "Key": "env", "Value": "dev" }
                            ]
                        }
                    ]
                },
                {
                    "Instances": [
                        {
                            "InstanceId": "i-bbb",
                            "State": { "Name": "stopped" }
                        }
                    ]
                }
            ]
        }"#;

        let out: DescribeInstancesOutput = serde_json::from_str(json).unwrap();
        assert_eq!(out.reservations.len(), 2);

        let first = &out.reservations[0].instances[0];
        assert_eq!(first.instance_id, "i-aaa");
        assert_eq!(first.state.name, "running");
        let tags = first.tags.as_ref().unwrap();
        assert_eq!(tags[0].key, "Name");
        assert_eq!(tags[0].value, "web-server");

        let second = &out.reservations[1].instances[0];
        assert_eq!(second.instance_id, "i-bbb");
        assert!(second.tags.is_none());
    }
}
