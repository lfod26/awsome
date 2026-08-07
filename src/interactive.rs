use anyhow::{Context, Result};
use dialoguer::{FuzzySelect, theme::ColorfulTheme};

/// Prompts the user to fuzzy-search and pick one item, returning the
/// index of the selection.
pub fn select_index<T: std::fmt::Display>(prompt: &str, items: &[T]) -> Result<usize> {
    FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .items(
            items
                .iter()
                .map(|v| console::strip_ansi_codes(&v.to_string()).into_owned()),
        )
        .default(0)
        .report(false)
        .interact()
        .context("failed to read selection")
}
