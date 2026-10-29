# Release verification

Version: **1.0.0**, Windows 11 x64. Local verification: 2026-10-29.

## Completed checks

- `cargo xtask quality --offline` passed formatting, strict workspace Clippy,
  **326 tests**, native Win32/DPI smoke, synthetic replay/rejection/idempotence,
  and the four independent production line-coverage gates. Test bodies are
  excluded from those gates.
- Production coverage: protocol **100.00%**, controls **99.38%**, storage
  **98.60%**, statistics **100.00%**. Windows adapter/UI coverage is lower:
  Bluetooth **2.62%**, discovery **25.00%**, main-window controller **85.51%**,
  painting **69.57%**, compact remote **82.80%**, Windows startup adapter
  **80.88%**. Core coverage is not whole-app
  coverage or a claim of branch coverage.
- The separately invoked documentation screenshot exporter passed. Actual native
  renders of Today, History, Targets and Settings were inspected at normal and
  narrow widths at 216 DPI / 225%, together with the compact remote. The nine
  captures use inert synthetic snapshots, without Bluetooth or personal data.
  All five public PNGs were regenerated from the current renderer. Settings
  now includes the startup/tray preferences; the other four PNGs are unchanged.
  The images are tracked under `docs/screenshots/` and included in the README
  and packages; narrow review captures remain untracked.
- Startup/tray regressions check first-database window visibility, later
  open/taskbar-minimized/tray-only launches, preference persistence, explicit
  taskbar styles on main and Compact, minimize/restore, main-window
  exit and Compact-only close. Windows startup registration was exercised
  against a disposable registry key, without changing the real Run entry.
  Updated native Settings screenshots were inspected at normal/narrow widths
  at the current 216 DPI / 225%, in light/dark themes, with default/changed
  toggles without explanatory text, startup errors and the lower shortcut/data
  sections. These review
  captures remain untracked; the public Settings screenshot was refreshed.
- `powershell -NoProfile -File scripts/build-release.ps1 -Offline` rebuilt the
  normal `target/release/desksteps.exe`. Windows string version fields report
  **1.0.0**; numeric fields report **1.0.0.0**. Import inspection found Windows
  system libraries, with no external SQLite or Visual C++ runtime DLL.
- The normal release passed an isolated native smoke with Bluetooth and controls
  disabled, a dedicated synthetic database and clean automatic exit. A new
  synthetic database opened the main window; reopening the same database
  started minimized on the taskbar with the default preferences. The locally
  updated executable matched the release SHA-256 and passed isolated offline
  first-run and subsequent-launch smokes; existing runtime storage mode and
  database hashes were preserved.
- A relocated copy passed a development-mode smoke. It created its isolated
  `development/desksteps.sqlite` beside that copy, with Bluetooth disabled.
- A fresh optimized offline development smoke with idle main and compact windows
  averaged **51.68 MiB total working set** and **0.026% whole-machine CPU** across
  20 samples over 20.35 seconds after startup. These support the README's
  qualitative efficiency description, not a resource guarantee: this short idle
  sample does not measure active walking or physical Bluetooth, and usage varies
  with workload, Windows and graphics drivers.
- Release source-path remapping removed the build profile and checkout paths
  from the executable in ASCII and UTF-16 scans. Tracked source/docs were
  reviewed for personal paths, private deployment instructions, real device
  identifiers and credentials.
  synthetic addresses, public protocol constants and third-party notices remain.
- PowerShell release/packaging scripts passed syntax parsing, the Windows
  manifest parsed as XML, and `git diff --check` passed.

Normal release executable SHA-256:
`241783D7BD6DA11150DAE66D4C8EBB0ECBBBAD23FABF0C7A6C9E3C22D9D245C8`.
This is a local build fingerprint; published package checksums are generated
separately during packaging and can differ after another build.

## Package acceptance

- The updated installer and portable ZIP were rebuilt locally with pinned Inno
  Setup 6.7.3 after the startup/tray changes and screenshot refresh. The compiler
  download's SHA-256 and Windows signature were verified. Hosted results are
  available in the [release workflow](https://github.com/georgiptr/desksteps/actions/workflows/release.yml).
- Both rebuilt packages matched `SHA256SUMS.txt`. Archive review confirmed the expected
  executable, documentation, screenshots, license notices and empty portable
  `data` folder, without runtime databases or private contributor files.
- The packaged executable reports version **1.0.0**. ASCII and UTF-16 scans
  found no unremapped build-account or checkout paths. Import inspection found
  Windows system DLLs, without external SQLite or Visual C++ runtime DLLs.
- Portable and installed copies passed synthetic replay and native startup
  checks with Bluetooth and controls disabled and explicit synthetic database
  paths. Relocated development-mode smoke checks also passed.
- Portable extraction over an existing synthetic history preserved that history.
  An isolated per-user installation, same-version reinstallation and uninstall
  succeeded. Reinstallation and uninstall preserved the tested synthetic history
  and development databases. The installer also correctly refused installation
  while a normal DeskSteps instance was running in the earlier package acceptance.
  The updated packages repeated the isolated install, same-version reinstall,
  uninstall, replay, native startup and data-preservation checks. These checks
  did not exercise migration from an older release.

The local packages contain the normal executable fingerprint recorded above.
Hosted builds can have different executable fingerprints. Published package
checksums are distributed with the [release downloads](https://github.com/georgiptr/desksteps/releases/tag/v1.0.0).

## Remaining limits

- This change has not been tested through an actual Windows sign-in. The
  installer startup-entry cleanup has been reviewed in source; deleting an
  actual Windows startup registration through uninstall has not been exercised.
- The executable is unsigned. No fresh physical Bluetooth or movement trial ran.
  Synthetic tests do not establish pad or firmware behavior. Supported model
  identifiers, observed firmware and the limits of earlier attended observations
  remain in [PROTOCOL.md](PROTOCOL.md).
