use anyhow::{Context, Result};
use dialoguer::{FuzzySelect, theme::ColorfulTheme};

/// Prompts the user to fuzzy-search and pick one item, returning the
/// index of the selection. Shared by `select` and `select_ref`.
pub fn select_index<T: std::fmt::Display>(prompt: &str, items: &[T]) -> Result<usize> {
    FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .items(items)
        .default(0)
        .interact()
        .context("failed to read selection")
}

/// Prompts the user to fuzzy-search and pick one item from `items`,
/// returning it by value (without cloning) via `Vec::swap_remove`.
pub fn select<T: std::fmt::Display>(prompt: &str, mut items: Vec<T>) -> Result<T> {
    let selection = select_index(prompt, &items)?;
    Ok(items.swap_remove(selection))
}
