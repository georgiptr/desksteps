# Rust-only testing

Run `cargo xtask quality`, or append `--offline` when dependencies are cached. Prerequisites: Rust MSVC toolchain, rustfmt, Clippy and matching `llvm-tools`. No Python, uv, Cargo coverage plugin or live hardware is required.

UI changes also require screenshot review. Export actual native-window pixels with
`cargo test --lib --locked --offline native_ui_screenshots_of_targets_settings_and_calories -- --ignored --nocapture`.
The opt-in test uses synthetic channels with no Bluetooth or personal database,
the current monitor's DPI, and normal/narrow widths. It writes BMP screenshots
under ignored `target/ui-screenshots/`, covering Targets, Settings, validation
messages and both calorie sources. Open and inspect the images for clipped text,
wrapping and overlap; a passing export test does not establish visual correctness.
Extend the capture cases when another screen is affected. Keep review captures
untracked; public documentation images require explicit authorization and
synthetic data. For the README's four main pages and compact remote, run
`cargo test --lib --locked --offline native_ui_screenshots_for_readme -- --ignored --nocapture`.
It exports BMPs to `target/readme-screenshots/` for review and conversion to PNG
in `docs/screenshots/`. The displayed values are synthetic; no transport runs.

Target display regressions check completed/target values, the centered-dot
percentage separator, separate remaining lines, exact completion, over-target
values and unavailable metrics. Weight validation accepts 20 and 150 kg inclusive,
rejects values outside those boundaries, and keeps blank weight optional.

Use `cargo dev` for interactive debug/simulator work. **Bare `cargo run` starts
the normal Bluetooth-enabled app and is not a routine test command.** For an
optimized development smoke, use the normal `target/release/desksteps.exe` with
`--development --smoke-seconds 3`; it remains offline and uses fixed synthetic
development storage. Ordinary `--no-bluetooth` smoke still requires an explicit
separate `--database`. Keep the stable executable outside Cargo's output folders.

Development regressions exercise the serialized access/action/telemetry stages,
deferred Pause, moving-speed freshness, no retry after rejection/timeout/write
failure, acceptance without measured motion, reconnect without starting motion,
raw decode failures, telemetry silence, synthetic counter resets, five-minute
confirmed-pause closure and local-midnight allocation. Sample-data tests check
idempotence, persistence and marked-database reset restrictions. Native panel
tests check mode gating, caption/icon inheritance, button bounds and typed
command dispatch without Bluetooth. OS ownership tests use separate processes
and hard-link aliases and verify unchanged file bytes and release on close.
The managed filesystem sandbox may prohibit creating hard links; run those
synthetic checks with desktop/filesystem access rather than weakening the test.

Speed regressions submit rapid increases/decreases before delayed indications or
new telemetry and inspect the stored absolute command bytes. They verify one
ordered write per accepted click, reuse of current-connection access, bounded
queues, stale-click rejection, separate selected/measured speed, background target
confirmation, and no retry on write failure or missing motion. Pause clears queued
speeds, and a late speed rejection cannot cancel an active Pause exchange. These
offline checks do not validate rapid writes against physical pad firmware.

The command runs workspace formatting, strict Clippy, instrumented Rust tests, a native Win32/DPI/SQLite smoke with Bluetooth disabled, synthetic replay/rejection/idempotence checks and production-only coverage gates. All generated data/reports remain under ignored `target/coverage` or temporary test folders. It does not open the normal user database.

Enforce at least 95% production line coverage independently for protocol (`protocol.rs`), controls (`control.rs`, `application/controls.rs`, `application/exchange.rs`), storage (`accounting.rs`, `storage/`) and statistics (`statistics/`). Every production source is discovered recursively and reported by relative path; each core area's executable lines are aggregated independently, and missing executable core sources fail. Inline tests belong at the end of source files; dedicated `tests.rs` files and `tests/` folders contain tests only. Their bodies are excluded. Tooling regression tests guard discovery, duplicate LLVM objects, missing areas and coverage inflation. Report Windows BLE adapter, main window, compact remote, toolkit, shortcuts and runtime coverage separately. High core coverage is not whole-app coverage. Rust/LLVM line coverage is not a claim of branch coverage.

Golden protocol cases independently specify handshake nonce/carry/checksum examples, optional-field alignment, signed fields, 24-bit steps, malformed/truncated frames and absent fields. Control cases exhaust all advertised tenth-km/h starting values and user increments, exercise access/response correlation, echoes, settling timestamps, stale generations, boundaries and deadlines. SQLite/statistics cases preserve transaction rollback, restarts, resets, gaps, DST/day boundaries, session grouping and calorie reference arithmetic.

Additional architecture tests prevent domain dependencies on SQL/Windows/UI and prevent UI access to databases or Bluetooth. Scripted transport tests distinguish write completion, protocol acceptance and measured motion. Coordinator tests cover deferred Pause, uncertainty, freshness and connection generations. Form, presentation and queue tests run without windows. Black-box executable replay tests cover raw-byte retention, day boundaries, resets, malformed input and idempotence using explicit temporary databases. Projection failure injection verifies rollback across all recording tables and session-expiration updates. See [ARCHITECTURE.md](ARCHITECTURE.md) for ownership and the contributor map.

