//! Writing and maintaining the managed `Host awsome` entry in `~/.ssh/config`,
//! whose `ProxyCommand` calls back into `awsome ssh-proxy`.

use std::fs;

use anyhow::{Context, Result, bail};

use super::{
    super::{logger_info, logger_success},
    HOST_ALIAS, REMOTE_USER, SshPaths,
    path_util::harden_file_permissions,
};

const MANAGED_BLOCK_BEGIN: &str = "# >>> awsome managed >>>";
const MANAGED_BLOCK_END: &str = "# <<< awsome managed <<<";

/// Ensures `~/.ssh/config` contains the managed `Host awsome` block, creating
/// the file if needed and replacing any previous managed block in place.
pub fn ensure_ssh_config(paths: &SshPaths) -> Result<()> {
    let conf = fs::read_to_string(&paths.config)
        .or_else(|err| {
            if matches!(err.kind(), std::io::ErrorKind::NotFound) {
                Ok(String::new())
            } else {
                Err(err)
            }
        })
        .with_context(|| format!("failed to read {}", paths.config.display()))?;

    let proxy_command = format!("{HOST_ALIAS} ssh-proxy %p");
    let block = render_managed_block(&paths.ssh_conf_relative_private_key, &proxy_command);

    let updated_conf = upsert_config(&conf, &block)?;

    if updated_conf != conf {
        fs::write(&paths.config, &updated_conf)
            .with_context(|| format!("failed to write {}", paths.config.display()))?;

        harden_file_permissions(&paths.config);

        logger_success!(
            "updated {} with a managed `Host {HOST_ALIAS}` entry.",
            paths.config.display()
        );
    } else {
        logger_info!(
            "`Host {HOST_ALIAS}` entry in {} is already up to date",
            paths.config.display()
        );
    }

    Ok(())
}

/// Renders the managed SSH-config block (no trailing newline).
fn render_managed_block(identity_file: &str, proxy_command: &str) -> String {
    format!(
        "{MANAGED_BLOCK_BEGIN}\n\
         # Added by the awsome CLI (`awsome setup-ssh`). Do not edit this block by\n\
         # hand; it is regenerated automatically and your changes will be lost.\n\
         Host {HOST_ALIAS}\n    \
         User {REMOTE_USER}\n    \
         IdentityFile {identity_file}\n    \
         IdentitiesOnly yes\n    \
         ProxyCommand {proxy_command}\n    \
         # This alias tunnels to whichever instance is currently selected, and\n    \
         # each instance has its own host key, so host-key pinning under the\n    \
         # shared `{HOST_ALIAS}` name would break on every switch. Skip it.\n    \
         UserKnownHostsFile {null_known_hosts}\n    \
         StrictHostKeyChecking no\n    \
         LogLevel ERROR\n    \
         ServerAliveInterval 30\n\
         {MANAGED_BLOCK_END}",
        null_known_hosts = get_null_known_hosts(),
    )
}

/// Null device for `UserKnownHostsFile`, so no host key is pinned for the
/// shared alias.
fn get_null_known_hosts() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}

/// Replaces the managed block in `existing` with `block` (or appends it),
/// erroring on the first conflict or corruption found.
fn upsert_config(conf: &str, block: &str) -> Result<String> {
    let mut new_lines = Vec::<&str>::new();
    let mut is_inside_managed_block = false;
    let mut has_found_block = false;

    for line in conf.lines() {
        let trimmed_line = line.trim();

        if trimmed_line == MANAGED_BLOCK_BEGIN {
            if has_found_block {
                bail!("can only have one managed block");
            }

            if is_inside_managed_block {
                bail!("2 managed block begins one after the other");
            }

            is_inside_managed_block = true;
            continue;
        }

        if trimmed_line == MANAGED_BLOCK_END {
            if !is_inside_managed_block {
                bail!("2 managed block ends one after the other");
            }

            is_inside_managed_block = false;
            has_found_block = true;
            continue;
        }

        if is_inside_managed_block {
            continue;
        }

        if is_host_awsome_line(line) {
            bail!("seems like there already is an awsome host, this will conflict");
        }

        new_lines.push(line);
    }

    if is_inside_managed_block {
        bail!("seems like we found a managed block that is corrupted and isn't closed");
    }

    let mut final_block = new_lines.join("\n").trim_end().to_string();

    if !final_block.is_empty() {
        final_block.push('\n'); // end the last kept line
        final_block.push('\n'); // blank line separating it from the new block
    }

    final_block.push_str(block);
    final_block.push('\n'); // single trailing newline

    Ok(final_block)
}

