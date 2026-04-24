use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

const PREFERENCES_FILE_NAME: &str = "ui-preferences.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UiPreferences {
    pub advanced_mode: bool,
}

pub fn load_ui_preferences() -> anyhow::Result<UiPreferences> {
    let Some(path) = preferences_path() else {
        return Ok(UiPreferences::default());
    };
    if !path.is_file() {
        return Ok(UiPreferences::default());
    }

    let json = fs::read_to_string(&path)
        .with_context(|| format!("read UI preferences {}", path.display()))?;
    let preferences = serde_json::from_str(&json).context("parse UI preferences json")?;
    Ok(preferences)
}

pub fn save_ui_preferences(preferences: &UiPreferences) -> anyhow::Result<()> {
    let Some(path) = preferences_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create preferences directory {}", parent.display()))?;
    }

    let json = serde_json::to_string_pretty(preferences).context("serialize UI preferences")?;
    fs::write(&path, json).with_context(|| format!("write UI preferences {}", path.display()))?;
    Ok(())
}

fn preferences_path() -> Option<PathBuf> {
    preferences_root().map(|root| root.join("awebpinator").join(PREFERENCES_FILE_NAME))
}

fn preferences_root() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
}
