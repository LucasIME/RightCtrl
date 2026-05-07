use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::apps::App;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub apps: BTreeMap<PathBuf, AppConfig>,
    #[serde(default)]
    pub settings: Settings,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub enabled: bool,
    pub letter_override: Option<char>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { enabled: true, letter_override: None }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub cycle_debounce_ms: u64,
    pub launch_at_login: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { cycle_debounce_ms: 800, launch_at_login: false }
    }
}

pub fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("dev", "rightctrl", "rightctrl")
        .context("could not resolve project dirs")
}

pub fn config_path() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    Ok(dirs.config_dir().join("config.toml"))
}

pub fn log_dir() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    Ok(dirs.data_local_dir().join("log"))
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let s = fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let cfg: Config = toml::from_str(&s)
            .with_context(|| format!("parsing {}", path.display()))?;
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let s = toml::to_string_pretty(self)?;
        atomic_write(&path, s.as_bytes())
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut tmp = path.to_path_buf();
    tmp.set_extension("toml.tmp");
    fs::write(&tmp, bytes)
        .with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

pub fn effective_letter(app: &App, cfg: &Config) -> Option<char> {
    if let Some(e) = cfg.apps.get(&app.exe_path) {
        if !e.enabled {
            return None;
        }
        if let Some(c) = e.letter_override {
            return Some(c.to_ascii_uppercase());
        }
    }
    app.default_letter()
}