/// Whether `line` is a `Host` directive that lists `awsome` as a pattern.
fn is_host_awsome_line(line: &str) -> bool {
    let mut parts = line.split_whitespace();

    matches!(parts.next(), Some(kw) if kw.eq_ignore_ascii_case("Host"))
        && parts.any(|p| p == HOST_ALIAS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_managed_block_contains_expected_directives() {
        let block = render_managed_block("~/.ssh/awsome", "awsome ssh-proxy %p");
        assert!(block.starts_with(MANAGED_BLOCK_BEGIN), "{block}");
        assert!(block.ends_with(MANAGED_BLOCK_END), "{block}");
        assert!(block.contains("Host awsome"));
        assert!(block.contains("User ec2-user"));
        assert!(block.contains("IdentityFile ~/.ssh/awsome"));
        assert!(block.contains("IdentitiesOnly yes"));
        assert!(block.contains("ProxyCommand awsome ssh-proxy %p"));
        assert!(block.contains("StrictHostKeyChecking no"));
        assert!(block.contains("ServerAliveInterval 30"));
        assert!(block.contains(&format!("UserKnownHostsFile {}", get_null_known_hosts())));
    }

    #[test]
    fn upsert_config_inserts_into_empty_config() {
        let block = render_managed_block("~/.ssh/awsome", "awsome ssh-proxy %p");
        let out = upsert_config("", &block).unwrap();
        assert!(out.contains(MANAGED_BLOCK_BEGIN));
        assert!(out.contains(MANAGED_BLOCK_END));
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn upsert_config_appends_after_existing_unrelated_content() {
        let existing = "Host other\n    HostName example.com\n";
        let block = render_managed_block("~/.ssh/awsome", "awsome ssh-proxy %p");
        let out = upsert_config(existing, &block).unwrap();

        assert!(out.starts_with("Host other"));
        assert!(out.contains(MANAGED_BLOCK_BEGIN));
        // A blank line separates the prior content from the managed block.
        assert!(out.contains("example.com\n\n# >>> awsome managed >>>"));
    }

    #[test]
    fn upsert_config_replaces_existing_managed_block_idempotently() {
        let block_v1 = render_managed_block("~/.ssh/awsome", "old-proxy %p");
        let existing = upsert_config("Host keep\n    HostName keep.example\n", &block_v1).unwrap();

        let block_v2 = render_managed_block("~/.ssh/awsome", "awsome ssh-proxy %p");
        let once = upsert_config(&existing, &block_v2).unwrap();

        assert!(once.contains("ProxyCommand awsome ssh-proxy %p"));
        assert!(!once.contains("old-proxy"));
        // Unrelated content is preserved.
        assert!(once.contains("Host keep"));
        // Exactly one managed block.
        assert_eq!(once.matches(MANAGED_BLOCK_BEGIN).count(), 1);
        assert_eq!(once.matches(MANAGED_BLOCK_END).count(), 1);

        // Running again with the same block is a no-op.
        let twice = upsert_config(&once, &block_v2).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn upsert_config_refuses_conflicting_manual_host() {
        let existing = "Host awsome\n    HostName manually-set.example\n";
        let block = render_managed_block("~/.ssh/awsome", "awsome ssh-proxy %p");
        assert!(upsert_config(existing, &block).is_err());
    }

    #[test]
    fn upsert_config_errors_on_unterminated_managed_block() {
        // A managed block whose end marker was lost should error rather than
        // silently leaving the block open (and swallowing the rest of the file).
        let existing = format!("{MANAGED_BLOCK_BEGIN}\nHost awsome\n    User ec2-user\n");
        let block = render_managed_block("~/.ssh/awsome", "awsome ssh-proxy %p");
        let err = upsert_config(&existing, &block).unwrap_err().to_string();
        assert!(err.contains("corrupted and isn't closed"), "{err}");
    }

    #[test]
    fn upsert_config_errors_on_end_marker_without_begin() {
        // A stray end marker with no matching begin marker should error
        // rather than being silently accepted.
        let existing = format!("Host other\n    HostName example.com\n{MANAGED_BLOCK_END}\n");
        let block = render_managed_block("~/.ssh/awsome", "awsome ssh-proxy %p");
        let err = upsert_config(&existing, &block).unwrap_err().to_string();
        assert!(err.contains("2 managed block ends"), "{err}");
    }

    #[test]
    fn is_host_awsome_line_matches_only_the_alias() {
        assert!(is_host_awsome_line("Host awsome"));
        assert!(is_host_awsome_line("  host   awsome  "));
        assert!(is_host_awsome_line("Host awsome other"));
        assert!(!is_host_awsome_line("Host awsome-prod"));
        assert!(!is_host_awsome_line("HostName awsome"));
        assert!(!is_host_awsome_line("    IdentityFile ~/.ssh/awsome"));
    }
}
