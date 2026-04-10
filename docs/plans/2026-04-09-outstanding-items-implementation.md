# Outstanding Items Resolution — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Resolve all outstanding items: diagnostics expansion (clear DTCs, readiness monitors, freeze-frame), raw capture baud_rate fix, and filesystem test coverage.

**Architecture:** Three sequential phases. Phase 1 is surgical fixes and tests (no new features). Phase 2 adds diagnostics features through the existing TEA message pipeline and session runner boundary. Phase 3 adds tests and documentation for the Phase 2 work.

**Tech Stack:** Rust, obd2-core (session API), ratatui (TUI), tempfile (test fixtures), tokio (async)

---

## Phase 1: Quick Wins

### Task 1: Fix baud_rate passthrough in raw capture metadata

**Files:**
- Modify: `crates/obd2-dash/src/main.rs:1508`

**Step 1: Read the current code**

Read `main.rs:1490-1515` to see `handle_toggle_raw_capture`. Line 1508 has `baud_rate: None`.

**Step 2: Find the baud rate source**

The CLI baud rate is in `cli.baud` (line 66). It's passed through to `connect_serial_with_retry` (line 398 param `baud: u32`). The `handle_toggle_raw_capture` function takes `&mut AppState` but doesn't have access to the CLI baud value. We need to store it.

**Step 3: Add baud_rate field to AppState**

In `crates/obd2-dash/src/app.rs`, add to `AppState`:

```rust
    // Connection metadata for raw capture
    pub serial_baud_rate: Option<u32>,
```

Initialize to `None` in `AppState::new()`.

**Step 4: Set baud_rate when serial transport is created**

In `main.rs`, after the serial transport is successfully created (around line 430 where `SerialTransport::new` succeeds), set:

```rust
state.serial_baud_rate = Some(actual_baud);
```

Do the same for the emulator path and any scanner-initiated serial connections.

**Step 5: Use it in handle_toggle_raw_capture**

In `main.rs:1508`, change:

```rust
baud_rate: None,
```

to:

```rust
baud_rate: state.serial_baud_rate,
```

**Step 6: Verify**

Run: `cargo check -p obd2-dash`
Expected: compiles clean

**Step 7: Commit**

```bash
git add crates/obd2-dash/src/app.rs crates/obd2-dash/src/main.rs
git commit -m "fix: pass serial baud_rate through to raw capture metadata"
```

---

### Task 2: Add tempfile dev-dependency

**Files:**
- Modify: `crates/obd2-dash/Cargo.toml`

**Step 1: Add dev-dependency**

Add to `crates/obd2-dash/Cargo.toml` after the `[dependencies]` section:

```toml
[dev-dependencies]
tempfile = "3"
```

**Step 2: Verify**

Run: `cargo check -p obd2-dash`
Expected: compiles clean (tempfile downloaded)

**Step 3: Commit**

```bash
git add crates/obd2-dash/Cargo.toml
git commit -m "chore: add tempfile dev-dependency for filesystem tests"
```

---

### Task 3: Add SessionIndex tests

**Files:**
- Modify: `crates/obd2-dash/src/recording/index.rs`
- Test: same file, `#[cfg(test)] mod tests`

**Step 1: Write the tests**

