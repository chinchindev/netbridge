use crate::storage::{atomic_write, stamp};
use crate::{config::Config, speed_view::Preferences};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub const MAX_BACKUP_BYTES: usize = 512 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsBackup {
    pub format: String,
    pub version: u32,
    pub config: Config,
    pub preferences: Preferences,
}
impl SettingsBackup {
    pub fn new(config: Config, preferences: Preferences) -> Self {
        Self {
            format: "netbridge-settings".into(),
            version: 1,
            config,
            preferences,
        }
    }
    pub fn validate(&self) -> io::Result<()> {
        if self.format != "netbridge-settings" || self.version != 1 {
            return Err(io::Error::other(
                "Unsupported NetBridge backup format/version",
            ));
        }
        self.config.validate()?;
        if serde_json::to_vec_pretty(&self.config)?.len() > 128 * 1024 {
            return Err(io::Error::other("Configuration exceeds 128 KB"));
        }
        Ok(())
    }
    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() > MAX_BACKUP_BYTES {
            return Err(io::Error::other("Backup exceeds 512 KB"));
        }
        let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
        let value: Self = serde_json::from_slice(bytes)?;
        value.validate()?;
        Ok(value)
    }
    pub fn to_json(&self) -> io::Result<String> {
        self.validate()?;
        let json = serde_json::to_string_pretty(self)?;
        if json.len() > MAX_BACKUP_BYTES {
            return Err(io::Error::other("Backup exceeds 512 KB"));
        }
        Ok(json)
    }
}
fn read_optional(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match crate::storage::read_limited(path, MAX_BACKUP_BYTES) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}
pub fn write_backup_file(path: &Path, backup: &SettingsBackup) -> io::Result<()> {
    atomic_write(path, backup.to_json()?.as_bytes())
}
pub fn restore_files(
    config_path: &Path,
    preferences_path: &Path,
    backup: &SettingsBackup,
) -> io::Result<PathBuf> {
    backup.validate()?;
    let original_preferences = read_optional(preferences_path)?;
    let recovery = config_path
        .parent()
        .ok_or_else(|| io::Error::other("Missing settings directory"))?
        .join("settings-backups")
        .join(stamp());
    fs::create_dir_all(&recovery)?;
    if config_path.exists() {
        fs::copy(config_path, recovery.join("config.json"))?;
    }
    if let Some(bytes) = &original_preferences {
        fs::write(recovery.join("toolbox-preferences.json"), bytes)?;
    }
    let config = serde_json::to_vec_pretty(&backup.config)?;
    let preferences = serde_json::to_vec_pretty(&backup.preferences)?;
    atomic_write(preferences_path, &preferences)?;
    if let Err(error) = atomic_write(config_path, &config) {
        // An absent preferences file means default settings. Preserve those semantics on rollback.
        let defaults = serde_json::to_vec_pretty(&Preferences::default())?;
        if let Err(rollback) = atomic_write(
            preferences_path,
            original_preferences.as_deref().unwrap_or(&defaults),
        ) {
            return Err(io::Error::other(format!(
                "Restore failed: {error}; preferences rollback failed: {rollback}; recovery: {}",
                recovery.display()
            )));
        }
        return Err(io::Error::other(format!(
            "Restore failed: {error}; previous preferences restored; recovery: {}",
            recovery.display()
        )));
    }
    Ok(recovery)
}
