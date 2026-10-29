# Using DeskSteps

Choose the installer or portable ZIP from GitHub Releases when a release has been published. Both contain the same app. Extract portable downloads to a writable folder and run `desksteps.exe`; keep the included `data` folder beside it. That folder selects portable mode, with history in `data\desksteps.sqlite`. Without it, the app uses `%LOCALAPPDATA%\DeskSteps`. The installer does not include an adjacent `data` folder.

The first launch with a new database opens the main window on Today, after confirming the detected timezone. Later launches follow **Start minimized** and **Minimize to tray** in Settings. The tray's **Connect Automatically** checkbox is enabled by default and remembered across launches. When checked, DeskSteps connects to the saved pad; if no pad is saved, it runs a 10-second active scan and saves and connects exactly one matching pad after the full scan. Multiple matches require selection from **Choose device**. A saved pad remains the default on later launches.

Use **Scan** to run another 10-second active scan. With automatic connection unchecked, scanning only discovers pads; selecting one saves the default without connecting. Check **Connect Automatically** to connect. Unchecking it stops connection attempts and reconnection while keeping an existing connection open. **Disconnect** appears only while connected and also unchecks and saves the automatic connection setting.

**Choose device** disappears after you choose a pad. Use **Scan** to show the choices again; a checkmark identifies your saved selection, including when only another pad has been rediscovered. Choosing the checked entry closes the chooser without reconnecting. Before a default is saved, discovered pads remain available to choose.

Use **Open** for the main window or **Open Compact** for the compact remote. Settings has startup/tray preferences, device controls (automatic connection, Scan, Disconnect and the saved pad), the speed step, global shortcuts and the data folder with the closed-app backup steps. Targets holds the dated weight and daily targets.

Settings → **Startup and tray** saves changes immediately:

- **Start with Windows** is off by default. Enabling it registers this executable for the current user's sign-in, with no administrator rights needed. Its initial window state follows the same startup preferences as manual launches. Windows Startup apps can override this registration. Offline preview and development modes cannot register startup. Moving a portable copy requires enabling startup again from its new location; disable startup before removing it. Installed uninstallation removes the registration only when it belongs to that installation.
- **Start minimized** is on by default. Later launches start minimized on the taskbar when **Minimize to tray** is off, or hidden in the tray when it is on. Turn **Start minimized** off to open the main window normally. First launch with a new database and development mode always open the main window.
- **Minimize to tray** is off by default. When enabled, the minimize button hides the main window or Compact; use the tray's **Open** or **Open Compact** to restore it. When off, visible windows have taskbar buttons and minimize normally to the taskbar.
- **Keep running when closed** is on by default. Closing the main window leaves recording running in the tray. Turning it off makes the main window's close button exit the whole app and stop recording. Closing Compact always dismisses only the remote. Tray → **Exit** always closes the app. Exiting does not stop the belt.

The keyboard works throughout: Tab moves through Pause, the pages and the page's controls, with Start and the speed buttons last; arrow keys move within the page list, Enter or Space activates only a control whose focus ring is showing, and Ctrl+1 to Ctrl+4 switch pages. Nothing is focused when a window opens.

Start/Resume uses a speed selected by the device. If current belt state is unknown, the app asks before sending Start. Pause preserves current workout counters; Disconnect and Exit do not stop the belt. Keep the physical remote available. The complete 1–6 range is advertised by the tested pad; physical PC trials have covered 1.0, 1.1 and 3.0, not every speed.

Settings → **Recover activity after gaps** is on by default, including after upgrading. It adds retained step-counter increases after app restarts, Bluetooth/data gaps and day boundaries. Usable elapsed-counter differences add estimated walking minutes; distance/time and dated weight support estimated active kcal. Recovered contributions appear separately in day/session details and history chart tooltips, and coverage stays partial. Elapsed time assumes walking and may include empty-belt time; known empty-belt intervals without steps or distance add no walking minutes. Device-reported calories are not mixed into the app estimate.

Turning recovery off preserves the original exclusion behavior for future gaps. Changing the switch does not rewrite earlier totals. Gaps crossing midnight are assigned to the receipt/reconnect date. Initial unknown baselines, detected resets and invalid or missing counters can still leave progress uncredited. DeskSteps does not promise complete history recovery or precise individual footfall timestamps.

See [protocol compatibility](PROTOCOL.md), [data and backup](DATA.md), and [development](DEVELOPMENT.md) for details.
