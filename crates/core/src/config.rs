use crate::model::Profile;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    path::PathBuf,
};

#[derive(Debug, Clone)]
pub struct Paths {
    pub config: PathBuf,
    pub data: PathBuf,
    pub applications: PathBuf,
}

impl Paths {
    pub fn discover() -> Result<Self> {
        let home = env::var_os("HOME").context("HOME is not set")?;
        let config = env::var_os("XDG_CONFIG_HOME")
            .filter(|p| std::path::Path::new(p).is_absolute())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(&home).join(".config"));
        let data = env::var_os("XDG_DATA_HOME")
            .filter(|p| std::path::Path::new(p).is_absolute())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(&home).join(".local/share"));
        Ok(Self {
            config: config.join("igpu-man/config.toml"),
            data: data.join("igpu-man"),
            applications: data.join("applications"),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub sample_interval_ms: u64,
    pub recording_interval_ms: u64,
    pub storage_limit_mb: u64,
    pub stutter_threshold_ms: f64,
    pub notifications: bool,
    pub aliases: BTreeMap<String, String>,
    pub profiles: Vec<Profile>,
    /// Executable identities, never PIDs; these survive application restarts.
    pub managed_elsewhere: BTreeSet<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            sample_interval_ms: 1000,
            recording_interval_ms: 250,
            storage_limit_mb: 1024,
            stutter_threshold_ms: 33.3,
            notifications: false,
            aliases: BTreeMap::new(),
            profiles: Vec::new(),
            managed_elsewhere: BTreeSet::new(),
        }
    }
}

impl Config {
    pub fn load(paths: &Paths) -> Result<Self> {
        let config: Self = match crate::fsutil::read_text_limited(&paths.config, 1024 * 1024) {
            Ok(s) => toml::from_str(&s).context("Invalid config.toml")?,
            Err(e)
                if e.downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                Self::default()
            }
            Err(e) => return Err(e),
        };
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            bail!("Unsupported configuration schema");
        }
        if self.sample_interval_ms < 250 || self.recording_interval_ms < 100 {
            bail!("Sampling intervals must be at least 250 ms / 100 ms");
        }
        if self.storage_limit_mb < 16 || self.storage_limit_mb > 1_048_576 {
            bail!("Storage limit must be between 16 and 1048576 MiB");
        }
        if !self.stutter_threshold_ms.is_finite() || self.stutter_threshold_ms <= 0.0 {
            bail!("Stutter threshold must be positive");
        }
        let mut names = std::collections::BTreeSet::new();
        for executable in &self.managed_elsewhere {
            if !std::path::Path::new(executable).is_absolute() || executable.contains('\0') {
                bail!("Managed-elsewhere identities must be absolute executable paths");
            }
        }
        for profile in &self.profiles {
            validate_profile(profile)?;
            if !names.insert(&profile.name) {
                bail!("Duplicate profile name");
            }
        }
        Ok(())
    }
    pub fn save(&self, paths: &Paths) -> Result<()> {
        self.validate()?;
        let content = toml::to_string_pretty(self)?;
        if content.len() > 1024 * 1024 {
            bail!("Configuration exceeds the 1 MiB limit");
        }
        crate::fsutil::atomic_private_write(&paths.config, content.as_bytes())?;
        Ok(())
    }
}

/// Public launch/save inputs cannot manufacture durable launcher-recovery state.
pub fn validate_profile_input(profile: &Profile) -> Result<()> {
    anyhow::ensure!(
        profile.pending_launcher.is_none(),
        "Pending launcher intent is internal recovery state, not a profile input"
    );
    validate_profile(profile)
}

pub fn validate_profile(profile: &Profile) -> Result<()> {
    if let Some(pending) = &profile.pending_launcher {
        anyhow::ensure!(
            pending.schema_version == 1 && pending.content.len() <= 128 * 1024,
            "Unsupported or oversized pending launcher intent"
        );
    }
    if profile.name.is_empty()
        || profile.name.len() > 80
        || !profile
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_".contains(c))
    {
        bail!("Profile names may contain 1–80 ASCII letters, digits, '-' and '_'");
    }
    if profile.executable.is_empty()
        || profile.executable.contains('\0')
        || profile.args.iter().any(|s| s.contains('\0'))
    {
        bail!("Invalid executable or arguments");
    }
    if profile.mangohud && !profile.record {
        bail!("MangoHud profiles must enable recording");
    }
    if profile
        .environment
        .iter()
        .any(|(key, value)| !valid_environment_key(key) || value.contains('\0'))
    {
        bail!("Invalid profile environment assignment");
    }
    if profile
        .working_directory
        .as_ref()
        .is_some_and(|p| !std::path::Path::new(p).is_absolute() || p.contains('\0'))
    {
        bail!("Working directory must be an absolute path");
    }
    if let Some(desktop) = &profile.desktop {
        if desktop
            .file_argument
            .is_some_and(|(index, code)| index > profile.args.len() || !"fFuU".contains(code))
        {
            bail!("Invalid desktop file/URL argument position");
        }
    }
    Ok(())
}

pub fn valid_environment_key(key: &str) -> bool {
    let mut chars = key.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod compatibility_tests {
    use super::*;
    #[test]
    fn config_typos_and_unknown_nested_fields_are_rejected_before_saving() {
        assert!(toml::from_str::<Config>("recoding_interval_ms=250").is_err());
        assert!(toml::from_str::<Config>(
            "[[profiles]]\nname='test'\nexecutable='/bin/true'\ngpu='igpu'\nreccord=true"
        )
        .is_err());
        let old: Config = toml::from_str("schema_version=1").unwrap();
        assert_eq!(old.recording_interval_ms, 250);
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config: dir.path().join("config.toml"),
            data: dir.path().into(),
            applications: dir.path().join("apps"),
        };
        let original = "notifications=false\nfuture_setting='retain this text'\n";
        std::fs::write(&paths.config, original).unwrap();
        assert!(Config::load(&paths).is_err());
        assert_eq!(std::fs::read_to_string(&paths.config).unwrap(), original);
    }
}
