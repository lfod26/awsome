use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::client::{describe_instances, list_profiles};

use super::{
    client::InstanceEntry,
    interactive::{select, select_index},
};

const CONFIG_NAME: &str = "awsome_conf.json";

fn get_profile(profile: Option<String>) -> Result<String> {
    let profiles = list_profiles()?;

    if let Some(p) = profile {
        if !profiles.iter().any(|s| s == &p) {
            bail!(
                "AWS CLI profile `{p}` was not found. Available profiles: {}",
                profiles.join(", ")
            );
        }

        return Ok(p.to_string());
    }

    let profile = select("profile", profiles)?;

    println!("Selected profile: {profile}");

    Ok(profile)
}

fn get_instance(profile: &str, instance_id: Option<String>) -> Result<String> {
    let instances = describe_instances(profile, None)?;

    if let Some(i_id) = instance_id {
        if instances.iter().any(|i| i.instance_id == i_id) {
            return Ok(i_id.to_string());
        }

        bail!("EC2 instance `{i_id}` was not found under profile `{profile}`.");
    }

    let InstanceEntry {
        instance_id, name, ..
    } = select("instance", instances)?;

    println!("Selected instance: {name} ({instance_id})");

    Ok(instance_id)
}

/// A single profile + instance pairing that `start`/`stop` can act on.
#[derive(Serialize, Deserialize, Clone)]
pub struct ProfileGroup {
    pub profile: String,
    pub instance_id: String,
}

impl std::fmt::Display for ProfileGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.profile, self.instance_id)
    }
}

/// Config file contents: a JSON object with a `selected` index (which
/// group `start`/`stop` act on) and a `groups` array of profile+instance
/// pairings. Supports multiple groups so this tool can manage more than
/// one instance (each possibly under a different AWS CLI profile).
#[derive(Serialize, Deserialize, Default)]
pub struct AwsomeConfig {
    #[serde(default)]
    selected: usize,
    #[serde(default)]
    groups: Vec<ProfileGroup>,
}

impl AwsomeConfig {
    /// Returns the currently selected group, per the `selected` index.
    /// Callers must ensure the config is non-empty first (see
    /// [`is_empty`](Self::is_empty)). If `selected` is out of range (e.g.
    /// after groups were removed, or a hand-edited config), a warning is
    /// printed, `selected` reverts to the first group, and that correction
    /// is written back to the config file.
    pub fn selected_group(&mut self) -> Result<&ProfileGroup> {
        if self.selected >= self.groups.len() {
            eprintln!(
                "⚠️  Selected group #{} is out of range ({} configured). Using the first one.",
                self.selected + 1,
                self.groups.len()
            );
            self.selected = 0;
            self.save()?;
        }

        Ok(&self.groups[self.selected])
    }

    /// Sets which group `start`/`stop` act on and persists it. `index` is
    /// 1-based (as shown by `configure show`); if omitted, prompts
    /// interactively.
    pub fn set_selected(&mut self, index: Option<usize>) -> Result<()> {
        if self.groups.is_empty() {
            println!("No configuration found. Run `awsome configure add` first.");
            return Ok(());
        }

        let selected = if let Some(i) = index {
            if i == 0 || i > self.groups.len() {
                self.show();
                bail!(
                    "Index {i} is out of range (expected 1-{})",
                    self.groups.len()
                );
            }

            i - 1
        } else {
            select_index("group to select", &self.groups)?
        };

        self.selected = selected;

        self.save()?;

        println!("✅ Selected {}", self.groups[self.selected]);

        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    pub fn add(&mut self, profile: Option<String>, instance_id: Option<String>) -> Result<()> {
        let profile = get_profile(profile)?;
        let instance_id = get_instance(&profile, instance_id)?;

        let group = ProfileGroup {
            profile,
            instance_id,
        };

        let group_str = group.to_string();

        self.groups.push(group);

        self.save()?;

        println!("✅ Added {group_str}");

        Ok(())
    }

    pub fn remove(&mut self, index: Option<usize>) -> Result<()> {
        if self.groups.is_empty() {
            println!("No configuration found. Nothing to remove.");
            return Ok(());
        }

        let remove_index = if let Some(i) = index {
            if i == 0 || i > self.groups.len() {
                bail!(
                    "Index {i} is out of range (expected 1-{})",
                    self.groups.len()
                );
            }

            i - 1
        } else {
            select_index("group to remove", &self.groups)?
        };

        let removed = self.groups.remove(remove_index);

        // Keep `selected` pointing at the same logical group where
        // possible: shift it down if an earlier group was removed, or
        // reset to the first group if the selected one itself was removed.
        if remove_index < self.selected {
            self.selected -= 1;
        } else if remove_index == self.selected {
            self.selected = 0;
        }

        self.save()?;

        println!("✅ Removed {removed}");

        Ok(())
    }

    pub fn show(&self) {
        if self.groups.is_empty() {
            println!("No configuration found. Run `awsome configure add` first.");
            return;
        }

        for (i, group) in self.groups.iter().enumerate() {
            let marker = if i == self.selected {
                " (selected)"
            } else {
                ""
            };
            println!("{}. {group}{marker}", i + 1);
        }
    }

    fn resolve_path() -> std::io::Result<PathBuf> {
        #[cfg(debug_assertions)]
        let mut base = std::env::current_dir()?;

        #[cfg(not(debug_assertions))]
        let mut base = std::env::current_exe().map(|mut p| {
            p.pop();
            p
        })?;

        base.push(CONFIG_NAME);
        Ok(base)
    }

    /// Loads the config file next to the executable. Returns a default
    /// (empty) config when there is no config file yet. If genuinely
    /// malformed JSON is found, the invalid file is backed up (so it
    /// isn't silently lost), a warning is printed, and an empty config is
    /// returned.
    pub fn load() -> Result<Self> {
        let path = Self::resolve_path().context("failed to resolve config file path")?;

        if !path.exists() {
            return Ok(Self::default());
        }

        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to open config file at {}", path.display()))?;

        match serde_json::from_str::<AwsomeConfig>(&contents) {
            Ok(config) => Ok(config),
            Err(err) => {
                let backup_path = path.with_extension("json.bak");
                match std::fs::rename(&path, &backup_path) {
                    Ok(()) => eprintln!(
                        "⚠️  Config file at {} is invalid ({err}). \
                         Backed it up to {} and will reconfigure.",
                        path.display(),
                        backup_path.display()
                    ),
                    Err(rename_err) => eprintln!(
                        "⚠️  Config file at {} is invalid ({err}), and it could not be \
                         backed up ({rename_err}). It will be overwritten when reconfiguring.",
                        path.display()
                    ),
                }
                Ok(Self::default())
            }
        }
    }

    /// Saves this config to the config file next to the executable.
    pub fn save(&self) -> Result<()> {
        let path = Self::resolve_path().context("failed to resolve config file path")?;
        let file = std::fs::File::create(&path)
            .with_context(|| format!("failed to create config file at {}", path.display()))?;
        serde_json::to_writer_pretty(file, self)
            .with_context(|| format!("failed to write config file at {}", path.display()))?;

        println!("✅ Saved config to {}", path.display());
        Ok(())
    }
}
