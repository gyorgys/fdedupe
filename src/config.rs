use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub db: Option<PathBuf>,
    pub recursive: bool,
    pub rescan: bool,
    pub follow_symlinks: bool,
    pub hidden: bool,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

impl Config {
    /// Load config from fdedupe_options.yaml, checking CWD first then exe dir.
    pub fn load() -> Result<Self> {
        let candidates = config_candidates();
        for path in &candidates {
            if path.exists() {
                let text = std::fs::read_to_string(path)?;
                let config: Config = serde_yaml::from_str(&text)?;
                return Ok(config);
            }
        }
        Ok(Config::default())
    }
}

fn config_candidates() -> Vec<PathBuf> {
    let filename = "fdedupe_options.yaml";
    let mut candidates = vec![PathBuf::from(filename)];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(filename));
        }
    }
    candidates
}
