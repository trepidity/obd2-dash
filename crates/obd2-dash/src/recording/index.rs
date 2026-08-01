use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single session entry in the index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    pub session_id: String,
    pub start_time: DateTime<Utc>,
    pub vin: Option<String>,
    pub vehicle_name: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
    pub duration_secs: u64,
    pub frame_count: u64,
    pub file_path: PathBuf,
    pub file_size_bytes: u64,
    pub compressed: bool,
}

impl SessionEntry {
    /// Human-readable duration string.
    pub fn duration_display(&self) -> String {
        let secs = self.duration_secs;
        let mins = secs / 60;
        let hours = mins / 60;
        format!("{}h {:02}m", hours, mins % 60)
    }
}

/// JSON index tracking all recorded sessions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionIndex {
    pub sessions: Vec<SessionEntry>,
}

impl SessionIndex {
    /// Load the index from a JSON file, or return empty if missing.
    pub fn load(path: &Path) -> Self {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(contents) => match serde_json::from_str::<SessionIndex>(&contents) {
                    Ok(index) => return index,
                    Err(e) => {
                        tracing::warn!("Invalid session index at {}: {}", path.display(), e);
                    }
                },
                Err(e) => {
                    tracing::warn!("Could not read session index at {}: {}", path.display(), e);
                }
            }
        }
        SessionIndex::default()
    }

    /// Save the index to a JSON file.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Add a new session entry.
    pub fn add_session(&mut self, entry: SessionEntry) {
        self.sessions.push(entry);
    }

    /// Remove a session by ID.
    pub fn remove_session(&mut self, session_id: &str) {
        self.sessions.retain(|s| s.session_id != session_id);
    }

    /// Get total size of all recordings.
    pub fn total_size_bytes(&self) -> u64 {
        self.sessions.iter().map(|s| s.file_size_bytes).sum()
    }

    /// Get sessions sorted by start time (newest first).
    pub fn sessions_sorted(&self) -> Vec<&SessionEntry> {
        let mut sorted: Vec<_> = self.sessions.iter().collect();
        sorted.sort_by_key(|entry| std::cmp::Reverse(entry.start_time));
        sorted
    }

    /// Update the file path and compressed flag for a session after compression.
    pub fn mark_compressed(&mut self, session_id: &str, new_path: PathBuf, new_size: u64) {
        if let Some(entry) = self
            .sessions
            .iter_mut()
            .find(|s| s.session_id == session_id)
        {
            entry.file_path = new_path;
            entry.file_size_bytes = new_size;
            entry.compressed = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry(id: &str, size: u64) -> SessionEntry {
        SessionEntry {
            session_id: id.to_string(),
            start_time: Utc::now(),
            vin: Some("TESTVIN1234567890".into()),
            vehicle_name: Some("Test Car".into()),
            profile_id: None,
            duration_secs: 60,
            frame_count: 100,
            file_path: PathBuf::from(format!("recordings/{}.obd2rec", id)),
            file_size_bytes: size,
            compressed: false,
        }
    }

    #[test]
    fn test_load_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let index = SessionIndex::load(&path);
        assert!(index.sessions.is_empty());
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions.json");

        let mut index = SessionIndex::default();
        index.add_session(sample_entry("aaa", 1000));
        index.add_session(sample_entry("bbb", 2000));
        index.save(&path).unwrap();

        let loaded = SessionIndex::load(&path);
        assert_eq!(loaded.sessions.len(), 2);
        assert_eq!(loaded.sessions[0].session_id, "aaa");
        assert_eq!(loaded.sessions[1].session_id, "bbb");
    }

    #[test]
    fn test_load_legacy_entry_defaults_profile_id() {
        let json = r#"{
          "sessions": [{
            "session_id": "legacy",
            "start_time": "2026-01-01T00:00:00Z",
            "vin": null,
            "vehicle_name": null,
            "duration_secs": 1,
            "frame_count": 0,
            "file_path": "recordings/legacy.obd2rec",
            "file_size_bytes": 1,
            "compressed": false
          }]
        }"#;

        let index: SessionIndex = serde_json::from_str(json).unwrap();

        assert_eq!(index.sessions.len(), 1);
        assert_eq!(index.sessions[0].session_id, "legacy");
        assert!(index.sessions[0].profile_id.is_none());
    }

    #[test]
    fn test_remove_session() {
        let mut index = SessionIndex::default();
        index.add_session(sample_entry("aaa", 1000));
        index.add_session(sample_entry("bbb", 2000));

        index.remove_session("aaa");
        assert_eq!(index.sessions.len(), 1);
        assert_eq!(index.sessions[0].session_id, "bbb");
    }

    #[test]
    fn test_total_size_bytes() {
        let mut index = SessionIndex::default();
        index.add_session(sample_entry("aaa", 1000));
        index.add_session(sample_entry("bbb", 2500));
        assert_eq!(index.total_size_bytes(), 3500);
    }

    #[test]
    fn test_mark_compressed() {
        let mut index = SessionIndex::default();
        index.add_session(sample_entry("aaa", 10000));

        index.mark_compressed("aaa", PathBuf::from("recordings/aaa.obd2rec.gz"), 3000);

        assert!(index.sessions[0].compressed);
        assert_eq!(index.sessions[0].file_size_bytes, 3000);
        assert_eq!(
            index.sessions[0].file_path,
            PathBuf::from("recordings/aaa.obd2rec.gz")
        );
    }

    #[test]
    fn test_sessions_sorted_newest_first() {
        let mut index = SessionIndex::default();
        let mut old = sample_entry("old", 1000);
        old.start_time = Utc::now() - chrono::Duration::hours(2);
        index.add_session(old);
        index.add_session(sample_entry("new", 2000));

        let sorted = index.sessions_sorted();
        assert_eq!(sorted[0].session_id, "new");
        assert_eq!(sorted[1].session_id, "old");
    }

    #[test]
    fn test_duration_display() {
        let mut entry = sample_entry("x", 100);
        entry.duration_secs = 7380; // 2h 3m
        let display = entry.duration_display();
        assert_eq!(display, "2h 03m");
    }
}
