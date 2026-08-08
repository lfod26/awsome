use clap::{Parser, Subcommand};

/// Starts a configured EC2 instance (waiting for it to reach the
/// `running` state), reading profile/instance groups from a config file
/// next to the executable. `start`, `stop`, and `setup-ssh` act on whichever group is
/// currently selected (see `configure select`); the selection is sticky
/// and persisted, so no prompting happens outside of `configure`. Instance
/// commands fail if no group has been configured yet.
#[derive(Parser)]
#[command(
    name = "awsome",
    about = "Manage a configured EC2 instance via `aws` CLI subcommands",
    version
)]
pub struct Cli {
    /// Which operation to run. If omitted, defaults to `start`.
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Add, remove, or show configured profile/instance groups. Only
    /// configures — does not start or stop any instance.
    Configure {
        #[command(subcommand)]
        action: ConfigureAction,
    },

    /// Start a configured EC2 instance (waiting for it to reach the
    /// `running` state).
    Start {
        /// Schedule an OS-level shutdown inside the instance at the given
        /// local clock time (24-hour HH:MM, e.g. `18:30`; default `18:30` if
        /// no value is given). Rolls over to tomorrow if that time has
        /// already passed today. Delivered via SSM Run Command. Instead of
        /// stopping the instance from here, this tells the instance itself
        /// to shut down later — so it stops even if this tool isn't running
        /// to see it happen. Checks first whether a shutdown is already
        /// scheduled and leaves it alone if so.
        #[arg(
            long,
            value_name = "HH:MM",
            num_args = 0..=1,
            default_missing_value = "18:30"
        )]
        schedule_shutdown: Option<String>,
    },

    /// Stop a configured EC2 instance (waiting for it to reach the
    /// `stopped` state) instead of starting it.
    Stop,

    /// Set up SSH access to the currently selected instance: ensures a
    /// dedicated local key exists, pushes its public half to the
    /// instance's `ec2-user` (over SSM Run Command), and writes a
    /// managed `Host awsome` entry to your `~/.ssh/config` whose
    /// `ProxyCommand` tunnels SSH through SSM. After running this once,
    /// `ssh awsome` connects to whichever group is currently selected
    /// (see `configure select`) - switch targets without editing any SSH
    /// config.
    SetupSsh,

    /// The SSH `ProxyCommand` used by the generated `~/.ssh/config` entry.
    /// You normally don't run this by hand - `ssh awsome` invokes it for you.
    /// It opens an SSH-over-SSM tunnel (`aws ssm start-session` with the
    /// `AWS-StartSSHSession` document) to the currently selected instance and
    /// bridges it to SSH via stdio.
    SshProxy {
        /// TCP port SSH wants to reach on the instance (passed by `ssh` as
        /// `%p`; usually 22).
        port: u16,
    },
}

#[derive(Subcommand)]
pub enum ConfigureAction {
    /// Add a new profile/instance group. Runs interactively (prompting
    /// you to pick a profile and instance) by default; pass `--profile`
    /// and/or `--instance-id` to skip the corresponding prompt.
    Add {
        /// AWS CLI profile to use for this group. Must be given together
        /// with `--instance-id`, or omitted entirely to prompt for both
        /// interactively.
        #[arg(long, requires = "instance_id")]
        profile: Option<String>,

        /// EC2 instance ID to use for this group. Must be given together
        /// with `--profile`, or omitted entirely to prompt for both
        /// interactively.
        #[arg(long, value_name = "INSTANCE_ID", requires = "profile")]
        instance_id: Option<String>,
    },

    /// Remove a configured profile/instance group. Runs interactively
    /// (prompting you to pick which one) by default; pass `--index` to
    /// remove a specific one without prompting (as numbered by
    /// `configure show`).
    Remove {
        /// 1-based index of the group to remove, as shown by
        /// `configure show`. If omitted, prompts you to pick one
        /// interactively.
        #[arg(long)]
        index: Option<usize>,
    },

    /// Show the currently configured profile/instance groups.
    Show,

    /// Select which configuration group to use.
    Select {
        /// 1-based index of the group to select, as shown by
        /// `configure show`. If omitted, prompts you to pick one
        /// interactively.
        #[arg(long)]
        index: Option<usize>,
    },
}