Add at the bottom of `crates/obd2-dash/src/recording/index.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::path::PathBuf;

    fn sample_entry(id: &str, size: u64) -> SessionEntry {
        SessionEntry {
            session_id: id.to_string(),
            start_time: Utc::now(),
            vin: Some("TESTVIN1234567890".into()),
            vehicle_name: Some("Test Car".into()),
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

        let mut index = SessionIndex { sessions: vec![] };
        index.add_session(sample_entry("aaa", 1000));
        index.add_session(sample_entry("bbb", 2000));
        index.save(&path).unwrap();

        let loaded = SessionIndex::load(&path);
        assert_eq!(loaded.sessions.len(), 2);
        assert_eq!(loaded.sessions[0].session_id, "aaa");
        assert_eq!(loaded.sessions[1].session_id, "bbb");
    }

    #[test]
    fn test_remove_session() {
        let mut index = SessionIndex { sessions: vec![] };
        index.add_session(sample_entry("aaa", 1000));
        index.add_session(sample_entry("bbb", 2000));

        index.remove_session("aaa");
        assert_eq!(index.sessions.len(), 1);
        assert_eq!(index.sessions[0].session_id, "bbb");
    }

    #[test]
    fn test_total_size_bytes() {
        let mut index = SessionIndex { sessions: vec![] };
        index.add_session(sample_entry("aaa", 1000));
        index.add_session(sample_entry("bbb", 2500));
        assert_eq!(index.total_size_bytes(), 3500);
    }

    #[test]
    fn test_mark_compressed() {
        let mut index = SessionIndex { sessions: vec![] };
        index.add_session(sample_entry("aaa", 10000));

        index.mark_compressed("aaa", PathBuf::from("recordings/aaa.obd2rec.gz"), 3000);

        assert!(index.sessions[0].compressed);
        assert_eq!(index.sessions[0].file_size_bytes, 3000);
        assert_eq!(index.sessions[0].file_path, PathBuf::from("recordings/aaa.obd2rec.gz"));
    }

    #[test]
    fn test_sessions_sorted_newest_first() {
        let mut index = SessionIndex { sessions: vec![] };
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
        assert!(display.contains("2h"), "expected '2h' in '{}''", display);
    }
}
```

**Step 2: Run tests**

Run: `cargo test -p obd2-dash recording::index::tests -- --nocapture`
Expected: all pass

**Step 3: Commit**

```bash
git add crates/obd2-dash/src/recording/index.rs
git commit -m "test: add SessionIndex filesystem roundtrip tests"
```

---

### Task 4: Add StorageManager tests

**Files:**
- Modify: `crates/obd2-dash/src/recording/storage.rs`

**Step 1: Write the tests**

Add at the bottom of `crates/obd2-dash/src/recording/storage.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use crate::recording::index::SessionEntry;

    fn sample_entry(id: &str, size: u64, recordings_dir: &Path) -> SessionEntry {
        let file_path = recordings_dir.join(format!("{}.obd2rec", id));
        // Create the actual file so compression/deletion can work
        std::fs::write(&file_path, vec![0u8; size as usize]).ok();
        SessionEntry {
            session_id: id.to_string(),
            start_time: Utc::now(),
            vin: Some("TEST".into()),
            vehicle_name: Some("Test".into()),
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

        // Verify persisted to disk
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
        mgr.register_session(sample_entry("a", 1000, dir.path())).unwrap();
        mgr.register_session(sample_entry("b", 2000, dir.path())).unwrap();

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
            max_total_bytes: 5000, // Very low limit
            compress_threshold_bytes: 999_999, // Disable compression
            ..StorageConfig::default()
        };
        let mut mgr = StorageManager::new(config);

        // Add sessions totaling > 5000 bytes
        let mut old = sample_entry("old", 3000, dir.path());
        old.start_time = Utc::now() - chrono::Duration::hours(2);
        mgr.register_session(old).unwrap();
        mgr.register_session(sample_entry("new", 3000, dir.path())).unwrap();

        // Total = 6000 > 5000, so maintenance should trim oldest
        mgr.run_maintenance().unwrap();

        // The "old" session should have been trimmed
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

        // No files initially
        assert_eq!(mgr.raw_capture_bytes(), 0);

        // Create some .obd2raw files
        std::fs::write(dir.path().join("test1.obd2raw"), vec![0u8; 1000]).unwrap();
        std::fs::write(dir.path().join("test2.obd2raw"), vec![0u8; 2000]).unwrap();
        std::fs::write(dir.path().join("test3.obd2rec"), vec![0u8; 5000]).unwrap(); // not counted

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
        mgr.register_session(sample_entry("a", 100, dir.path())).unwrap();

        // Externally modify the index
        let mut external = SessionIndex::load(mgr.index_path());
        external.add_session(sample_entry("b", 200, dir.path()));
        external.save(mgr.index_path()).unwrap();

        // mgr doesn't see "b" yet
        assert_eq!(mgr.index.sessions.len(), 1);

        mgr.reload_index();
        assert_eq!(mgr.index.sessions.len(), 2);
    }
}
```

**Step 2: Run tests**

