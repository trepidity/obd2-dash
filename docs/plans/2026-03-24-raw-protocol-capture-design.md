# Raw Protocol Capture Design

## Problem

The current recording system captures parsed data (decoded values + stripped data bytes) at the domain layer. The raw serial/BLE conversation — AT commands, hex framing, `\r` terminators, `>` prompts, echo lines, multi-chunk reads — is lost. This makes it impossible to:

- Troubleshoot adapter integration issues from a recording
- Build realistic emulators from captured sessions
- Generate `MockTransport::expect()` pairs from real hardware

## Decision Summary

| Decision | Choice |
|----------|--------|
| Sidecar vs. embedded | Separate `.obd2raw` file alongside `.obd2rec` |
| Capture fidelity | Both transport chunks and assembled command/response pairs |
| Toggle independence | Raw capture is independently toggleable from structured recording |
| Code location | `LoggingTransport<T>` decorator in `obd2-core` |
| File format | Line-oriented text, human-readable |

## Architecture

A `LoggingTransport<T: Transport>` decorator in `obd2-core` wraps any transport implementation. It forwards all calls transparently and writes a text log when capture is active.

```
ELM327Adapter
    |
    v
LoggingTransport<SerialTransport>
    | \--- writes to .obd2raw file
    v
SerialTransport
    |
    v
  /dev/tty.usb...
```

### File layout (obd2-core)

```
obd2-core/src/transport/
  mod.rs          -- Transport trait (add optional chunk observer)
  serial.rs       -- SerialTransport (call chunk observer in read loop)
  ble.rs          -- BleTransport (call chunk observer in read loop)
  mock.rs         -- MockTransport (unchanged)
  logging.rs      -- NEW: LoggingTransport<T>
```

### File layout (obd2-dash)

No new files. Integration via existing keybinding system and `StorageManager`.

## File Format: `.obd2raw`

### Example

```
# obd2-raw v1
# transport=serial port=/dev/tty.usbserial-0001 baud=115200
# started=2026-03-24T14:30:00.000Z
0.000 W ATZ\r
0.045 R.chunk ELM327 v2.
0.089 R.chunk 1\r\r>
0.089 R ELM327 v2.1\r\r>
0.100 W ATE0\r
0.142 R ATE0\rOK\r\r>
0.200 W 010C\r
0.328 R 41 0C 0A A0\r\r>
```

### Rules

- **Header lines** start with `#`. Contain format version and transport metadata (type, port/device name, baud rate).
- **`W` lines**: exact bytes passed to `transport.write()`.
- **`R.chunk` lines**: each individual buffer fill from the transport's internal read loop. Only appear when a response arrives in multiple chunks.
- **`R` lines**: the complete assembled response (all chunks concatenated). This is the command/response boundary.
- **Timestamps**: seconds with millisecond precision (`%.3f`), relative to capture start.
- **Byte encoding**: printable ASCII rendered literally. Non-printable bytes escaped: `\r` for 0x0D, `\n` for 0x0A, `\t` for 0x09, `\xHH` for everything else. This means `\r` in the log always represents a literal carriage return byte, never a line ending.
- **File line endings**: `\n` (Unix). Protocol `\r` bytes are always escaped in the data payload.

## LoggingTransport API

```rust
/// Metadata written to the file header.
pub struct CaptureMetadata {
    pub transport_type: String, // "serial", "ble"
    pub port_or_device: String, // "/dev/tty.usbserial-0001" or "OBDII BLE"
    pub baud_rate: Option<u32>, // serial only
}

pub struct LoggingTransport<T: Transport> {
    inner: T,
    writer: Option<BufWriter<File>>,
    start_instant: Instant,
}

impl<T: Transport> LoggingTransport<T> {
    /// Wrap a transport. Capture starts inactive (zero-cost passthrough).
    pub fn new(inner: T) -> Self;

    /// Start capturing to a file. Writes the header comment lines.
    pub fn start_capture(
        &mut self,
        path: &Path,
        metadata: &CaptureMetadata,
    ) -> io::Result<()>;

    /// Stop capturing. Flushes and closes the file. Returns the path if active.
    pub fn stop_capture(&mut self) -> io::Result<Option<PathBuf>>;

    /// Whether capture is currently active.
    pub fn is_capturing(&self) -> bool;

    /// Access the inner transport.
    pub fn inner(&self) -> &T;
    pub fn inner_mut(&mut self) -> &mut T;
}
```

