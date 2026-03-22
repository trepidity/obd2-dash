use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::scanner::DeviceKind;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConnectionPrefs {
    pub last_device: Option<DeviceKind>,
}

impl ConnectionPrefs {
    /// Load prefs from a JSON file, returning defaults if missing or invalid.
    pub fn load(path: &Path) -> Self {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(contents) => match serde_json::from_str::<ConnectionPrefs>(&contents) {
                    Ok(prefs) => {
                        tracing::info!("Loaded connection prefs from {}", path.display());
                        return prefs;
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Invalid connection prefs at {}: {}, using default",
                            path.display(),
                            e
                        );
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        "Could not read connection prefs at {}: {}, using default",
                        path.display(),
                        e
                    );
                }
            }
        }
        Self::default()
    }

    /// Save prefs to a JSON file.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        tracing::info!("Saved connection prefs to {}", path.display());
        Ok(())
    }
}
