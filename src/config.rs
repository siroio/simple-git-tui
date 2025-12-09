use std::fs;
use anyhow::Context;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub git_path: String,
    pub repo_path: String,
    #[serde(default)]
    pub colors: ColorConfig,
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
    pub lfs: Option<String>, // "none" | "fetch" | "pull"
}

pub fn load_config() -> anyhow::Result<Config> {
    let text = fs::read_to_string("Config.toml").context("cannot read Config.toml")?;
    let cfg: Config = toml::from_str(&text).context("invalid config.toml")?;
    Ok(cfg)
}

