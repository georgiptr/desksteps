# Development and contributor guide

Install stable Rust for Windows MSVC and the Visual C++ build tools/Windows SDK. SQLite and the C runtime are linked into the release executable; Windows system libraries remain required.

```powershell
powershell -NoProfile -File scripts/build-release.ps1
rustup component add llvm-tools
cargo xtask quality
cargo xtask package -Iscc PATH_TO_ISCC.exe
```

Use `cargo xtask quality --offline` with cached locked dependencies. The checks do not install toolchains, access your WalkingPad database, connect Bluetooth or command hardware. See [testing](TESTING.md).

For a Bluetooth-disabled preview, always select separate synthetic storage:

```powershell
.\target\release\desksteps.exe --no-bluetooth --database .\target\preview.sqlite --timezone UTC
```

Additional flags: `--portable`, `--database PATH`, `--timezone IANA_NAME`, `--no-controls`, `--help`, `--version`. Replay and ordinary preview smoke require an explicit database; development mode selects its own isolated storage.

## Develop while recording

Keep your stable release executable in a separate folder outside this checkout's
`target` directory. This lets Cargo rebuild development and release output while
the stable app continues recording. Preserve the stable copy's existing storage
mode; adding an adjacent `data` folder selects portable storage with a different
database.

**`cargo run` starts the normal app, with Bluetooth and normal storage.** A debug
build does not imply offline operation. **`cargo dev` builds and launches the
debug development simulator**, equivalent to `cargo run -- --development`.
Use `cargo --offline dev` with cached dependencies. The release executable also
accepts `--development` for testing the optimized build without hardware.

Development windows use the red version of the normal icon and the title
**DeskSteps Development — Offline**. A separate native Simulator panel opens
alongside the main window and can be reopened from **Open Simulator** in the tray.
Real Bluetooth is always disabled. The normal Start/Resume, Pause and speed
buttons operate the synthetic pad through the same command coordinator and
accounting pipeline. The simulated belt starts paused; reconnecting does not
start it.

Synthetic history and settings live in `target/development/desksteps.sqlite`
for Cargo builds. Packaged copies use a separate
`development/desksteps.sqlite` beside the executable. Debug and release development
launches share this location. Development mode rejects `--database`, `--portable`,
`--device` and `--replay` overrides. It never imports personal history. First
launch creates seven days of synthetic history and example targets/weight;
later launches preserve it. **Reset development data…** explicitly restores
the sample dataset after confirmation. Nothing deletes normal history.

Development global shortcuts are disabled by default, even if bindings are
saved. To test them, use **Toggle development shortcuts** in the Simulator panel
and configure unused combinations in the app's shortcut editor. The explicit
opt-in is saved only in development storage. Disable it again to avoid conflicts
with the stable app.

The panel supports disconnect/reconnect, synthetic discovery, telemetry silence,
rejected commands, two-second response delays, missing replies, failed writes,
accepted commands without measured motion, malformed packets and step-counter
resets. Fault buttons arm the next command transaction. **Five-minute pause**
and **Cross local midnight** advance a development-only virtual clock, preserving
the normal five-minute session rule and day-allocation logic. Time scenarios
require a connection and no pending command; midnight starts a synthetic
2 km/h walk. The virtual clock survives restarts and resets with the sample data.
To test another timezone, select `cargo dev --timezone Europe/Helsinki` on first use
or after removing development output between runs; an existing database retains
its timezone.

One normal app, one offline preview and one development app may coexist, each
with its own database. Every app/replay writer holds an OS lock on the database
file; a second writer is rejected, including through relative-path, case or
hard-link aliases. Readers such as external SQLite inspection tools are not
blocked by this application ownership lock. Older DeskSteps binaries do not
participate: replace the stable binary with the new release between sessions.
Simulator verification does not establish physical Bluetooth or pad behavior.

## Documentation

- [Using the app](USAGE.md)
- [Bluetooth protocol and verified behavior](PROTOCOL.md)
- [Privacy, storage and backup](DATA.md)
- [Offline testing and coverage](TESTING.md)
- [Build, release and GitHub workflow](RELEASING.md)
- [Completed verification and remaining checks](VERIFICATION.md)

Source is MIT licensed. Retain the application license when redistributing.
GitHub downloads are unsigned; verify their origin/checksums. Do not disable Windows security globally to install them.
