use std::fs;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use flate2::write::GzEncoder;
use flate2::Compression;

use super::index::{SessionEntry, SessionIndex};

/// Configuration for storage management.
#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub recordings_dir: PathBuf,
    /// Compress raw files larger than this (default: 50 MB).
    pub compress_threshold_bytes: u64,
    /// Maximum total uncompressed data (default: 100 MB).
    pub max_raw_bytes: u64,
    /// Maximum total storage (default: 500 MB) — trim oldest when exceeded.
    pub max_total_bytes: u64,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            recordings_dir: PathBuf::from("recordings"),
            compress_threshold_bytes: 50 * 1024 * 1024,
            max_raw_bytes: 100 * 1024 * 1024,
            max_total_bytes: 500 * 1024 * 1024,
        }
    }
}

/// Manages recording storage: compression and trimming.
pub struct StorageManager {
    pub config: StorageConfig,
    pub index: SessionIndex,
    index_path: PathBuf,
}

impl StorageManager {
    pub fn new(config: StorageConfig) -> Self {
        fs::create_dir_all(&config.recordings_dir).ok();
        let index_path = config.recordings_dir.join("sessions.json");
        let index = SessionIndex::load(&index_path);
        Self {
            config,
            index,
            index_path,
        }
    }

    /// Register a completed recording session.
    pub fn register_session(&mut self, entry: SessionEntry) -> anyhow::Result<()> {
        self.index.add_session(entry);
        self.index.save(&self.index_path)?;
        Ok(())
    }

    /// Compress a raw recording file to gzip.
    pub fn compress_session(&mut self, session_id: &str) -> anyhow::Result<()> {
        let entry = self
            .index
            .sessions
            .iter()
            .find(|s| s.session_id == session_id)
            .cloned();

        let entry = match entry {
            Some(e) if !e.compressed => e,
            _ => return Ok(()), // Already compressed or not found
        };

        let raw_path = &entry.file_path;
        let gz_path = PathBuf::from(format!("{}.gz", raw_path.display()));

        // Compress
        let input = fs::File::open(raw_path)?;
        let mut reader = BufReader::new(input);
        let output = fs::File::create(&gz_path)?;
        let mut encoder = GzEncoder::new(BufWriter::new(output), Compression::default());

        let mut buf = [0u8; 8192];
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            encoder.write_all(&buf[..n])?;
        }
        encoder.finish()?;

        // Get new size
        let new_size = fs::metadata(&gz_path)?.len();

        // Remove raw file
        fs::remove_file(raw_path)?;

        // Update index
        self.index.mark_compressed(session_id, gz_path, new_size);
        self.index.save(&self.index_path)?;

        tracing::info!(
            "Compressed session {}: {} -> {} bytes",
            session_id,
            entry.file_size_bytes,
            new_size
        );