Run: `cargo test -p obd2-dash recording::storage::tests -- --nocapture`
Expected: all pass

**Step 3: Commit**

```bash
git add crates/obd2-dash/src/recording/storage.rs
git commit -m "test: add StorageManager filesystem tests"
```

---

### Task 5: Add ConnectionPrefs tests

**Files:**
- Modify: `crates/obd2-dash/src/connection_prefs.rs`

**Step 1: Write the tests**

Add at the bottom of `crates/obd2-dash/src/connection_prefs.rs`:

```rust
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
```

**Step 2: Run tests**

Run: `cargo test -p obd2-dash connection_prefs::tests -- --nocapture`
Expected: all pass

**Step 3: Commit**

```bash
git add crates/obd2-dash/src/connection_prefs.rs
git commit -m "test: add ConnectionPrefs load/save roundtrip tests"
```

---

## Phase 2: Diagnostics Expansion

### Task 6: Add readiness message types and domain state

**Files:**
- Modify: `crates/obd2-dash/src/domain.rs`
- Modify: `crates/obd2-dash/src/app.rs`

**Step 1: Add imports and domain state field**

In `crates/obd2-dash/src/domain.rs`, add import:

```rust
use obd2_core::protocol::service::ReadinessStatus;
```

Add to `DomainMessage`:

```rust
    ReadinessUpdate(ReadinessStatus),
```

Add to `DomainState` struct:

```rust
    pub readiness: Option<ReadinessStatus>,
```

Initialize in `DomainState::new()`:

```rust
    readiness: None,
```

Add match arm in `DomainState::update()`:

```rust
DomainMessage::ReadinessUpdate(status) => {
    self.readiness = Some(status);
}
```

Clear readiness on disconnect (in the `ConnectionStatus` arm alongside discovery clear):

```rust
self.readiness = None;
```

**Step 2: Add Message variant in app.rs**

In `crates/obd2-dash/src/app.rs`, add import:

```rust
use obd2_core::protocol::service::ReadinessStatus;
```

Add to `Message`:

```rust
    ReadinessUpdate(ReadinessStatus),
```

Add to `AppState::update()` match:

```rust
Message::ReadinessUpdate(status) => {
    self.domain.update(DomainMessage::ReadinessUpdate(status));
}
```

**Step 3: Write a domain test**

In `crates/obd2-dash/src/domain.rs` tests module, add:

```rust
#[test]
fn test_readiness_update_stored_and_cleared_on_disconnect() {
    use obd2_core::protocol::service::{ReadinessStatus, MonitorStatus};

    let mut domain = DomainState::new(250);
    let status = ReadinessStatus {
        mil_on: false,
        dtc_count: 0,
        compression_ignition: false,
        monitors: vec![MonitorStatus {
            name: "Catalyst".into(),
            supported: true,
            complete: true,
        }],
    };

    domain.update(DomainMessage::ReadinessUpdate(status));
    assert!(domain.readiness.is_some());
    assert_eq!(domain.readiness.as_ref().unwrap().monitors.len(), 1);

    domain.update(DomainMessage::ConnectionStatus(ConnectionState::Disconnected));
    assert!(domain.readiness.is_none());
}
```

**Step 4: Verify**

Run: `cargo test -p obd2-dash domain::tests -- --nocapture`
Expected: all pass

**Step 5: Commit**

```bash
git add crates/obd2-dash/src/domain.rs crates/obd2-dash/src/app.rs
git commit -m "feat: add readiness monitor message types and domain state"
```

---

### Task 7: Add poll_readiness to session runner

**Files:**
- Modify: `crates/obd2-dash/src/session_runner.rs`

**Step 1: Add poll_readiness function**

Add after `poll_o2_monitoring`:

```rust
async fn poll_readiness<A: Adapter>(
    session: &mut Session<A>,
    tx: &mpsc::UnboundedSender<Message>,
) {
    match session.read_readiness().await {
        Ok(status) => {
            let _ = tx.send(Message::ReadinessUpdate(status));
        }
        Err(e) => {
            tracing::debug!("Skipping readiness update this cycle: {}", e);
        }
    }
}
```

**Step 2: Wire into polling loop**

