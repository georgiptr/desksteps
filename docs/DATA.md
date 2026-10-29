# Local data, privacy and backup

DeskSteps stores settings and walking history in SQLite. No application data is uploaded. GitHub is used for source distribution and manually downloaded releases, not health-data synchronization.

Installed mode: `%LOCALAPPDATA%\DeskSteps\desksteps.sqlite`.

Portable mode: `data\desksteps.sqlite` relative to the executable's folder. An adjacent `data` directory selects portable mode, even when empty; the portable ZIP includes it. `--portable` also selects this mode and creates the directory if needed. An ordinary file named `data` does not select portable mode. Moving the complete folder carries settings/history. A non-writable location is an error; the app does not silently switch to AppData. Windows can retain its own ordinary execution/download traces.

Without an adjacent `data` directory or `--portable`, the app uses installed mode. Do not remove the portable `data` directory during updates: it contains your history and selects the storage mode. Adding one beside an installed executable selects portable mode without moving existing AppData history.

An explicit `--database PATH` overrides the default. Database timezone is saved and reused; conflicting explicit timezone selection is rejected rather than mixing daily allocations. Changing the Windows timezone does not rewrite existing history.

Development mode is separate: `cargo dev` / `--development` uses only
`target/development/desksteps.sqlite` for Cargo builds, or `development/desksteps.sqlite` beside a packaged executable. Normal
`cargo run` retains the installed/portable rules above. Development contains
synthetic history, never imports personal records and rejects storage/device
overrides. Its confirmed reset clears only the marked synthetic database and
restores sample history/settings. No automatic data-retention deletion is added.

All app and replay writers acquire a nonblocking exclusive byte-range lock on
the actual database file at offset 1 TiB, beyond SQLite's own locking region.
It does not grow the file or overlap SQLite readers/writers. Ownership is taken
before app settings/schema access and held until the SQLite connection closes;
process exit or a crash releases it. Aliases of the same file conflict, even
across processes. A second app fails before creating its tray/windows. Replay
fails before schema initialization. Other SQLite tools and older app versions
do not honor this extra ownership protocol; it is not a general filesystem
access restriction or a replacement for closed-app backups.

## What is saved

Device identity/address, firmware metadata, received GATT bytes, receipt UTC and monotonic times, connection identities, decode errors, control-write intent, accounting decisions, restart anchors, session/day summaries, targets, optional weight and control/shortcut preferences. Receipt timestamps are not individual footfall timestamps. Raw records currently have no automatic deletion policy, so disk usage grows with recording.

The SQLite file is not separately encrypted by DeskSteps. It inherits filesystem/account protections. Treat databases, WAL files, exports, support screenshots and backups as private. Do not attach them to a public GitHub issue. Share redacted error text and synthetic reproduction cases instead.

## Safe manual backup

1. Choose **Exit** from the tray and wait for DeskSteps to exit. Closing the dashboard alone does not stop the recorder. Exiting does not send Pause or stop the belt; operate the pad as needed with the physical remote.
2. Copy the complete data directory to a private backup location. Include the SQLite file and any remaining `-wal`/`-shm` files together. Never copy only the main SQLite file while a writer is running.
3. Keep the backup separate from the application/update folder and outside Git. Protect it like other personal activity data.
4. To restore, exit DeskSteps first, preserve the current data directory as a separate backup, then restore the complete saved directory. Keep its original timezone.

Installed upgrades and uninstallation preserve the data directory. Portable updates replace the executable/documentation while preserving `data\`. Only one normal Bluetooth-enabled instance runs in a Windows session. An offline preview and a development simulator have separate instance gates and may run beside it with separate databases. A different executable folder alone does not isolate storage; preserve your selected installed/portable mode.

## Accounting boundaries

Raw samples/events and transactional anchors remain authoritative. Directly observed positive increments require close ordered samples in the same connection/day. The default-on **Recover activity after gaps** setting additionally credits positive retained-counter differences after gaps. Original sample quality is preserved; separate `recovery_credits` rows record credited steps, estimated walking milliseconds and an optional active-kcal factor. `recovery_baselines` preserve the corresponding counter snapshots across process restarts. Evidence, credit, baseline and session projections commit together, with source-event uniqueness preventing double counting.

Every decoded sample retains the pad's reported `calories_kcal` and `calories_per_hour` in `samples.decoded`, regardless of weight; notification bytes remain in `events.raw`. The additive `pad_calories` table separately projects cumulative counter increases, with recovered, incomplete and accepted-baseline flags. It commits with the raw sample and existing projections. Repeated cumulative readings contribute zero; unknown initial amounts, missing fields, decreases and uncredited gaps are not invented as calories. Rejected out-of-order samples cannot replace the calorie baseline. Empty-belt diagnostic runs stay excluded. Existing samples receive missing projections transactionally on open, using their saved recovery decisions rather than the current recovery switch. Original samples and app estimate factors are unchanged.

Without weight effective on the displayed date, daily/session totals and history show pad calories. With weight, they show the app's estimated active calories, with no fallback to the pad if estimation inputs are missing. These are different measures and can diverge. The calorie target uses the displayed source. Partial pad totals remain labelled; recovered pad increments follow existing recovery attribution, including the receipt date across midnight.

Recovery uses the same device's ordered counters and rejects detected step/time/distance decreases. Unknown initial counters are not credited. Elapsed-counter increases require positive steps or distance and must fit the wall-clock gap with two seconds of timestamp/counter tolerance. No overflow rollover is inferred. Recovered elapsed time assumes walking throughout the device workout and may include unobserved empty-belt time. The calorie estimate uses average distance/time speed in the existing flat-treadmill MET model, above resting expenditure, and weight effective on the receipt date. Missing distance/time, unsupported speeds or missing weight leave the corresponding calorie estimate unavailable, not an invented zero. Partial calorie totals are labelled when recovered time cannot be estimated.

The setting affects subsequent recording decisions only: enabling does not backfill historical gaps, and disabling does not remove earlier recovery credits. Upgrades can seed the next recovery from the last original accounting anchor. Cross-midnight recovery is allocated to the receipt date without inventing a split. Coverage labels remain partial even when the counter difference was recovered. Five minutes of confirmed pause close an app session at the pause boundary; a Bluetooth disconnect is not a physical pause.

A recorder restart or reconnect during a confirmed pause can rejoin the same
session within that five-minute window without adding a recording-gap warning.
This requires the first ordered sample after the interruption to retain the
same known step and distance counters and a nondecreasing known workout clock.
The clock may advance during startup before new steps arrive. Missing counters,
detected resets, unobserved activity and interruptions without a confirmed pause
retain uncertainty; a clean restart never clears an earlier genuine gap.
Session-rule upgrades replay saved events transactionally to correct derived
grouping/coverage, reproducing every existing interval's steps, time and calorie
factor exactly or keeping the earlier projection. Raw evidence, sample quality,
accounting anchors and recovery credits are not rewritten.