        Ok(())
    }

    /// Total size of .obd2raw sidecar files in the recordings directory.
    pub fn raw_capture_bytes(&self) -> u64 {
        fs::read_dir(&self.config.recordings_dir)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().is_some_and(|ext| ext == "obd2raw"))
                    .filter_map(|e| e.metadata().ok())
                    .map(|m| m.len())
                    .sum()
            })
            .unwrap_or(0)
    }

    /// Run maintenance: compress large raw files, trim oldest if over storage limit.
    pub fn run_maintenance(&mut self) -> anyhow::Result<()> {
        // Compress large raw files
        let to_compress: Vec<String> = self
            .index
            .sessions
            .iter()
            .filter(|s| !s.compressed && s.file_size_bytes > self.config.compress_threshold_bytes)
            .map(|s| s.session_id.clone())
            .collect();

        for sid in to_compress {
            if let Err(e) = self.compress_session(&sid) {
                tracing::warn!("Failed to compress session {}: {}", sid, e);
            }
        }

        // Trim oldest sessions if over total limit (including .obd2raw sidecars)
        while self.index.total_size_bytes() + self.raw_capture_bytes() > self.config.max_total_bytes
        {
            // Find oldest session
            let oldest = self
                .index
                .sessions
                .iter()
                .min_by_key(|s| s.start_time)
                .map(|s| (s.session_id.clone(), s.file_path.clone()));

            match oldest {
                Some((sid, path)) => {
                    // Delete recording file
                    if path.exists() {
                        fs::remove_file(&path).ok();
                    }
                    // Delete matching .obd2raw sidecar if it exists
                    let raw_path = self.config.recordings_dir.join(format!("{}.obd2raw", sid));
                    if raw_path.exists() {
                        fs::remove_file(&raw_path).ok();
                        tracing::info!("Deleted raw capture sidecar: {}", sid);
                    }
                    self.index.remove_session(&sid);
                    tracing::info!("Trimmed oldest session: {}", sid);
                }
                None => break,
            }
        }

        self.index.save(&self.index_path)?;
        Ok(())
    }

    /// Delete a specific session by ID.
    pub fn delete_session(&mut self, session_id: &str) -> anyhow::Result<()> {
        if let Some(entry) = self
            .index
            .sessions
            .iter()
            .find(|s| s.session_id == session_id)
        {
            let path = entry.file_path.clone();
            if path.exists() {
                fs::remove_file(&path)?;
            }
        }
        self.index.remove_session(session_id);
        self.index.save(&self.index_path)?;
        Ok(())
    }

    /// Get human-readable storage stats.
    pub fn storage_stats(&self) -> StorageStats {
        let total_bytes = self.index.total_size_bytes() + self.raw_capture_bytes();
        let session_count = self.index.sessions.len();
        let raw_count = self.index.sessions.iter().filter(|s| !s.compressed).count();
        let compressed_count = session_count - raw_count;

        StorageStats {
            session_count,
            raw_count,
            compressed_count,
            total_bytes,
            max_bytes: self.config.max_total_bytes,
        }
    }

    /// Reload the index from disk (useful after external changes).
    pub fn reload_index(&mut self) {
        self.index = SessionIndex::load(&self.index_path);
    }

    pub fn recordings_dir(&self) -> &Path {
        &self.config.recordings_dir
    }

    pub fn index_path(&self) -> &Path {
        &self.index_path
    }
}

#[derive(Debug, Clone)]
pub struct StorageStats {
    pub session_count: usize,
    pub raw_count: usize,
    pub compressed_count: usize,
    pub total_bytes: u64,
    pub max_bytes: u64,
}

impl StorageStats {
    pub fn total_display(&self) -> String {
        format_bytes(self.total_bytes)
    }

