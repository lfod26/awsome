//! A tiny terminal spinner for wrapping blocking calls (e.g. `aws ec2
//! wait ...`) that can take a while with no other output, so it's clear
//! the app hasn't hung.

use std::time::Duration;

use indicatif::ProgressBar;

/// Runs `f` to completion while animating a spinner with `message`, then
/// clears the spinner line. `indicatif` automatically falls back to a
/// single static line when output isn't a terminal (piped/redirected),
/// so scripted usage stays clean.
pub fn with_spinner<T>(message: impl Into<String>, f: impl FnOnce() -> T) -> T {
    let mut message = message.into();
    message.push_str("...");

    let spinner = ProgressBar::new_spinner();
    spinner.set_message(message);
    spinner.enable_steady_tick(Duration::from_millis(80));

    let result = f();

    spinner.finish_and_clear();
    result
}
