use anyhow::{Context, Result};
use dialoguer::{FuzzySelect, theme::ColorfulTheme};

/// Prompts the user to fuzzy-search and pick one item, returning the
/// index of the selection.
pub fn select_index<T, I>(prompt: &str, items: I) -> Result<usize>
where
    T: ToString,
    I: IntoIterator<Item = T>,
{
    FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .items(items)
        .default(0)
        .report(false)
        .interact()
        .context("failed to read selection")
}
