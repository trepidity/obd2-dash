# Raw Protocol Capture — obd2-dash Integration Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Integrate `LoggingTransport` from obd2-core into the obd2-dash application, providing an independent keybinding to toggle raw protocol capture and managing `.obd2raw` files via the existing StorageManager.

**Architecture:** The transport is wrapped in `LoggingTransport` at creation time. Since the transport lives inside a spawned tokio task while keybindings are handled on the main thread, we use a `Message`-based approach: the main thread sends a `StartRawCapture`/`StopRawCapture` message, and the session poll loop acts on it (since it owns the transport). A `CaptureHandle` (Arc-wrapped shared state) lets the main thread query whether capture is active for UI display.

**Tech Stack:** Rust, tokio, obd2-core LoggingTransport, crossterm keybindings

**Design doc:** `docs/plans/2026-03-24-raw-protocol-capture-design.md`
**Depends on:** obd2-core raw protocol capture plan (must be completed first)

---

### Task 1: Update obd2-core Dependency

**Files:**
- Modify: `crates/obd2-dash/Cargo.toml`

**Step 1: Update the obd2-core dependency version or path**

If using a path dependency, no version change needed — just ensure it points to the updated obd2-core that includes `LoggingTransport`. If using a git or crates.io dependency, bump the version.

**Step 2: Verify it compiles**

Run: `cargo check -p obd2-dash`
Expected: Compiles with access to `obd2_core::transport::LoggingTransport`.

**Step 3: Commit**

```
chore: update obd2-core dependency for LoggingTransport
```

---

### Task 2: Add CaptureHandle Shared State

**Files:**
- Modify: `crates/obd2-dash/src/app.rs`

The key challenge: the transport lives in a spawned async task, but the keybinding handler and UI renderer run on the main thread. We need shared state to coordinate.

**Step 1: Define CaptureHandle**

Add to `app.rs` (after imports):

```rust
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::path::PathBuf;

/// Shared handle for raw protocol capture state.
/// Arc-wrapped so the session task and main thread can coordinate.
#[derive(Clone)]
pub struct CaptureHandle {
    active: Arc<AtomicBool>,
    recordings_dir: Arc<PathBuf>,
}

impl CaptureHandle {
    pub fn new(recordings_dir: PathBuf) -> Self {
        Self {
            active: Arc::new(AtomicBool::new(false)),
            recordings_dir: Arc::new(recordings_dir),
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    pub fn recordings_dir(&self) -> &Path {
        &self.recordings_dir
    }
}
```

**Step 2: Add CaptureHandle to AppState**

Add field to `AppState` (around line 112, before the closing `}`):

```rust
    pub capture_handle: Option<CaptureHandle>,
```

Initialize to `None` in `AppState::new()`.

**Step 3: Add capture messages to Message enum**

Add to the `Message` enum (around line 47):

```rust
    // Raw protocol capture
    StartRawCapture,
    StopRawCapture,
    RawCaptureStarted,
    RawCaptureStopped(PathBuf),
    RawCaptureError(String),
```

**Step 4: Run tests**

Run: `cargo check -p obd2-dash`
Expected: Compiles. Warnings about unused variants are OK for now.

**Step 5: Commit**

```
feat: add CaptureHandle and raw capture messages

Shared state and message types for coordinating raw protocol
capture between the session task and main UI thread.
```

---

### Task 3: Wrap Transports in LoggingTransport

**Files:**
- Modify: `crates/obd2-dash/src/main.rs`

**Step 1: Wrap the emulator transport (lines 222-234)**

Change lines 222-234 from:

```rust
let transport = match obd2_core::transport::serial::SerialTransport::new(&port_path, 38400) {
    Ok(t) => t,
    Err(e) => { /* error handling */ }
};
let adapter = Elm327Adapter::new(Box::new(transport));
```

To:

```rust
let transport = match obd2_core::transport::serial::SerialTransport::new(&port_path, 38400) {
    Ok(t) => t,
    Err(e) => { /* error handling unchanged */ }
};
let logging = obd2_core::transport::LoggingTransport::new(transport);
let adapter = Elm327Adapter::new(Box::new(logging));
```

**Step 2: Wrap the serial transport (lines 551-563)**

Change from:

```rust
let transport = match obd2_core::transport::serial::SerialTransport::new(port_path, actual_baud) {
    Ok(t) => t,
    Err(e) => { /* retry handling */ }
};
let adapter = Elm327Adapter::new(Box::new(transport));
```

To:

```rust
let transport = match obd2_core::transport::serial::SerialTransport::new(port_path, actual_baud) {
    Ok(t) => t,
    Err(e) => { /* retry handling unchanged */ }
};
let logging = obd2_core::transport::LoggingTransport::new(transport);
let adapter = Elm327Adapter::new(Box::new(logging));
```

**Step 3: Wrap the BLE transport (lines 606-608)**

Change from:

