use crate::{
    adapters::Adapter,
    policy::{self, Route},
};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub pattern: String,
    pub route: Route,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckTarget {
    pub name: String,
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub port: u16,
    pub lan_id: String,
    pub usb_id: String,
    pub rules: Vec<Rule>,
    pub adb_path: String,
    #[serde(default)]
    pub check_targets: Vec<CheckTarget>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 17890,
            lan_id: String::new(),
            usb_id: String::new(),
            rules: Vec::new(),
            adb_path: "auto".into(),
            check_targets: Vec::new(),
        }
    }
}

impl Config {
    pub const MAX_BYTES: usize = 128 * 1024;
    pub fn select(&self, host: &str) -> Route {
        self.rules
            .iter()
            .find(|r| policy::matches(&r.pattern, host))
            .map(|r| r.route)
            .unwrap_or(Route::System)
    }
    pub fn auto_select(&mut self, adapters: &[Adapter]) {
        if self.usb_id.is_empty() {
            let candidates: Vec<_> = adapters.iter().filter(|a| a.is_usb_network()).collect();
            if let [a] = candidates.as_slice() {
                self.usb_id = a.id.clone();
            }
        }
        if self.lan_id.is_empty() {
            if let Some(a) = adapters
                .iter()
                .find(|a| a.id != self.usb_id && a.if_type == 6 && !a.is_usb_network())
            {
                self.lan_id = a.id.clone();
            }
        }
    }
    pub fn validate(&self) -> io::Result<()> {
        if self.port == 0
            || self.rules.len() > 200
            || self.check_targets.len() > 4
            || self
                .check_targets
                .iter()
                .map(|t| (t.host.trim_end_matches('.').to_ascii_lowercase(), t.port))
                .collect::<std::collections::HashSet<_>>()
                .len()
                != self.check_targets.len()
            || self.check_targets.iter().any(|t| {
                t.port == 0
                    || !policy::valid_host(&t.host)
                    || t.name.trim().is_empty()
                    || t.name.chars().count() > 64
                    || t.name.chars().any(char::is_control)
            })
            || [&self.lan_id, &self.usb_id, &self.adb_path]
                .iter()
                .any(|value| value.len() > 4096 || value.chars().any(char::is_control))
            || self
                .rules
                .iter()
                .any(|r| !policy::valid_pattern(&r.pattern))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid port or routing rule",
            ));
        }
        Ok(())
    }
    pub fn load(path: &Path) -> io::Result<Self> {
        match crate::storage::read_limited(path, Self::MAX_BYTES) {
            Ok(data) => Self::decode(&data),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }
    pub fn decode(data: &[u8]) -> io::Result<Self> {
        if data.len() > Self::MAX_BYTES {
            return Err(io::Error::other("Configuration is too large"));
        }
        let data = data.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(data);
        let mut config: Self = serde_json::from_slice(data).map_err(io::Error::other)?;
        if let Ok(directory) = std::env::var("LOCALAPPDATA") {
            let previous = format!("{directory}\\Android\\Sdk\\platform-tools\\adb.exe");
            if config
                .adb_path
                .replace('/', "\\")
                .eq_ignore_ascii_case(&previous)
            {
                config.adb_path = "auto".into();
            }
        }
        config.validate()?;
        Ok(config)
    }
    pub fn save(&self, path: &Path) -> io::Result<()> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        if bytes.len() > Self::MAX_BYTES {
            return Err(io::Error::other("Configuration is too large"));
        }
        match crate::storage::read_limited(path, Self::MAX_BYTES) {
            Ok(previous) => {
                Self::decode(&previous)?;
                crate::storage::atomic_write(&path.with_extension("json.bak"), &previous)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        crate::storage::atomic_write(path, &bytes)
    }
    pub fn recover_last_good(path: &Path) -> io::Result<(Self, Option<PathBuf>)> {
        let candidate =
            crate::storage::read_limited(&path.with_extension("json.bak"), Self::MAX_BYTES)?;
        let config = Self::decode(&candidate)?;
        let preserved = crate::storage::preserve_file(path)?;
        crate::storage::atomic_write(path, &candidate)?;
        Ok((config, preserved))
    }
    pub fn last_good(path: &Path) -> io::Result<Self> {
        Self::decode(&crate::storage::read_limited(
            &path.with_extension("json.bak"),
            Self::MAX_BYTES,
        )?)
    }
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("NetBridge")
}
