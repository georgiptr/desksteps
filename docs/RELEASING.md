# Building and releasing DeskSteps

DeskSteps targets Windows 11 x64. The executable is `desksteps.exe`; the source
and packages are MIT licensed.

## Build and package

1. Install stable Rust for Windows MSVC, the Visual C++ build tools/Windows SDK,
   and Rust's rustfmt, Clippy and matching llvm-tools components.
2. Run `cargo xtask quality`. Add `--offline` for cached dependencies.
   `--headless` omits interactive native checks for hosted CI; desktop acceptance
   still requires the full check and screenshot review.
3. Install Inno Setup 6 separately. The packaging workflow pins 6.7.3 and verifies
   the compiler download's checksum and Windows signature.
4. Run `cargo xtask package -Iscc PATH_TO_ISCC.exe`. This rebuilds the normal
   release executable, collects locked dependency license texts, creates the
   portable ZIP and installer, and writes `dist/SHA256SUMS.txt`.
   Packaging never launches the app.
5. Review executable version metadata, dependencies, hashes, license texts,
   archive contents and installer behavior. Use isolated synthetic data for
   portable, installed, upgrade and uninstall checks.

Each package build stages files under a unique ignored `target/package/`
directory. The portable ZIP includes an empty `data` directory; the installer
omits it. Preserve existing runtime data when replacing an installed or portable
executable. Keep recording copies outside Cargo output folders.

## Versioning

For a standalone release binary, run
`powershell -NoProfile -File scripts/build-release.ps1` (add `-Offline` for cached
dependencies). Packaging uses the same script. It preserves static CRT linkage
and remaps compiler source paths for the checkout, profile and dependency caches,
so published binaries do not disclose the build account or local source paths.

Set the application version in Cargo.toml, update Cargo.lock, and keep the
four-part identity in app.manifest aligned. Windows numeric and string version
resources are generated from the Cargo version. For this release they are
`1.0.0.0` and `1.0.0`.

## GitHub workflows

The check workflow runs Windows headless quality checks and retains coverage
reports for seven days. It requires no Bluetooth, private database or signing key.

Manual dispatch of the release workflow builds downloadable CI artifacts.
Pushing a matching version tag, such as `v1.0.0`, creates a **draft**
GitHub Release, or replaces the installer, ZIP and checksums on an existing
release while preserving its publication status. Review the installer, ZIP,
checksums and [verification
notes](VERIFICATION.md) before publishing the draft. A source push alone does
not publish packages. Hosted behavior remains unverified until the workflow runs.

Authenticate through GitHub-supported local tooling; keep credentials outside
source control. Review tracked files before pushing a source branch. Never
upload a working folder, Git metadata, private instruction files, build history
backups, runtime databases, captures or tool downloads as source or release assets.

Both packages are unsigned. Checksums detect changes but do not provide a
publisher signature. Keep Windows protections enabled. Installed upgrades and
uninstallation must preserve activity data.
