use anyhow::{Context, Result};
use serde::Deserialize;
use std::{fs, path::PathBuf};

use crate::define::DEFAULT_CONFIG;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub git_path: String,
    #[serde(default)]
    pub colors: ColorConfig,
    #[serde(default)]
    pub layout: LayoutConfig,
    pub commands: Vec<CommandConfig>,
}

#[derive(Deserialize, Debug, Default)]
pub struct ColorConfig {
    pub accent: Option<String>,
    pub error: Option<String>,
    pub background: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct CommandConfig {
    pub name: String,
    pub cmd: String,
    #[serde(default)]
    pub lfs: Option<String>,
    #[serde(default)]
    pub interactive: bool,
}

#[derive(Deserialize, Debug)]
pub struct LayoutConfig {
    pub cmd_width: u16,
    pub files_height: u16,
    pub result_height: u16,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            cmd_width: 32,
            files_height: 7,
            result_height: 5,
        }
    }
}

pub fn load_config() -> Result<Config> {
    let path = ensure_config_file()?;
    let text = fs::read_to_string(&path)
        .with_context(|| format!("cannot read config file: {}", path.display()))?;
    let cfg: Config = toml::from_str(&text).context("invalid config.toml")?;
    Ok(cfg)
}

fn ensure_config_file() -> Result<PathBuf> {
    if let Some(path) = preferred_config_path() {
        if !path.exists() {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir)
                    .with_context(|| format!("failed to create config dir: {}", dir.display()))?;
            }
            fs::write(&path, DEFAULT_CONFIG)
                .with_context(|| format!("failed to write default config: {}", path.display()))?;
        }
        return Ok(path);
    }

    let legacy = PathBuf::from("config.toml");
    if !legacy.exists() {
        fs::write(&legacy, DEFAULT_CONFIG)
            .with_context(|| "failed to write default config.toml in current dir")?;
    }
    Ok(legacy)
}

fn preferred_config_path() -> Option<PathBuf> {
    dirs_next::config_dir().map(|dir| dir.join("simple-git-tui").join("config.toml"))
}