```rust
match obd2_core::transport::ble::BleTransport::scan_and_connect(filter, scan_dur).await {
    Ok(ble_transport) => {
        let adapter = Elm327Adapter::new(Box::new(ble_transport));
```

To:

```rust
match obd2_core::transport::ble::BleTransport::scan_and_connect(filter, scan_dur).await {
    Ok(ble_transport) => {
        let logging = obd2_core::transport::LoggingTransport::new(ble_transport);
        let adapter = Elm327Adapter::new(Box::new(logging));
```

**Step 4: Verify compilation**

Run: `cargo check -p obd2-dash`
Expected: Compiles. `LoggingTransport<T>` implements `Transport`, so `Box::new(logging)` works as before.

**Step 5: Commit**

```
feat: wrap all transports in LoggingTransport

All serial and BLE transports are now wrapped with LoggingTransport.
Capture is inactive by default — zero overhead passthrough.
```

---

### Task 4: Handle Capture Messages in Session Poll Loop

**Files:**
- Modify: `crates/obd2-dash/src/main.rs`

This is the core integration. The session poll loop (which owns the adapter and transport) needs to handle capture start/stop messages. The challenge is that `ELM327Adapter` takes `Box<dyn Transport>` — we can't downcast to `LoggingTransport`. Instead, we need to expose capture control through the adapter.

**Step 1: Assess the adapter access pattern**

The `Elm327Adapter` stores `transport: Box<dyn Transport>`. We need a way to call `start_capture()`/`stop_capture()` on the inner `LoggingTransport` through the dynamic dispatch.

**Step 2: Add capture methods to the Transport trait (in obd2-core)**

NOTE: This requires a small addition to obd2-core. Add to the `Transport` trait in `transport/mod.rs`:

```rust
    /// Start raw protocol capture to a file. Default: no-op (returns false).
    fn start_raw_capture(&mut self, _path: &Path, _metadata: &logging::CaptureMetadata) -> bool {
        false
    }

    /// Stop raw protocol capture. Default: no-op (returns None).
    fn stop_raw_capture(&mut self) -> Option<PathBuf> {
        None
    }

    /// Whether raw capture is currently active. Default: false.
    fn is_capturing(&self) -> bool {
        false
    }
```

Override these in `LoggingTransport` to delegate to `start_capture()`/`stop_capture()`.