### Transport trait forwarding

```rust
#[async_trait]
impl<T: Transport> Transport for LoggingTransport<T> {
    async fn write(&mut self, data: &[u8]) -> Result<(), Obd2Error> {
        self.log_line('W', data);
        self.inner.write(data).await
    }

    async fn read(&mut self) -> Result<Vec<u8>, Obd2Error> {
        let result = self.inner.read().await?;
        self.log_line('R', &result);
        Ok(result)
    }

    async fn reset(&mut self) -> Result<(), Obd2Error> {
        self.inner.reset().await
    }

    fn name(&self) -> &str {
        self.inner.name()
    }
}
```

## Chunk-Level Capture

The Transport trait gets an optional chunk observer callback:

```rust
pub trait Transport: Send + Sync {
    async fn write(&mut self, data: &[u8]) -> Result<(), Obd2Error>;
    async fn read(&mut self) -> Result<Vec<u8>, Obd2Error>;
    async fn reset(&mut self) -> Result<(), Obd2Error>;
    fn name(&self) -> &str;

    /// Set a callback invoked on each raw read chunk before assembly.
    /// Default: no-op.
    fn set_chunk_observer(&mut self, _observer: Option<ChunkObserver>) {}
}

pub type ChunkObserver = Box<dyn Fn(&[u8]) + Send + Sync>;
```

`SerialTransport` and `BleTransport` call the observer inside their read loops:

```rust
// In SerialTransport::read(), inside the loop:
result.extend_from_slice(&self.read_buf[..n]);
if let Some(ref observer) = self.chunk_observer {
    observer(&self.read_buf[..n]);
}
```

`LoggingTransport` installs an observer that logs `R.chunk` lines. When `read()` returns, it logs the final assembled `R` line.

For single-chunk responses (common), only the `R` line appears in the log.

## Integration with obd2-dash

### Startup

The app always wraps the transport in `LoggingTransport`:

```rust
let serial = SerialTransport::new(port, baud).await?;
let transport = LoggingTransport::new(serial);
let adapter = Elm327Adapter::new(transport);
```

Zero overhead when capture is inactive — `log_line` short-circuits on `self.writer.is_none()`.

### Toggle

A keybinding (e.g., `c`) toggles raw capture independently of structured recording:

- **Start**: generates a UUID filename, calls `transport.start_capture(path, metadata)`
- **Stop**: calls `transport.stop_capture()`, registers file with `StorageManager`

### Storage

`.obd2raw` files live in the same `recordings/` directory. `StorageManager` tracks them alongside `.obd2rec` files for size accounting and cleanup.

## MockTransport Generation

A utility function parses `.obd2raw` into command/response pairs:

```rust
/// Extract (command, response) pairs from a raw capture file.
/// Filters to W/R lines (ignoring R.chunk), pairs sequentially,
/// strips \r framing for direct use with MockTransport::expect().
pub fn parse_raw_capture(path: &Path) -> io::Result<Vec<(String, String)>>;
```

This enables the workflow:
1. Capture a real session on hardware
2. Run `parse_raw_capture()` on the `.obd2raw` file
3. Feed pairs into `MockTransport::expect()` for deterministic replay in tests

## Storage Impact

Estimated `.obd2raw` sizes for a 1-hour session:

| Scenario | Approx size |
|----------|-------------|
| Standard PIDs, 250ms poll | ~2-3 MB |
| Heavy (enhanced + O2) | ~5-8 MB |
| With AT init overhead | +~2 KB (negligible) |

Well within the existing 500 MB storage budget. Compression via `StorageManager` applies if files exceed the threshold.
