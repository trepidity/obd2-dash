# CLAUDE.md — obd2-dash

## Agent entry (read first)

| Doc | Role |
| --- | --- |
| **`AGENTS.md`** (this repo) | SP-SYS personality + non-negotiables |
| `../HAULLOGIC-CODING-STANDARDS-AND-AGENT-SOP.md` | Program coding SOP |
| `../HAULLOGIC-MASTER-DESIGN-MATRIX.md` | Architecture / ownership |
| `../skills/` or `.claude/skills/` | P0 program skills (OBD session, review, …) |

## Project

Real-time OBD-II diagnostics TUI (and related apps under `apps/`). Protocol I/O via `obd2-core` Session. Proven profiles and scrubbed captures graduate into shared core — Desktop does not reimplement dash-only probes as SoT.

## Commands

```bash
cargo test
cargo run -- --mock
cargo clippy
```

See `README.md` and `MANUAL.md` for hardware CLI options.
