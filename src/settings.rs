use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub game: Option<PathBuf>,
    /// Install as soon as a drop is read, unless there's a choice to make.
    pub auto_install: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { game: None, auto_install: true }
    }
}

fn path() -> PathBuf {
    crate::install::data_dir().join("settings.json")
}

impl Settings {
    pub fn load() -> Settings {
        std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let p = path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, json);
        }
    }
}
