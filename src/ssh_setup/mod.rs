//! Local SSH-setup helpers for `awsome ssh`: managing a dedicated SSH key,
//! generating the remote script that trusts its public half, and writing a
//! managed `~/.ssh/config` entry whose `ProxyCommand` tunnels SSH through SSM.
//!
//! The connection target is intentionally *not* baked into the SSH config:
//! the generated `Host awsome` entry calls back into `awsome ssh-proxy`, which
//! opens a Session Manager tunnel to whichever profile/instance group is
//! currently selected. Switch targets with `configure select`, no SSH-config
//! edits needed.

mod config;
mod keys;
mod path_util;

use std::path::PathBuf;

use anyhow::Result;

use config::ensure_ssh_config;
use keys::{build_push_script, ensure_local_ssh_key_exists, read_public_ssh_key};
use path_util::home_dir;

/// Fixed SSH `Host` alias written to `~/.ssh/config`. Generic on purpose so a
/// single `ssh awsome` follows the currently selected group.
const HOST_ALIAS: &str = "awsome";

/// Remote Linux user the key is installed for and that SSH logs in as.
const REMOTE_USER: &str = "ec2-user";

/// Resolved on-disk locations of the dedicated key pair and the SSH config.
struct SshPaths {
    ssh_dir: PathBuf,
    private_key: PathBuf,
    /// This is for the SSH config IdentityFile.
    ssh_conf_relative_private_key: String,
    public_key: PathBuf,
    config: PathBuf,
}

impl SshPaths {
    fn from_home(home: PathBuf) -> Result<Self> {
        let ssh_dir = home.join(".ssh");
        let private_key = ssh_dir.join(HOST_ALIAS);
        let ssh_conf_relative_private_key = format!(
            "\"~/{}\"",
            private_key
                .strip_prefix(&home)?
                .to_string_lossy()
                // `ssh_config` only expands `~/` when followed by forward
                // slashes; on Windows `strip_prefix` yields `\`-separated
                // components, which OpenSSH's tilde expansion doesn't
                // recognize. Normalize so `IdentityFile` works everywhere.
                .replace('\\', "/")
                .replace('"', "\\\"")
        );

        Ok(Self {
            private_key,
            ssh_conf_relative_private_key,
            public_key: ssh_dir.join(format!("{HOST_ALIAS}.pub")),
            config: ssh_dir.join("config"),
            ssh_dir,
        })
    }
}

pub fn setup_ssh(script_callback: impl FnOnce(&str) -> Result<()>) -> Result<()> {
    let paths = SshPaths::from_home(home_dir()?)?;

    ensure_local_ssh_key_exists(&paths)?;

    let public_key = read_public_ssh_key(&paths)?;
    let script = build_push_script(&public_key);

    script_callback(&script)?;

    ensure_ssh_config(&paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_all_ssh_paths_from_the_user_home() {
        let paths = SshPaths::from_home(PathBuf::from("home")).unwrap();

        assert_eq!(paths.ssh_dir, PathBuf::from("home").join(".ssh"));
        assert_eq!(
            paths.private_key,
            PathBuf::from("home").join(".ssh").join("awsome")
        );
        assert_eq!(
            paths.public_key,
            PathBuf::from("home").join(".ssh").join("awsome.pub")
        );
        assert_eq!(
            paths.config,
            PathBuf::from("home").join(".ssh").join("config")
        );
        assert_eq!(paths.ssh_conf_relative_private_key, "\"~/.ssh/awsome\"");
    }
}
