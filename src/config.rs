use crate::{
    adapters::Adapter,
    policy::{self, Route},
};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rule {
    pub pattern: String,
    pub route: Route,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub port: u16,
    pub lan_id: String,
    pub usb_id: String,
    pub rules: Vec<Rule>,
    pub adb_path: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 17890,
            lan_id: String::new(),
            usb_id: String::new(),
            rules: [
                ("git.etc.vn", Route::Lan),
                ("10.3.6.5", Route::Lan),
                ("*.onorca.dev", Route::Usb),
                ("103.122.82.124", Route::Usb),
            ]
            .into_iter()
            .map(|(p, r)| Rule {
                pattern: p.into(),
                route: r,
            })
            .collect(),
            adb_path: "auto".into(),
        }
    }
}

impl Config {
    pub fn select(&self, host: &str) -> Route {
        self.rules
            .iter()
            .find(|r| policy::matches(&r.pattern, host))
            .map(|r| r.route)
            .unwrap_or(Route::System)
    }
    pub fn auto_select(&mut self, adapters: &[Adapter]) {
        if self.usb_id.is_empty() {
            if let Some(a) = adapters.iter().find(|a| {
                a.description.to_ascii_lowercase().contains("rndis")
                    || a.description.to_ascii_lowercase().contains("remote ndis")
            }) {
                self.usb_id = a.id.clone();
            }
        }
        if self.lan_id.is_empty() {
            if let Some(a) = adapters
                .iter()
                .find(|a| a.id != self.usb_id && a.if_type == 6)
            {
                self.lan_id = a.id.clone();
            }
        }
    }
    pub fn validate(&self) -> io::Result<()> {
        if self.port == 0
            || self.rules.len() > 200
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
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = fs::read(path)?;
        if data.len() > 128 * 1024 {
            return Err(io::Error::other("Configuration is too large"));
        }
        let mut config: Self = serde_json::from_slice(&data).map_err(io::Error::other)?;
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
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if path.exists() {
            fs::copy(path, path.with_extension("json.bak"))?;
        }
        fs::write(
            path,
            serde_json::to_vec_pretty(self).map_err(io::Error::other)?,
        )
    }
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("NetBridge")
}