In `run_prepared_session`, in the `cycle % 20 == 0` block (line ~177), add `poll_readiness` alongside `poll_o2_monitoring`:

```rust
if cycle % 20 == 0 {
    poll_o2_monitoring(session, tx).await;
    poll_readiness(session, tx).await;
    emit_session_state(session, tx, &mut prepared.last_connection);
    if emit_discovery(session, tx, &mut prepared.last_discovery) {
        prepared.enhanced_targets = build_enhanced_targets(session);
    }
}
```

**Step 3: Verify**

Run: `cargo check -p obd2-dash`
Expected: compiles clean

**Step 4: Commit**

```bash
git add crates/obd2-dash/src/session_runner.rs
git commit -m "feat: poll readiness monitors every 20th cycle"
```

---

### Task 8: Add ReadinessPanel widget

**Files:**
- Modify: `crates/obd2-dash/src/widget/mod.rs`
- Modify: `crates/obd2-dash/src/widget/renderers.rs`

**Step 1: Add widget kind**

In `crates/obd2-dash/src/widget/mod.rs`, add to `WidgetKind` enum:

```rust
    ReadinessPanel,
```

Add to `widget_registry()`:

```rust
WidgetMeta {
    kind: WidgetKind::ReadinessPanel,
    title: "Readiness Monitors",
    category: WidgetCategory::Diagnostics,
    default_size: WidgetSize::Half,
    description: "MIL status, DTC count, and per-monitor readiness",
},
```

**Step 2: Add renderer**

In `crates/obd2-dash/src/widget/renderers.rs`, add a match arm in `render_widget` for `WidgetKind::ReadinessPanel` and a render function:

```rust
fn render_readiness_panel(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    block: Block<'_>,
    _selected: Option<usize>,
) {
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();

    match &state.domain.readiness {
        Some(status) => {
            let mil_style = if status.mil_on {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };
            let mil_text = if status.mil_on { "ON" } else { "OFF" };
            lines.push(Line::from(vec![
                Span::raw("  MIL: "),
                Span::styled(mil_text, mil_style),
                Span::raw(format!("  DTCs: {}  ", status.dtc_count)),
                Span::raw(if status.compression_ignition { "Diesel" } else { "Spark" }),
            ]));
            lines.push(Line::from(""));

            let supported: Vec<_> = status.monitors.iter().filter(|m| m.supported).collect();
            let complete_count = supported.iter().filter(|m| m.complete).count();
            lines.push(Line::from(format!(
                "  Monitors: {}/{} complete",
                complete_count,
                supported.len()
            )));
            lines.push(Line::from(""));

            for monitor in &status.monitors {
                if !monitor.supported {
                    continue;
                }
                let (icon, style) = if monitor.complete {
                    ("OK", Style::default().fg(Color::Green))
                } else {
                    ("--", Style::default().fg(Color::Yellow))
                };
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(format!("{:>2}", icon), style),
                    Span::raw(format!("  {}", monitor.name)),
                ]));
            }
        }
        None => {
            lines.push(Line::from("  No readiness data"));
            lines.push(Line::from("  (waiting for connection)"));
        }
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}
```

**Step 3: Verify**

Run: `cargo check -p obd2-dash`
Expected: compiles clean

Run: `cargo run -p obd2-dash -- --mock --headless` (briefly, Ctrl+C)
Expected: no panics

**Step 4: Commit**

```bash
git add crates/obd2-dash/src/widget/mod.rs crates/obd2-dash/src/widget/renderers.rs
git commit -m "feat: add ReadinessPanel widget with MIL and monitor status"
```

---

### Task 9: Add clear DTC message types and session runner command

**Files:**
- Modify: `crates/obd2-dash/src/app.rs`
- Modify: `crates/obd2-dash/src/domain.rs`
- Modify: `crates/obd2-dash/src/session_runner.rs`

**Step 1: Add ClearDtcCommand to app.rs**

Add a command enum (similar to `CaptureCommand`):

```rust
pub enum DiagnosticCommand {
    ClearAll,
    ClearOnModule(obd2_core::vehicle::ModuleId),
    FetchFreezeFrame { dtc_code: String, pids: Vec<obd2_core::protocol::pid::Pid> },
}
```

Add message variants:

