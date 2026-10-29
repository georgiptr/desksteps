# Native UI design

The main window uses a navigation rail with an always-visible live card, plus
Today, History, Targets and Settings pages. A separate compact remote shares
the same movement controls. UI code submits typed application commands and
reads immutable snapshots; it never opens SQLite or Bluetooth directly.

Direct2D and DirectWrite draw the interface. Native edit controls handle text
entry and a system month calendar handles date selection. Automatic, Light and
Dark appearance share a central theme; Windows high contrast takes priority.
Development mode has a red identity and an offline caption.

Layouts adapt to narrow windows and monitor DPI. The live card retains its
actual step count and target percentage; day tiles show completed/target values
and a separate remaining line. Partial coverage and recovered activity remain
explicit. Selected speed is distinct from measured speed. Start/Resume and Pause
are separate actions routed through the shared command coordinator.

Tab order reaches Pause before movement controls. Arrow keys select pages,
Ctrl+1 through Ctrl+4 switch pages, and Enter/Space activate only a visibly
focused control. UI Automation exposes named elements and native text fields.
Property-change and focus events are not raised; clients see updates when they
next read an element. Tooltips are mouse-hover only. The calendar retains its
system appearance, and window placement is not persisted between launches.

For every UI change, inspect actual rendered screenshots at normal and narrow
widths and high DPI, using synthetic offline data. Automated layout assertions
do not establish visual correctness. See [testing](TESTING.md) for the native
screenshot exporter and desktop acceptance checks.