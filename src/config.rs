use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use console::style;
use serde::{Deserialize, Serialize};

use super::{
    client::{describe_instances, ensure_logged_in, list_profiles},
    interactive::select_index,
    logger_success,
};

const CONFIG_NAME: &str = "awsome_conf.json";

/// A single profile + instance pairing that `start`/`stop` can act on.
#[derive(Serialize, Deserialize, Clone)]
pub struct ProfileGroup {
    pub profile: String,
    pub instance_id: String,
}

impl std::fmt::Display for ProfileGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {}",
            style(&self.instance_id).dim(),
            style(&self.profile).underlined()
        )
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

impl std::fmt::Display for AwsomeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.groups.is_empty() {
            return write!(f, "{} empty configuration", style("i").blue());
        }

        for (i, pg) in self.groups.iter().enumerate() {
            let idx = i + 1;
            let bold_idx = style(idx).bold();

            if i == self.selected {
                write!(f, "{}. {} {pg}", bold_idx, style("✓").green())?;
            } else {
                write!(f, "{}.   {pg}", bold_idx)?;
            }

            if idx != self.groups.len() {
                writeln!(f)?;
            }
        }

        Ok(())
    }
}

impl AwsomeConfig {
    pub fn get_selected(&self) -> Result<&ProfileGroup> {
        self.ensure_groups_exist()?;

        self.groups.get(self.selected).with_context(|| {
            format!(
                "config corruption: selected config index (#{}) \
                is out of bounds",
                self.selected + 1
            )
        })
    }

    pub fn set_selected(&mut self, idx: Option<usize>) -> Result<()> {
        self.ensure_groups_exist()?;

        let selected_idx = if let Some(i) = idx {
            i.checked_sub(1)
                .context("expected an index number greater than 0")?
        } else {
            select_index("config to select", &self.groups)?
        };

        let group = self.groups.get(selected_idx).with_context(|| {
            format!(
                "#{} is out of index for the current configuration",
                selected_idx + 1
            )
        })?;

        self.selected = selected_idx;

        self.save()?;

        logger_success!("selected {group}");

        Ok(())
    }

    pub fn add(&mut self, profile: Option<String>, instance_id: Option<String>) -> Result<()> {
        let (profile, instance_id) = Self::resolve_profile_and_instance_id(profile, instance_id)?;

        let group = ProfileGroup {
            profile,
            instance_id,
        };

        let group_str = group.to_string();

        self.groups.push(group);

        self.save()?;

        logger_success!("added {group_str}");

        Ok(())
    }

    pub fn remove(&mut self, idx: Option<usize>) -> Result<()> {
        self.ensure_groups_exist()?;

        let selected_idx = if let Some(i) = idx {
            i.checked_sub(1)
                .context("expected an index number greater than 0")?
        } else {
            select_index("config to remove", &self.groups)?
        };

        let group = self.remove_group(selected_idx)?;

        self.save()?;

        logger_success!("removed {group}");

        Ok(())
    }

    fn resolve_profile_and_instance_id(
        profile: Option<String>,
        instance_id: Option<String>,
    ) -> Result<(String, String)> {
        let profiles = list_profiles()?;

        let profile = if let Some(p) = profile {
            if !profiles.iter().any(|s| s == &p) {
                bail!(
                    "AWS CLI profile {p} was not found. Available profiles: {}",
                    profiles.join(", ")
                );
            }

            p
        } else {
            let idx = select_index("profile", &profiles)?;

            let p = profiles
                .into_iter()
                .nth(idx)
                .context("failed to get profile")?;

            logger_success!("selected profile {p}");

            p
        };

        ensure_logged_in(&profile)?;

        let instances = describe_instances(&profile, None)?;

        let instance_id = if let Some(i_id) = instance_id {
            if !instances.iter().any(|i| i.instance_id == i_id) {
                bail!("EC2 instance {i_id} was not found under profile `{profile}`.");
            }

            i_id
        } else {
            let idx = select_index("instance", &instances)?;

            let inst = instances
                .into_iter()
                .nth(idx)
                .context("failed to get instance")?;

            logger_success!("selected instance {inst}");

            inst.instance_id
        };

        Ok((profile, instance_id))
    }

    fn remove_group(&mut self, idx: usize) -> Result<ProfileGroup> {
        if idx >= self.groups.len() {
            bail!("index out of bounds for available configs");
        }

        if idx == self.selected {
            bail!("index cannot be the same as the selected config");
        }

        if idx < self.selected {
            self.selected -= 1;
        }

        Ok(self.groups.remove(idx))
    }