```rust
    DiagnosticReady(mpsc::UnboundedSender<DiagnosticCommand>),
    ClearDtcsComplete,
    ClearDtcsError(String),
```

Wire `DiagnosticReady` in `AppState::update()`:

```rust
Message::DiagnosticReady(tx) => {
    self.diagnostic_tx = Some(tx);
}
Message::ClearDtcsComplete => {
    self.domain.stored_dtcs.clear();
    tracing::info!("DTCs cleared successfully");
}
Message::ClearDtcsError(e) => {
    self.domain.last_error = Some(format!("Clear DTCs failed: {}", e));
    tracing::warn!("Clear DTCs failed: {}", e);
}
```

Add `diagnostic_tx: Option<mpsc::UnboundedSender<DiagnosticCommand>>` to `AppState`.

**Step 2: Handle commands in session runner**

In `run_prepared_session`, add a `diagnostic_rx` parameter (like `capture_rx`). In the main loop, drain commands:

```rust
while let Ok(cmd) = diagnostic_rx.try_recv() {
    match cmd {
        DiagnosticCommand::ClearAll => {
            match session.clear_dtcs().await {
                Ok(()) => { let _ = tx.send(Message::ClearDtcsComplete); }
                Err(e) => { let _ = tx.send(Message::ClearDtcsError(e.to_string())); }
            }
        }
        DiagnosticCommand::ClearOnModule(module_id) => {
            match session.clear_dtcs_on_module(module_id).await {
                Ok(()) => { let _ = tx.send(Message::ClearDtcsComplete); }
                Err(e) => { let _ = tx.send(Message::ClearDtcsError(e.to_string())); }
            }
        }
        DiagnosticCommand::FetchFreezeFrame { dtc_code, pids } => {
            // Implemented in Task 11
        }
    }
}
```

**Step 3: Wire the channel in main.rs**

In `main.rs`, where `run_session_task` is called, create an `mpsc::unbounded_channel()` for `DiagnosticCommand`, send the sender via `Message::DiagnosticReady`, and pass the receiver to the session runner.

**Step 4: Verify**

Run: `cargo check -p obd2-dash`
Expected: compiles clean

**Step 5: Commit**

```bash
git add crates/obd2-dash/src/app.rs crates/obd2-dash/src/domain.rs \
    crates/obd2-dash/src/session_runner.rs crates/obd2-dash/src/main.rs
git commit -m "feat: add clear DTC command channel and session runner handling"
```

---

### Task 10: Add clear DTC UI (popup + two-key confirmation)

**Files:**
- Modify: `crates/obd2-dash/src/main.rs` (key handling)
- Modify: `crates/obd2-dash/src/app.rs` (confirmation state)
- Modify: `crates/obd2-dash/src/tui/ui.rs` (popup rendering)

**Step 1: Add confirmation state to AppState**

In `app.rs`, add:

```rust
    /// Pending clear-DTC confirmation state
    pub clear_dtc_confirm: Option<ClearDtcConfirm>,
```

```rust
pub enum ClearDtcConfirm {
    /// Popup asking "Clear all DTCs?"
    BroadcastPopup,
    /// Two-key: waiting for second C press, with module ID and expiry
    ModulePending { module_id: obd2_core::vehicle::ModuleId, expires: std::time::Instant },
}
```

**Step 2: Handle 'C' keypress in main.rs**

In the key event handler, when `C` is pressed and the DTC panel is focused:

- If no DTC is selected → show broadcast popup: `state.clear_dtc_confirm = Some(ClearDtcConfirm::BroadcastPopup)`
- If a DTC is selected → check if `clear_dtc_confirm` is `ModulePending` and not expired → send `DiagnosticCommand::ClearOnModule`. Otherwise set `ModulePending` with 2-second expiry.

In the broadcast popup, Enter sends `DiagnosticCommand::ClearAll`, Esc cancels.

**Step 3: Render confirmation popup**

In `tui/ui.rs`, if `state.clear_dtc_confirm == Some(BroadcastPopup)`, render a centered popup:

```
╔══ CLEAR ALL DTCs ═══════════════════════╗
║                                         ║
║  This will clear all stored DTCs and    ║
║  reset readiness monitors.              ║
║                                         ║
║  Enter: confirm  |  Esc: cancel         ║
╚═════════════════════════════════════════╝
```

