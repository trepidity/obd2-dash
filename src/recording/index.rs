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
        sorted.sort_by(|a, b| b.start_time.cmp(&a.start_time));
        sorted
    }

    /// Update the file path and compressed flag for a session after compression.
    pub fn mark_compressed(&mut self, session_id: &str, new_path: PathBuf, new_size: u64) {
        if let Some(entry) = self.sessions.iter_mut().find(|s| s.session_id == session_id) {
            entry.file_path = new_path;
            entry.file_size_bytes = new_size;
            entry.compressed = true;
        }
    }
}
