#!/usr/bin/env bash
#
# Start the Ircama ELM327-emulator with the 2020 Malibu scenario.
# Prints the PTY path and the cargo command to connect.
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
EMU_OUT=$(mktemp /tmp/elm_emu_XXXXXX.txt)

# ── Check dependencies ────────────────────────────────────────────────────────
if ! command -v python3 &>/dev/null; then
    echo "Error: python3 not found. Install Python 3." >&2
    exit 1
fi

if ! python3 -c "import elm" 2>/dev/null; then
    echo "ELM327-emulator not installed. Install with:"
    echo "  pip install ELM327-emulator"
    exit 1
fi

# ── Start emulator in background ─────────────────────────────────────────────
# Feed merge + scenario commands via stdin; batch mode writes PTY path to file.
echo -e "merge malibu_2020\nscenario malibu_2020" | \
    python3 -m elm -b "$EMU_OUT" &
EMU_PID=$!

# Wait for the emulator to write the PTY path and finish processing commands
for i in $(seq 1 20); do
    if grep -q "End of batch commands." "$EMU_OUT" 2>/dev/null; then
        break
    fi
    sleep 0.25
done

if ! grep -q "End of batch commands." "$EMU_OUT" 2>/dev/null; then
    echo "Error: Emulator did not start within 5 seconds." >&2
    kill "$EMU_PID" 2>/dev/null || true
    rm -f "$EMU_OUT"
    exit 1
fi

# First line is the PTY device path
PTY_PATH=$(head -1 "$EMU_OUT")

if [ -z "$PTY_PATH" ] || [ ! -e "$PTY_PATH" ]; then
    echo "Error: Could not determine PTY path from emulator output." >&2
    echo "Output file contents:"
    cat "$EMU_OUT" >&2
    kill "$EMU_PID" 2>/dev/null || true
    rm -f "$EMU_OUT"
    exit 1
fi

# ── Print connection info ────────────────────────────────────────────────────
echo "============================================"
echo "  ELM327 Emulator (2020 Chevy Malibu 1.5T)"
echo "============================================"
echo ""
echo "  PTY device: $PTY_PATH"
echo "  PID:        $EMU_PID"
echo ""
echo "  Connect with:"
echo "    cargo run -- --emu --port $PTY_PATH"
echo ""
echo "  Run integration test:"
echo "    cargo test -p obd2-core --test elm327_emulator -- --ignored"
echo ""
echo "  Press Ctrl+C to stop the emulator."
echo "============================================"

# ── Cleanup on exit ──────────────────────────────────────────────────────────
cleanup() {
    echo ""
    echo "Stopping emulator (PID $EMU_PID)..."
    kill "$EMU_PID" 2>/dev/null || true
    rm -f "$EMU_OUT"
}
trap cleanup EXIT INT TERM

# Wait for the emulator process
wait "$EMU_PID" 2>/dev/null || true