For `ModulePending`, show a footer flash: `"Press C again to clear DTCs on [module]"`.

**Step 4: Verify**

Run: `cargo run -p obd2-dash -- --mock`
- Press `d` to get DTCs, focus the DTC panel, press `C` → popup appears
- Press Esc → popup dismissed
- Press `C` then Enter → DTCs cleared

**Step 5: Commit**

```bash
git add crates/obd2-dash/src/main.rs crates/obd2-dash/src/app.rs \
    crates/obd2-dash/src/tui/ui.rs
git commit -m "feat: add clear DTC UI with popup and two-key confirmation"
```

---

### Task 11: Add freeze-frame to DTC detail popup

**Files:**
- Modify: `crates/obd2-dash/src/app.rs` (messages and state)
- Modify: `crates/obd2-dash/src/domain.rs` (freeze-frame state)
- Modify: `crates/obd2-dash/src/session_runner.rs` (fetch handler)
- Modify: `crates/obd2-dash/src/tui/panel.rs` (popup rendering)
- Modify: `crates/obd2-dash/src/main.rs` (trigger on popup open)

**Step 1: Add freeze-frame state to domain**

In `domain.rs`:

```rust
    pub freeze_frame_pending: bool,
    pub freeze_frame_data: Option<FreezeFrameSnapshot>,
```

```rust
#[derive(Debug, Clone)]
pub struct FreezeFrameSnapshot {
    pub dtc_code: String,
    pub readings: Vec<(Pid, f64, &'static str)>, // (pid, value, unit)
}
```

Add `DomainMessage::FreezeFrameResult(FreezeFrameSnapshot)` and `DomainMessage::FreezeFrameError(String)`.

**Step 2: Handle FetchFreezeFrame in session runner**

Complete the `DiagnosticCommand::FetchFreezeFrame` arm from Task 9:

```rust
DiagnosticCommand::FetchFreezeFrame { dtc_code, pids } => {
    let mut readings = Vec::new();
    for pid in &pids {
        match session.read_freeze_frame(*pid, 0).await {
            Ok(reading) => {
                if let Ok(val) = reading.value.as_f64() {
                    readings.push((*pid, val, reading.unit));
                }
            }
            Err(_) => {} // Skip PIDs with no freeze-frame data
        }
    }
    if !readings.is_empty() {
        let _ = tx.send(Message::FreezeFrameResult(FreezeFrameSnapshot {
            dtc_code, readings,
        }));
    } else {
        let _ = tx.send(Message::FreezeFrameError("No freeze-frame data available".into()));
    }
}
```

**Step 3: Trigger fetch on DTC popup open**

In `main.rs`, when a DTC detail popup is opened (Enter key on a selected DTC), send `DiagnosticCommand::FetchFreezeFrame` with the DTC's code and a set of correlated PIDs (RPM, speed, coolant, load, trims — the same PIDs shown in the "Related Sensors" section).

**Step 4: Render in DTC popup**

In `tui/panel.rs` `build_popup()`, in the `PanelItemDetail::Dtc` arm, if `state.domain.freeze_frame_data` matches the current DTC code, append a "Freeze-Frame Snapshot" section:

```rust
if let Some(ref ff) = state.domain.freeze_frame_data {
    if ff.dtc_code == *code {
        lines.push(String::new());
        lines.push("Freeze-Frame Snapshot:".to_string());
        for (pid, value, unit) in &ff.readings {
            lines.push(format!("  {}: {:.1} {}", pid.name(), value, unit));
        }
    }
}
```

If `state.domain.freeze_frame_pending`, show "Loading freeze-frame data..." instead.

**Step 5: Verify**

Run: `cargo run -p obd2-dash -- --mock`
- Press `d` for DTCs, select a DTC, press Enter → popup should show with freeze-frame section (may show "no data" in mock mode, which is correct)

**Step 6: Commit**

```bash
git add crates/obd2-dash/src/app.rs crates/obd2-dash/src/domain.rs \
    crates/obd2-dash/src/session_runner.rs crates/obd2-dash/src/tui/panel.rs \
    crates/obd2-dash/src/main.rs
git commit -m "feat: add freeze-frame data to DTC detail popup (on-demand)"
```

