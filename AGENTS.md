# DeskSteps contributor guidance

Keep explanations clear and discuss high-level product changes with the maintainer.

Use `docs/DEVELOPMENT.md` for build commands, offline simulator setup and
contributor documentation; `docs/USAGE.md` covers detailed app behavior.

For every UI change, take and inspect actual rendered screenshots before calling
the work complete. Check the affected screens at normal and narrow widths,
including high DPI scaling, for clipping, wrapping and overlap. Automated
layout checks and startup smoke tests do not replace screenshot review. Use
synthetic offline data; keep review screenshots out of source commits. Explicitly
requested public documentation screenshots may be tracked after privacy review.

Launch commands are deliberately different: `cargo run` starts the NORMAL app,
including Bluetooth and normal history. A debug build is not automatically safe
for development. Use `cargo dev` (or `desksteps.exe --development`) for the red
`DeskSteps Development — Offline` simulator, isolated synthetic storage and
global shortcuts disabled by default. Never use bare `cargo run` as a routine
test. Keep any installed or portable recording copy outside Cargo output folders.

Activity recovery is enabled by default. Preserve raw gap classifications and
store recovered step/time/calorie contributions separately and transactionally.
The Settings switch affects future gaps, not historical totals. Detected resets
and unknown baselines remain uncredited; recovered time assumes walking, and
active-kcal estimates require usable distance/time and dated weight. Attribute
cross-midnight recovery to the receipt date and retain partial-coverage labels.

DeskSteps is a small Rust/Win32/SQLite app for Windows 11 x64 and KingSmith WalkingPad Z1. Preserve conservative step accounting, raw evidence, uncertainty, transactional anchors, and five-minute confirmed-pause session grouping. Do not add a browser engine, service, telemetry upload or cloud integration. Settings and history are private runtime data, never fixtures or source commits.

Keep third-party binaries, real captures and personal records out of this project. Use synthetic or appropriately licensed public fixtures. The public wire reference is `docs/PROTOCOL.md`.

Run `cargo xtask quality` (or `cargo xtask quality --offline` when cached). Keep 95% production line coverage individually for protocol, controls, storage and statistics. Exclude test bodies from gates; report lower Windows adapter/UI coverage explicitly. All routine checks use synthetic data with Bluetooth disabled.

Never launch a normal Bluetooth-enabled executable as a test without fresh operator confirmation of the physical pad state and readiness. Software implementation approval is not hardware-trial approval. Use the physical remote as fallback. The advertised 1–6 km/h range does not authorize a physical high-speed test. No movement on startup/reconnect, retries after uncertain outcomes, Stop/reset/preference/firmware sender or global Start shortcut.

Use one serialized command transaction shared by every control surface. A successful GATT write is not protocol acceptance, and acceptance is not measured motion. Keep Pause available at the next valid exchange boundary. Reject stale generations and nonfresh speed adjustments.

Preserve all raw diagnostic records initially; do not introduce automatic deletion. Backup is a documented, closed-app procedure. Portable data lives beside the executable; installed data uses AppData. Do not import unrelated application data.

Do not push or publish release assets without explicit maintainer authorization. Rebuild and verify the normal `target/release/desksteps.exe` as well as any separately instrumented test binary.