**Alternative approach (if we don't want to modify the Transport trait):** Add a `capture_control()` method to `Elm327Adapter` that returns `&mut dyn Transport`. Then the session poll loop can attempt to downcast. However, the trait-method approach is cleaner and more extensible.

**Step 3: Add a receive channel to the session poll loop**

Find `run_session_poll_loop()` in `main.rs`. Add an `mpsc::UnboundedReceiver<CaptureCommand>` parameter. In the select loop, add a branch that handles `StartRawCapture` and `StopRawCapture`.

```rust
enum CaptureCommand {
    Start { path: PathBuf, metadata: CaptureMetadata },
    Stop,
}
```

When `Start` is received:
1. Call `session.adapter_mut().transport_mut().start_raw_capture(path, metadata)` (or however the transport is exposed)
2. Send `RawCaptureStarted` back via the main tx channel
3. Update the CaptureHandle's active flag

When `Stop` is received:
1. Call `stop_raw_capture()`
2. Send `RawCaptureStopped(path)` back
3. Update the CaptureHandle's active flag

**Step 4: Wire the capture channel at session creation**

At each transport creation site (emulator, serial, BLE), create a `mpsc::unbounded_channel()` for capture commands. Store the sender in AppState (or pass it through CaptureHandle). Pass the receiver into the session poll loop.

**Step 5: Verify compilation**

Run: `cargo check -p obd2-dash`
Expected: Compiles with new message handling.

**Step 6: Commit**

```
feat: handle raw capture start/stop in session poll loop

The session poll loop receives CaptureCommand messages and
controls LoggingTransport capture from within the async task.
```

---

### Task 5: Add Keybinding for Raw Capture Toggle

**Files:**
- Modify: `crates/obd2-dash/src/main.rs`

**Step 1: Add 'c' keybinding in handle_key()**

Add after the 'r' keybinding block (around line 1172):

```rust
KeyCode::Char('c') => {
    handle_toggle_raw_capture(state);
}
```

**Step 2: Implement handle_toggle_raw_capture()**

Add a new function near `handle_toggle_recording()`:

```rust
fn handle_toggle_raw_capture(state: &mut AppState) {
    if let Some(ref handle) = state.capture_handle {
        if handle.is_active() {
            // Send stop command
            if let Some(ref tx) = state.capture_tx {
                let _ = tx.send(CaptureCommand::Stop);
            }
        } else {
            // Generate filename and send start command
            let session_id = uuid::Uuid::new_v4().to_string();
            let path = handle.recordings_dir().join(format!("{}.obd2raw", session_id));
            let metadata = obd2_core::transport::CaptureMetadata {
                transport_type: state.domain.adapter_info
                    .as_ref()
                    .map(|i| format!("{:?}", i.chipset))
                    .unwrap_or_else(|| "unknown".to_string()),
                port_or_device: state.domain.adapter_info
                    .as_ref()
                    .map(|i| i.firmware.clone())
                    .unwrap_or_else(|| "unknown".to_string()),
                baud_rate: None, // TODO: store baud at connection time
            };
            if let Some(ref tx) = state.capture_tx {
                let _ = tx.send(CaptureCommand::Start { path, metadata });
            }
        }
    }
}
```

**Step 3: Add capture_tx to AppState**

Add field:

```rust
pub capture_tx: Option<mpsc::UnboundedSender<CaptureCommand>>,
```

Initialize from the channel created in Task 4.

**Step 4: Verify compilation**

Run: `cargo check -p obd2-dash`
Expected: Compiles.

**Step 5: Commit**

```
feat: add 'c' keybinding for raw protocol capture toggle

Independent from structured recording. Sends capture commands
to the session poll loop via channel.
```

---

### Task 6: Display Capture Status in UI

**Files:**
- Modify: `crates/obd2-dash/src/tui/status_bar.rs` (or wherever the status bar is rendered)

**Step 1: Find the status bar rendering**

Look for where recording status is shown (likely shows "REC" when recording). Add a similar indicator for raw capture.

**Step 2: Add capture indicator**

When `state.capture_handle.as_ref().map_or(false, |h| h.is_active())`, display a "RAW" indicator next to the existing "REC" indicator.

**Step 3: Test visually**

Run: `cargo run -p obd2-dash -- --mock`
Expected: No "RAW" indicator when idle. Press 'c' to see it appear.

**Step 4: Commit**

```
feat: show RAW capture indicator in status bar

Displays "RAW" when raw protocol capture is active,
independent of the structured recording "REC" indicator.
```

---

### Task 7: Register .obd2raw Files with StorageManager

**Files:**
- Modify: `crates/obd2-dash/src/main.rs` (where `RawCaptureStopped` message is handled)
- Possibly modify: `crates/obd2-dash/src/recording/storage.rs`

**Step 1: Handle RawCaptureStopped message**

In the main message processing loop, when `RawCaptureStopped(path)` is received:

```rust
Message::RawCaptureStopped(path) => {
    let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    tracing::info!("Raw capture saved: {} ({} bytes)", path.display(), file_size);
    // Track file size for storage accounting
    // The .obd2raw file doesn't need a SessionEntry — it's a sidecar artifact.
    // StorageManager can track total disk usage including these files.
}
```

**Step 2: Consider storage cleanup**

For now, `.obd2raw` files are cleaned up when the user manually deletes them or when the recordings directory is pruned by StorageManager. If StorageManager only tracks `.obd2rec` files, we may need to add awareness of `.obd2raw` files to `run_maintenance()`.

The simplest approach: in `StorageManager::run_maintenance()`, when calculating total storage, also glob for `*.obd2raw` files and include their sizes. When trimming old sessions, also delete any `.obd2raw` file with the same session UUID prefix.

**Step 3: Test end-to-end**

Run: `cargo run -p obd2-dash -- --mock`
1. Press 'c' to start capture
2. Wait a few seconds
3. Press 'c' to stop capture
4. Check `recordings/` directory for `.obd2raw` file
5. Verify file content is human-readable raw protocol log

**Step 4: Commit**

```
feat: track .obd2raw files in storage manager

Raw capture files are included in storage accounting and
cleaned up alongside .obd2rec files during maintenance.
```

---

### Task 8: End-to-End Verification

**Files:** None (testing only)

**Step 1: Test with emulator**

```bash
# Terminal 1: start the emulator
cargo run -p obd2-dash -- --emu --port /tmp/elm327_pty

# Press 'c' to start raw capture
# Wait 10 seconds
# Press 'c' to stop
```

**Step 2: Inspect the .obd2raw file**

```bash
cat recordings/*.obd2raw
```

Expected output should look like:

```
# obd2-raw v1
# transport=serial port=/tmp/elm327_pty baud=38400
# started=2026-03-24T...
0.000 W ATZ\r
0.045 R ELM327 v2.1\r\r>
0.100 W ATE0\r
...
```

**Step 3: Test MockTransport generation**

Write a quick integration test or script that calls `parse_raw_capture()` on the captured file and verifies it produces valid `(command, response)` pairs.

**Step 4: Test that structured recording and raw capture work simultaneously**

Press 'r' (start recording) then 'c' (start capture). Both indicators should show. Stop both. Verify both files exist.

**Step 5: Commit**

```
test: verify end-to-end raw protocol capture

Tested with emulator, verified .obd2raw format, confirmed
independent operation alongside structured recording.
```