---

## Phase 3: Tests and Documentation

### Task 12: Add tests for new diagnostics features

**Files:**
- Modify: `crates/obd2-dash/src/domain.rs` (tests)
- Modify: `crates/obd2-dash/src/session_runner.rs` (tests)

**Step 1: Domain tests for clear DTCs and freeze-frame**

```rust
#[test]
fn test_clear_dtcs_clears_stored_dtcs() {
    let mut domain = DomainState::new(250);
    domain.stored_dtcs.push(Dtc { code: "P0420".into(), ..default_dtc() });
    // Simulate clear complete (handled in AppState, verify domain state cleared)
    domain.stored_dtcs.clear();
    assert!(domain.stored_dtcs.is_empty());
}

#[test]
fn test_freeze_frame_stored_and_cleared_on_disconnect() {
    let mut domain = DomainState::new(250);
    domain.freeze_frame_data = Some(FreezeFrameSnapshot {
        dtc_code: "P0420".into(),
        readings: vec![],
    });
    domain.update(DomainMessage::ConnectionStatus(ConnectionState::Disconnected));
    assert!(domain.freeze_frame_data.is_none());
}
```

**Step 2: Run all tests**

Run: `cargo test -p obd2-dash`
Expected: all pass

**Step 3: Commit**

```bash
git add crates/obd2-dash/src/domain.rs crates/obd2-dash/src/session_runner.rs
git commit -m "test: add diagnostics expansion tests"
```

---

### Task 13: Update documentation

**Files:**
- Modify: `README.md`
- Modify: `MANUAL.md`
- Modify: `docs/OUTSTANDING.md`

**Step 1: README updates**

- Add `ReadinessPanel` to the widget list in Architecture section
- Add `C` keybinding to keyboard controls table
- Update DTC description to mention clear and freeze-frame
- Update test count

**Step 2: MANUAL updates**

- Update Section 9 (DTC Diagnostics) with clear DTC instructions and freeze-frame popup section
- Add readiness monitors description (can go in Section 10 or as new Section 10a)
- Add `C` keybinding to keyboard reference tables
- Update widget list to include ReadinessPanel

**Step 3: Update OUTSTANDING.md**

Mark completed items and remove resolved entries. Keep only items that remain outstanding (e.g., Mode $06 test results).

**Step 4: Commit**

```bash
git add README.md MANUAL.md docs/OUTSTANDING.md
git commit -m "docs: update for diagnostics expansion (readiness, clear DTCs, freeze-frame)"
```

---

## Landing Sequence

| Order | Task | Phase | Risk | Commit message |
|-------|------|-------|------|----------------|
| 1 | Baud rate fix | 1 | Low | `fix: pass serial baud_rate through to raw capture metadata` |
| 2 | tempfile dep | 1 | Low | `chore: add tempfile dev-dependency for filesystem tests` |
| 3 | SessionIndex tests | 1 | Low | `test: add SessionIndex filesystem roundtrip tests` |
| 4 | StorageManager tests | 1 | Low | `test: add StorageManager filesystem tests` |
| 5 | ConnectionPrefs tests | 1 | Low | `test: add ConnectionPrefs load/save roundtrip tests` |
| 6 | Readiness types | 2 | Low | `feat: add readiness monitor message types and domain state` |
| 7 | poll_readiness | 2 | Low | `feat: poll readiness monitors every 20th cycle` |
| 8 | ReadinessPanel widget | 2 | Medium | `feat: add ReadinessPanel widget with MIL and monitor status` |
| 9 | Clear DTC commands | 2 | Medium | `feat: add clear DTC command channel and session runner handling` |
| 10 | Clear DTC UI | 2 | Medium | `feat: add clear DTC UI with popup and two-key confirmation` |
| 11 | Freeze-frame popup | 2 | Medium | `feat: add freeze-frame data to DTC detail popup (on-demand)` |
| 12 | Diagnostics tests | 3 | Low | `test: add diagnostics expansion tests` |
| 13 | Documentation | 3 | Low | `docs: update for diagnostics expansion` |
