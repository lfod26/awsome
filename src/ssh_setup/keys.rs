//! Managing the dedicated `ed25519` key location via `ssh-keygen` without
//! touching the user's other keys.
//! Building the remote shell script that installs the public key into
//! `ec2-user`'s `authorized_keys`, run via SSM Run Command.

use std::{fs, io, path::Path, process::Command};

use anyhow::{Context, Result, bail};
use console::style;

use super::{
    super::{logger_info, logger_success},
    HOST_ALIAS, REMOTE_USER, SshPaths,
    path_util::harden_dir_permissions,
};

/// The `install_key` shell function, embedded at compile time with CRLF
/// normalized to LF so a Windows checkout can't smuggle `\r` into the remote
/// POSIX shell. Kept in its own `.sh` file for readability and shellcheck.
const INSTALL_KEY_SH: &str = const_str::replace!(include_str!("install_key.sh"), "\r\n", "\n");

/// Builds the remote shell script (run by SSM Run Command *as root*) that
/// installs `public_key` into `ec2-user`'s `authorized_keys`, by sourcing the
/// embedded `install_key` function and invoking it with the user and key.
pub fn build_push_script(public_key: &str) -> String {
    format!(
        "set -eu\n\
         {function}\n\
         install_key {user} {key}",
        function = INSTALL_KEY_SH,
        user = REMOTE_USER,
        key = shell_single_quote(public_key),
    )
}

/// Wraps `s` in single quotes for safe embedding in a POSIX shell script,
/// escaping any embedded single quotes.
fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Prepares the dedicated `ed25519` key location without touching other keys.
pub fn ensure_local_ssh_key_exists(paths: &SshPaths) -> Result<()> {
    fs::create_dir(&paths.ssh_dir)
        .or_else(|err| {
            if matches!(err.kind(), io::ErrorKind::AlreadyExists) {
                Ok(())
            } else {
                Err(err)
            }
        })
        .with_context(|| format!("failed to create {}", paths.ssh_dir.display()))?;

    harden_dir_permissions(&paths.ssh_dir);

    let private_exists = paths
        .private_key
        .try_exists()
        .context("failed to determine whether private key exists")?;
    let public_exists = paths
        .public_key
        .try_exists()
        .context("failed to determine whether public key exists")?;

    if (private_exists && !public_exists) || (!private_exists && public_exists) {
        bail!(
            "ssh key corruption detected: \n\
            private key exists: {private_exists}\n\
            public key exists: {public_exists}\n\
            manual intervention is required"
        );
    }

    if private_exists && public_exists {
        logger_info!(
            "reusing existing SSH keys at\n{}\n{}",
            style(paths.private_key.display()).dim(),
            style(paths.public_key.display()).dim()
        );

        return Ok(());
    }

    generate_key(&paths.private_key)?;

    logger_success!(
        "generated a new SSH key at {}",
        style(paths.private_key.display()).dim()
    );

    Ok(())
}

/// Reads and trims the dedicated public key.
pub fn read_public_ssh_key(paths: &SshPaths) -> Result<String> {
    let contents = std::fs::read_to_string(&paths.public_key).with_context(|| {
        format!(
            "failed to read public key at {}",
            paths.public_key.display()
        )
    })?;

    Ok(contents.trim().to_string())
}

/// Generates a fresh `ed25519` key pair at `private_key` (writing the matching
/// `.pub` alongside it), with an empty passphrase for non-interactive use.
fn generate_key(path: &Path) -> Result<()> {
    let output = Command::new("ssh-keygen")
        .arg("-t") // key type: https://man.openbsd.org/ssh-keygen.1#t
        .arg("ed25519")
        .arg("-f") // output keyfile path: https://man.openbsd.org/ssh-keygen.1#f
        .arg(path)
        .arg("-N") // new passphrase: https://man.openbsd.org/ssh-keygen.1#N
        .arg("")
        .arg("-C") // comment: https://man.openbsd.org/ssh-keygen.1#C
        .arg(HOST_ALIAS)
        .output()?;

    if !output.status.success() {
        bail!(
            "failed to generate a key at {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_push_script_embeds_function_and_invokes_it_for_ec2_user() {
        let script = build_push_script("ssh-ed25519 AAAAKEY awsome");

        // The embedded function definition is present...
        assert!(script.contains("install_key() {"));
        assert!(script.contains("getent passwd \"$user\""));
        assert!(script.contains("install -d -m 700 -o \"$user\" -g \"$user\""));
        assert!(script.contains("grep -qF"));
        // ...and it is invoked with the remote user and the quoted key.
        assert!(script.contains("install_key ec2-user 'ssh-ed25519 AAAAKEY awsome'"));
    }

    #[test]
    fn build_push_script_has_no_carriage_returns() {
        let script = build_push_script("ssh-ed25519 AAAAKEY awsome");
        assert!(!script.contains('\r'));
    }

    #[test]
    fn build_push_script_escapes_single_quotes() {
        let script = build_push_script("key-with-'quote'");
        assert!(script.contains("'key-with-'\\''quote'\\'''"));
    }

    #[test]
    fn read_public_ssh_key_trims_the_key_file_contents() {
        let temp_dir =
            std::env::temp_dir().join(format!("awsome-ssh-key-test-{}", std::process::id()));
        let paths = SshPaths::from_home(temp_dir.clone()).unwrap();
        std::fs::create_dir_all(&paths.ssh_dir).unwrap();
        std::fs::write(
            &paths.public_key,
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITest awsome\n",
        )
        .unwrap();

        let public_key = read_public_ssh_key(&paths).unwrap();
        std::fs::remove_dir_all(temp_dir).unwrap();

        assert_eq!(
            public_key,
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITest awsome"
        );
    }
}