    fn ensure_groups_exist(&self) -> Result<()> {
        if self.groups.is_empty() {
            bail!("no configured groups available");
        }

        Ok(())
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
    /// (empty) config when there is no config file yet.
    pub fn load() -> Result<Self> {
        let path = Self::resolve_path().context("failed to resolve config file path")?;

        if !path.exists() {
            return Ok(Self::default());
        }

        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to open config file at {}", path.display()))?;

        serde_json::from_str::<AwsomeConfig>(&contents).map_err(Into::into)
    }

    /// Saves this config to the config file next to the executable.
    pub fn save(&self) -> Result<()> {
        let path = Self::resolve_path().context("failed to resolve config file path")?;

        let file = std::fs::File::create(&path)
            .with_context(|| format!("failed to create config file at {}", path.display()))?;

        serde_json::to_writer_pretty(file, self)
            .with_context(|| format!("failed to write config file at {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(profile: &str, instance_id: &str) -> ProfileGroup {
        ProfileGroup {
            profile: profile.to_string(),
            instance_id: instance_id.to_string(),
        }
    }

    #[test]
    fn config_serde_round_trips_object_format() {
        let config = AwsomeConfig {
            selected: 1,
            groups: vec![group("a", "i-a"), group("b", "i-b")],
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"selected\""), "{json}");
        assert!(json.contains("\"groups\""), "{json}");

        let back: AwsomeConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.selected, 1);
        assert_eq!(back.groups.len(), 2);
        assert_eq!(back.groups[0].profile, "a");
        assert_eq!(back.groups[1].instance_id, "i-b");
    }

    #[test]
    fn empty_object_deserializes_to_default() {
        let config: AwsomeConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config.selected, 0);
        assert!(config.groups.is_empty());
    }

    #[test]
    fn missing_selected_defaults_to_zero() {
        let json = r#"{ "groups": [ { "profile": "a", "instance_id": "i-a" } ] }"#;
        let config: AwsomeConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.selected, 0);
        assert_eq!(config.groups.len(), 1);
    }

    #[test]
    fn remove_group_shifts_selection_down_when_earlier_group_removed() {
        let mut config = AwsomeConfig {
            selected: 2,
            groups: vec![group("a", "i-a"), group("b", "i-b"), group("c", "i-c")],
        };

        let removed = config.remove_group(0).unwrap();
        assert_eq!(removed.profile, "a");
        // "c" was selected (index 2); after dropping "a" it's now index 1.
        assert_eq!(config.selected, 1);
        assert_eq!(config.groups.len(), 2);
        assert_eq!(config.groups[config.selected].profile, "c");
    }

    #[test]
    fn remove_group_errors_when_removing_selected_group() {
        let mut config = AwsomeConfig {
            selected: 1,
            groups: vec![group("a", "i-a"), group("b", "i-b"), group("c", "i-c")],
        };

        assert!(config.remove_group(1).is_err());
        // Nothing should have changed on error.
        assert_eq!(config.selected, 1);
        assert_eq!(config.groups.len(), 3);
    }

    #[test]
    fn remove_group_keeps_selection_when_later_group_removed() {
        let mut config = AwsomeConfig {
            selected: 0,
            groups: vec![group("a", "i-a"), group("b", "i-b"), group("c", "i-c")],
        };

        config.remove_group(2).unwrap();
        assert_eq!(config.selected, 0);
        assert_eq!(config.groups[config.selected].profile, "a");
    }

    #[test]
    fn profile_group_display() {
        assert_eq!(
            console::strip_ansi_codes(&group("james-bond", "i-045").to_string()),
            "i-045 james-bond"
        );
    }

    #[test]
    fn get_selected_rejects_an_out_of_bounds_persisted_selection() {
        let config = AwsomeConfig {
            selected: 1,
            groups: vec![group("a", "i-a")],
        };

        let error = match config.get_selected() {
            Ok(_) => panic!("expected an out-of-bounds selected group to fail"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("config corruption"), "{error}");
        assert!(error.contains("#2"), "{error}");
    }

    #[test]
    fn remove_group_rejects_an_out_of_bounds_index_without_mutating_config() {
        let mut config = AwsomeConfig {
            selected: 0,
            groups: vec![group("a", "i-a")],
        };

        assert!(config.remove_group(1).is_err());
        assert_eq!(config.groups.len(), 1);
        assert_eq!(config.selected, 0);
    }
}
