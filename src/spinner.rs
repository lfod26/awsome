//! A tiny terminal spinner for wrapping blocking calls (e.g. `aws ec2
//! wait ...`) that can take a while with no other output, so it's clear
//! the app hasn't hung.

use std::{fmt::Display, time::Duration};

use indicatif::ProgressBar;

/// Runs `f` to completion while animating a spinner with `message`.
/// Beware that if `f` needs to print to console it should use [`ProgressBar`]
/// in the future.
pub fn with_spinner<T>(message: impl Display, f: impl FnOnce() -> T) -> T {
    let mut message = message.to_string();
    message.push_str("...");

    let pb = ProgressBar::new_spinner();
    pb.set_message(message);
    pb.enable_steady_tick(Duration::from_millis(80));

    let result = f();

    pb.finish_and_clear();

    result
}