    pub fn max_display(&self) -> String {
        format_bytes(self.max_bytes)
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::index::SessionEntry;
    use chrono::Utc;

    fn sample_entry(id: &str, size: u64, recordings_dir: &Path) -> SessionEntry {
        let file_path = recordings_dir.join(format!("{}.obd2rec", id));
        std::fs::write(&file_path, vec![0u8; size as usize]).ok();
        SessionEntry {
            session_id: id.to_string(),
            start_time: Utc::now(),
            vin: Some("TEST".into()),
            vehicle_name: Some("Test".into()),
            profile_id: None,
            duration_secs: 60,
            frame_count: 100,
            file_path,
            file_size_bytes: size,
            compressed: false,
        }
    }

    #[test]
    fn test_register_session_persists_to_index() {
        let dir = tempfile::tempdir().unwrap();
        let config = StorageConfig {
            recordings_dir: dir.path().to_path_buf(),
            ..StorageConfig::default()
        };
        let mut mgr = StorageManager::new(config);

        let entry = sample_entry("sess1", 5000, dir.path());
        mgr.register_session(entry).unwrap();

        assert_eq!(mgr.index.sessions.len(), 1);
        assert_eq!(mgr.index.sessions[0].session_id, "sess1");

        let reloaded = SessionIndex::load(mgr.index_path());
        assert_eq!(reloaded.sessions.len(), 1);
    }

    #[test]
    fn test_delete_session_removes_file_and_index() {
        let dir = tempfile::tempdir().unwrap();
        let config = StorageConfig {
            recordings_dir: dir.path().to_path_buf(),
            ..StorageConfig::default()
        };
        let mut mgr = StorageManager::new(config);

        let entry = sample_entry("sess1", 5000, dir.path());
        let file_path = entry.file_path.clone();
        mgr.register_session(entry).unwrap();

        assert!(file_path.exists());
        mgr.delete_session("sess1").unwrap();
        assert!(!file_path.exists());
        assert!(mgr.index.sessions.is_empty());
    }

    #[test]
    fn test_storage_stats() {
        let dir = tempfile::tempdir().unwrap();
        let config = StorageConfig {
            recordings_dir: dir.path().to_path_buf(),
            max_total_bytes: 1_000_000,
            ..StorageConfig::default()
        };
        let mut mgr = StorageManager::new(config);
        mgr.register_session(sample_entry("a", 1000, dir.path()))
            .unwrap();
        mgr.register_session(sample_entry("b", 2000, dir.path()))
            .unwrap();

        let stats = mgr.storage_stats();
        assert_eq!(stats.session_count, 2);
        assert_eq!(stats.raw_count, 2);
        assert_eq!(stats.compressed_count, 0);
        assert_eq!(stats.max_bytes, 1_000_000);
    }

    #[test]
    fn test_maintenance_trims_oldest_when_over_quota() {
        let dir = tempfile::tempdir().unwrap();
        let config = StorageConfig {
            recordings_dir: dir.path().to_path_buf(),
            max_total_bytes: 5000,
            compress_threshold_bytes: 999_999,
            ..StorageConfig::default()
        };
        let mut mgr = StorageManager::new(config);

        let mut old = sample_entry("old", 3000, dir.path());
        old.start_time = Utc::now() - chrono::Duration::hours(2);
        mgr.register_session(old).unwrap();
        mgr.register_session(sample_entry("new", 3000, dir.path()))
            .unwrap();

        mgr.run_maintenance().unwrap();

        assert_eq!(mgr.index.sessions.len(), 1);
        assert_eq!(mgr.index.sessions[0].session_id, "new");
    }

    #[test]
    fn test_raw_capture_bytes_counts_obd2raw_files() {
        let dir = tempfile::tempdir().unwrap();
        let config = StorageConfig {
            recordings_dir: dir.path().to_path_buf(),
            ..StorageConfig::default()
        };
        let mgr = StorageManager::new(config);

        assert_eq!(mgr.raw_capture_bytes(), 0);

        std::fs::write(dir.path().join("test1.obd2raw"), vec![0u8; 1000]).unwrap();
        std::fs::write(dir.path().join("test2.obd2raw"), vec![0u8; 2000]).unwrap();
        std::fs::write(dir.path().join("test3.obd2rec"), vec![0u8; 5000]).unwrap();

        assert_eq!(mgr.raw_capture_bytes(), 3000);
    }

    #[test]
    fn test_reload_index() {
        let dir = tempfile::tempdir().unwrap();
        let config = StorageConfig {
            recordings_dir: dir.path().to_path_buf(),
            ..StorageConfig::default()
        };
        let mut mgr = StorageManager::new(config);
        mgr.register_session(sample_entry("a", 100, dir.path()))
            .unwrap();

        let mut external = SessionIndex::load(mgr.index_path());
        external.add_session(sample_entry("b", 200, dir.path()));
        external.save(mgr.index_path()).unwrap();

        assert_eq!(mgr.index.sessions.len(), 1);
        mgr.reload_index();
        assert_eq!(mgr.index.sessions.len(), 2);
    }
}