Calorie regressions preserve pad-reported totals and hourly values with and without weight, verify independent app arithmetic and dated source selection across days, and cover cumulative plateaus, calorie ticks without step ticks, unknown baselines, missing fields, resets, reversed timestamps, monotonic gaps, multiple devices and excluded diagnostics. Synthetic database reopen/backfill tests preserve evidence, saved recovery decisions, receipt-date attribution and idempotence. Failure injection covers the calorie projection in the complete recording transaction. UI checks cover source labels, targets, sessions, recovery details, history, unavailable estimates and the short Settings explanation.

Native window regression tests (`native_ui_` prefix, skipped by the coverage run) read the actual Win32 captions and icon handles of the main window and compact remote and use distinct shared test icons to check inheritance. They drive the main window with synthetic snapshots: every page, resize across the rail breakpoint, real press/release and double-click messages, keyboard Tab/arrow/Enter/Escape navigation, Today/History/day-detail requests and the Back scroll position, the Targets form (inline messages, confirmation, and no preference write when the window is closed during the confirmation), the Settings shortcut and step controls, theme changes and render-target loss. They assert that nothing is focused on open, that the first Tab never lands on Start or the speed buttons, and that Enter or Space never activates a control whose focus ring is hidden (for example after a mouse press). The compact remote test checks pinning, command submission with the published generation, and the same focus rules. All channels are inert; no Bluetooth transport is created and no system pointer is moved.

The executable smoke opens the main window and the compact remote and verifies their captions and both icon sizes against the real app icon, including the hidden tray owner.

Startup regressions launch a fresh synthetic database and then reopen it with
Bluetooth and controls disabled. The smoke report records actual main-window
visibility and minimized/taskbar state before the smoke opens any windows:
visible on first launch, then open, taskbar-minimized or tray-only according
to the saved options. Native window tests check explicit taskbar styles on
main and Compact, and exercise ordinary minimization,
hide/restore, main-window close-to-tray versus Exit, and Compact-only close.
Worker tests check persisted window preferences and reject Windows startup
changes in preview and development modes. The Windows startup adapter test
creates and deletes a uniquely named disposable registry key; it never writes
the real Windows Run key. Registry tests require normal desktop permissions.
The Settings screenshot exporter also covers startup toggle states, inline
startup failures, light/dark themes and normal/narrow widths.

Resource/update regressions check that windows on one UI thread share a Direct2D
factory, release the last factory owner on close, and recover one render target
without disturbing another. Statistics snapshots share immutable dashboard data
while retaining their original version. Native refresh tests skip diagnostic-only
repaints while preserving fresh-speed control gating, theme changes, late form
defaults, unsaved edits, page navigation and restored History scroll. An actual
offline worker test drives Start, steps, speed changes, telemetry silence, a
delayed command followed by Pause, and paused reconnect. Idle simulator wakeups
follow its next telemetry tick; pending controls, telemetry and speed-freshness
deadlines keep their tighter bounds. These are behavior checks, not fixed CPU or
memory thresholds: Windows and graphics-driver allocations vary by machine.

Settings regressions verify the folder action reaches its owner, omit the Mode
and Last telemetry rows, and retain the storage-location explanation. Synthetic
path tests cover portable auto-detection, forced portable, installed and explicit
database folders. Day presentation checks cover independent target percentages,
whole-number minutes/kcal and the one-minute Sessions heading threshold.
Step-display regressions preserve the prominent actual count and exact
`completed/target · percent` text, share the live-card percentage with Compact,
and verify that the live card always ends at the progress bar. There is no
command-feedback field, message row or conditional card expansion.

Pure layout, input, theme, chart and view-model code carries ordinary unit tests at narrow, medium and wide sizes and several DPI scales. The theme tests compute WCAG contrast for text, secondary text, error text, accent labels, pill text and focus rings on every surface in light, dark, every accent variant, development red and a high-contrast palette. The layout tests round every key element to device pixels at 96, 144 and 216 DPI and require it to stay inside the window and clear of its neighbours. A native test paints every screen in the light, dark and high-contrast themes at those three scales.

**Checking dark mode without changing Windows settings.** Development mode only: set `DESKSTEPS_DEV_THEME=dark` (or `light`) in the environment of a `--development` launch to override the automatic app theme. An explicit Light or Dark choice in Settings takes priority. The environment override is ignored outside `--development`, and it never writes a setting.

**UI Automation.** Pure tests describe the element tree for each page (names, enabled state, toggle state, off-screen flags). A native test drives the COM provider directly (navigation, properties, patterns, runtime ids) and checks that Invoke only posts a message to the window. Another native test sends those messages to a window and requires that Start/Resume submits directly when belt state is unknown and that Pause stays available. Shared-action tests cover both known and unknown state, ordinary and development modes, and blocked controls. The real Windows UI Automation client can read a running development window with `System.Windows.Automation` from PowerShell.

Release acceptance additionally includes actual visual/keyboard/DPI review, compact-window lifecycle, shortcut conflict/repeat/lock behavior, writable/non-writable portable paths, installer/upgrade/uninstall and final binary dependency inspection. Record the checks actually performed and their limits in `VERIFICATION.md`; do not infer acceptance from a source commit alone.
