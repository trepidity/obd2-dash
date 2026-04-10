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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::DeviceKind;

    #[test]
    fn test_load_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let prefs = ConnectionPrefs::load(&path);
        assert!(prefs.last_device.is_none());
    }

    #[test]
    fn test_save_and_load_roundtrip_serial() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");

        let prefs = ConnectionPrefs {
            last_device: Some(DeviceKind::Serial {
                port_path: "/dev/ttyUSB0".into(),
                baud: 115200,
            }),
        };
        prefs.save(&path).unwrap();

        let loaded = ConnectionPrefs::load(&path);
        match loaded.last_device {
            Some(DeviceKind::Serial { port_path, baud }) => {
                assert_eq!(port_path, "/dev/ttyUSB0");
                assert_eq!(baud, 115200);
            }
            other => panic!("expected Serial, got {:?}", other),
        }
    }

    #[test]
    fn test_save_and_load_roundtrip_ble() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");

        let prefs = ConnectionPrefs {
            last_device: Some(DeviceKind::Ble {
                name: "OBDLink MX+".into(),
            }),
        };
        prefs.save(&path).unwrap();

        let loaded = ConnectionPrefs::load(&path);
        match loaded.last_device {
            Some(DeviceKind::Ble { name }) => {
                assert_eq!(name, "OBDLink MX+");
            }
            other => panic!("expected Ble, got {:?}", other),
        }
    }

    #[test]
    fn test_load_invalid_json_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        std::fs::write(&path, "{ not valid json !!!").unwrap();

        let prefs = ConnectionPrefs::load(&path);
        assert!(prefs.last_device.is_none());
    }
}
