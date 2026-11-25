# A GNOME/Wayland version of MegaSIP

Status: proposal, nothing built yet. Written 2026-11-25 against `main` at `725b714`
(after "One account per window" and the short-name change).

## Summary

- **Most of MegaSIP is already toolkit-independent.** About 9,000 of the ~13,000 lines of
  Rust and QML (SIP engine, media, config, database, IPC, secrets, logging, the debug
  buffers, the CLI and `--source`) have no Qt in them at all. The Qt coupling is concentrated
  in `src/bridge.rs` (~1,000 lines), `cpp/app.cpp`, `build.rs`, and ~3,900 lines of QML.
- **The hard GNOME problems are not the toolkit.** GNOME Shell has no system tray, does not
  let apps raise their own windows on Wayland, and has its own rules for notification
  actions. Those hit a Qt build running on GNOME just as hard as a GTK build. A GTK port
  fixes the *look*; only a desktop-integration layer fixes the *behaviour*.
- **Recommendation:** split into a `megasip-core` crate plus frontends, do the GNOME
  behaviour fixes in a shared integration crate first (the Qt app benefits too), then build a
  GTK4/libadwaita frontend with relm4. Rough total: **6 to 10 weeks** of focused work for one
  developer, plus a permanent cost: every UI feature from then on is written twice.
- **Before committing, decide whether a native GNOME look is worth that permanent cost.**
  A cheaper option (Option A below, 2 to 3 weeks) makes the existing Qt app behave
  properly on GNOME without looking native.

## Decisions (2026-11-25)

- **Option A.** The Qt app is made to work on GNOME. There will be no GTK/libadwaita
  frontend. The GTK sections below stay for the record.
- **Packages are `.deb` and `.rpm` only.** No Flatpak or Snap, so risk 5 (PID namespaces)
  does not apply. The reasons are in the README under "Packaging".

### Ubuntu GNOME vs Fedora's vanilla GNOME

Both run the same GNOME Shell and Mutter, so window raising, notifications, always-on-top,
PipeWire audio and GNOME Keyring behave the same. Where they differ:

- **Tray:** Ubuntu enables its AppIndicator extension by default, so the existing Qt tray
  icon (StatusNotifierItem) should just appear. Fedora has no tray unless the user installs
  `gnome-shell-extension-appindicator`. MegaSIP needs to detect a missing
  `org.kde.StatusNotifierWatcher` and not hide into a tray that isn't there.
- **Dock:** Ubuntu has a permanent dock that can show a badge count (a possible missed-call
  count). Fedora only has the dash in the overview. Cosmetic.
- **Build dependencies:** the KDE Frameworks 6 packages exist on Fedora 40+ and Debian 13,
  and on Ubuntu from 24.10; Ubuntu 24.04 LTS does not have them. On either distro the
  `org.kde.desktop` style needs `qqc2-desktop-style` installed, or Qt falls back to a
  plainer style.
- **Notifications** (correction to risk 3 for Option A): off Plasma, KNotification falls back
  to `org.freedesktop.Notifications`, which delivers action clicks to the process that sent
  the notification. So Answer/Decline may already work per window on GNOME. Untested.

## What is toolkit-independent today

| Module | Lines | Qt/KDE? | Notes |
|---|---:|---|---|
| `engine/` (transport, msg, sdp, stun, call, mod) | ~4,100 | No | tokio thread, `Command` in, `Event` out. The cleanest seam in the codebase. |
| `media/` (hub, dsp, source, mod) | ~2,050 | No | cpal + PulseAudio backend works on GNOME (pipewire-pulse). Two comments mention Plasma. |
| `config.rs` | 585 | No | settings.json, accounts, short names, merge of concurrent saves. |
| `db.rs` | 258 | No | SQLite account locks and history. |
| `ipc.rs` | 70 | No | Per-PID Unix socket. |
| `secrets.rs` | 70 | No | Secret Service, so it already works with GNOME Keyring. Only the doc comment and README say "KWallet". |
| `logging.rs`, `debug.rs` | ~570 | No | Files, Venator, ring buffers for the Debug window. |
| `store.rs` | 164 | No | Contacts, history entries. |
| `cli.rs` | 87 | No | `--cli` headless mode. Already a second "frontend" over the engine, which is good evidence the seam works. |
| `main.rs` | 175 | Mostly no | Argument parsing, `-a` short-name resolution, `--source`, handing requests to running windows. Only the last line (`run_qt_app`) is Qt. |

So the CLI contract (`-a <short name>`, `--source`, `megasip 1002`, `--answer`/`--hangup`/
`--quit`, one account per window, the lock table) is already UI-free and should be shared
verbatim by both frontends.

## What is Qt/KDE-specific

| Piece | What it does | GNOME equivalent |
|---|---|---|
| `bridge.rs` | The `Phone` QObject: JSON properties down to QML, invokables up. **Also holds real logic** (see below). | Split: logic to core, a thin adapter per frontend. |
| `cpp/app.cpp`, `appcpp.rs`, `build.rs` | `QApplication`, `org.kde.desktop` style, KI18n context, screenshot debug aid, link order fixes. | `adw::Application` setup in Rust. |
| `Main.qml` | `Kirigami.ApplicationWindow`, tabs, account picker, error banner. | `AdwApplicationWindow` + `AdwViewStack`/`AdwViewSwitcher`, `AdwToastOverlay` for errors. |
| `Platform.SystemTrayIcon` | Tray icon, account picker, DND/auto answer, Quit. | **No tray on stock GNOME.** See below. |
| `org.kde.notification` + `megasip.notifyrc` | Incoming call (persistent, Answer/Decline), missed call. | `org.freedesktop.Notifications` over D-Bus. See below. |
| `IncomingWindow.qml` | Always-on-top ring dialog that raises itself. | Not possible on GNOME Wayland. Notification + in-window banner. |
| `PreferencesWindow.qml` (941 lines) | Categories, inline account editor, Advanced fold. | `AdwPreferencesDialog` with pages, `AdwEntryRow`, `AdwPasswordEntryRow`, `AdwComboRow`, `AdwSwitchRow`, `AdwExpanderRow` for Advanced. A good fit, but the largest single piece of UI work. |
| `DebugWindow.qml`, `CallLadder.qml`, `CallQuality.qml`, `Sparkline.qml` | SIP trace list, sequence diagram drawn on a Canvas, live graphs. | `gtk::ListView` + `gio::ListStore`; ladder and sparklines as `gtk::DrawingArea` with cairo (or a custom widget `snapshot`). The ladder is the fiddliest port. |
| `Dialogs.FileDialog` | Contact import/export, ringtone, save trace. | `gtk::FileDialog` (GTK 4.10+), which goes through the file chooser portal. |
| KF6I18n | Linked and initialised, but **no QML file calls `i18n()`**. | Nothing to port today. If translation is ever wanted, gettext on the GTK side. |
| `.desktop` file | `Categories=...;Qt;KDE;`, `X-KDE-Shortcuts`. | A second desktop file and app id for the GTK build, plus AppStream metainfo. |
| Default ringtone | Tries Plasma's Ocean sound first, then freedesktop, then a synthesised ring. | Already falls back to the freedesktop theme, which GNOME ships. No change needed. |
| Autostart | Writes `~/.config/autostart/megasip.desktop`, already with `X-GNOME-Autostart-enabled=true`. | Works as is outside Flatpak. Inside Flatpak, use the Background portal's autostart request. |

### The logic hiding in `bridge.rs`

`bridge.rs` is not just glue. Roughly 60% of it is the "window controller" that any frontend
needs, and today it is tangled with `Pin<&mut Self>`, `qt_thread().queue(...)` and `QString`:

- account activation and deactivation against the lock table (`activate`, `deactivate_account`, `drop_vanished_account`)
- start-up requests: dial once registered or after 10 s, hand the number to the holder if another window won the race (`start_up`, `flush_pending_dial`)
- IPC line handling (`dial`, `answer`, `hangup`, `quit`, `show`, `tel:`/`callto:` stripping)
- engine event handling: contact-name lookup, history save, missed count, conference reset
- settings save with three-way merge against what other windows saved, validation, re-configure, logging apply
- external reload when another window saves settings or contacts
- tray account picking: activate here, raise the holder, or spawn `megasip -a <id>`
- status text, last-number tracking, contact import/export, autostart

If a GTK frontend is written without extracting this first, it will be copied, and the two
copies will drift. Extracting it is the single most important step in this plan, and it
pays off for the Qt version even if the GTK version is never finished.

## The GNOME Wayland integration problems

These are the risks that matter most, because none of them have a clean answer.

### 1. No system tray

GNOME Shell has no tray. StatusNotifierItem icons only appear with the AppIndicator
extension, which Ubuntu enables by default and Fedora and stock GNOME do not. MegaSIP's KDE
design leans on the tray: closing the window keeps it running "in the tray", and the tray is
the main account picker.

What to do instead:

- **Make the window the primary surface.** The account picker already exists in the window
  header; DND, auto answer and redial move to the primary (hamburger) menu.
- **Closing the window:** on GNOME the expected behaviour is that a background app keeps
  running and can be found again by launching it. `megasip` with no args already raises an
  idle window over IPC, and `megasip -a work` already raises the holder, so relaunching from
  the app grid does the right thing *if* raising works (see 2).
- **Offer an SNI tray when one exists.** The `ksni` crate is pure Rust over zbus and talks
  StatusNotifierItem directly. If it lives in the shared integration crate, the Qt build can
  use it too and drop `Qt.labs.platform`. Detect the watcher (`org.kde.StatusNotifierWatcher`
  on the bus) and fall back to "closing the window quits" only if the user opts in.
- **Background portal:** `org.freedesktop.portal.Background` lets an app ask to keep running
  without windows (and to autostart). On GNOME the "Background Apps" list in Quick Settings
  is built from this, but as far as I can tell it reliably covers Flatpak apps only, and I
  found a recent Fedora bug report about the list missing in GNOME 50. Treat it as nice to
  have, not as the tray replacement.

**Risk:** a softphone that hides with no visible indicator is easy to forget is running,
which for a phone means "why did my laptop ring". A persistent indicator is worth more here
than in most apps. Option worth testing: while an account is registered and no window is
shown, keep a low-priority persistent notification ("MegaSIP: work is online"). GNOME may
treat that as noise; try it before promising it.

### 2. Windows cannot raise themselves

Mutter will not let a window take focus without an xdg-activation token. `present()` without
one gives a "MegaSIP is ready" notification instead of a raise. This affects:

- `bring_to_front` on incoming calls: **will not work** on GNOME Wayland. The notification
  has to carry the call.
- `megasip -a work` when another window holds `work`, `megasip 1002`, `tel:` links: these
  *can* work, because the launcher (shell, browser, terminal via a shortcut) gives the new
  process an `XDG_ACTIVATION_TOKEN`. Today the IPC sends `show`; it should send the token
  along (`show <token>`), and the receiving GTK window calls `set_startup_id(token)` before
  `present()`. This is a small, contained change to `ipc.rs` and `main.rs`, and Qt can use
  the same token, so it belongs in the shared phase.
- Always-on-top (`settings.alwaysOnTop`, the incoming window's `WindowStaysOnTopHint`): GTK4
  has no API for it on Wayland. Hide the setting in the GNOME build.

### 3. Notifications and the multi-process design

MegaSIP is deliberately **one process per account window** (chosen over one process with
several windows, so one crash doesn't take down every line). GTK and GNOME's application
model assume the opposite: `GApplication` is single-instance by default, and `GNotification`
delivers action clicks by activating the application's D-Bus name. With
`G_APPLICATION_NON_UNIQUE` (needed to keep separate processes), no process owns that name,
so Answer/Decline on a `GNotification` would not reach the window that is ringing.

What to do: send notifications with `org.freedesktop.Notifications` directly (via `zbus`
or the `notify-rust` crate), which GNOME Shell supports, including actions, and which
delivers `ActionInvoked` to the connection that sent the notification, so each process gets
its own clicks. This also lives in the shared integration crate and can replace
KNotification in the Qt build, which removes `megasip.notifyrc`.

Things to verify early with a 50-line prototype, because I have not tested them:

- GNOME Shell shows actions on a critical-urgency notification and keeps it until dismissed.
- A click on the notification body brings an activation token through
  `ActivationToken` (spec 1.2) so the window can actually raise.
- Behaviour when two windows ring at once.

**Keeping multi-process is the right call** even on GNOME, because the lock table, IPC and
CLI semantics are already built around it and would be shared. But it means giving up some
of `GApplication`'s conveniences (D-Bus activation, `app.open` for `tel:` links); MegaSIP's
own IPC already covers those.

### 4. Global shortcuts

The KDE build exposes Answer and Hang Up as desktop actions, which Plasma can bind globally.
On GNOME there are two routes:

- **Works today, no code:** Settings → Keyboard → Custom Shortcuts → command
  `megasip --answer` / `megasip --hangup`. The IPC already makes this correct across all
  windows. Document it.
- **GlobalShortcuts portal:** GNOME 48 added it to xdg-desktop-portal-gnome (`ashpd` has a
  Rust client). It asks the user to confirm bindings in a system dialog. Nice to have; it is
  newer, so older LTS distros won't have it. Phase 5 at the earliest.

### 5. Flatpak breaks the PID-based design

If the GNOME version is distributed as a Flatpak (the usual route for GNOME apps), each
`flatpak run` gets its own PID namespace. The lock table's stale-lock check (`/proc/<pid>`
plus start time) and the socket name `megasip-<pid>.sock` both assume PIDs are global, and
separate sandboxes can easily both be PID 2. That would silently break "one account per
window".

Fix before any Flatpak: replace PIDs as instance identity with a random instance id, and
detect liveness with an `flock` held on a per-instance file (the kernel drops it on crash),
keeping PID only for display. This is a core change and benefits the native build too.
Other Flatpak items are routine: `--share=network`, `--socket=pulseaudio`, the Secret portal
(the `oo7` crate handles both portal and host Secret Service), the file chooser portal
(free with `gtk::FileDialog`), and `x-scheme-handler/sip`/`tel` in the desktop file.

The alternative is to skip Flatpak at first and ship the GNOME build the way the Qt one ships
today (`install.sh`, later a distro package). That defers this risk rather than removing it.

## Toolkit options

### Option A: keep Qt, make it behave on GNOME (no new UI)

Do the shared integration work (SNI tray via `ksni` with a no-tray fallback, fdo
notifications, activation tokens, shortcuts docs) and drop the hard KDE runtime deps where
possible. Kirigami runs fine on GNOME but looks like a KDE app, and the `org.kde.desktop`
style falls back to Fusion-like controls without Plasma.

- Cost: 2 to 3 weeks (phases 0 to 2 below).
- One UI to maintain.
- Honest downside: GNOME users will see a foreign-looking app, and libadwaita conventions
  (header bar, toasts, preferences dialog) will be missing.

### Option B: shared core + GTK4/libadwaita frontend (recommended if the native look matters)

Two frontends over one core. Details below.

### Option C: replace Qt with GTK everywhere

Not recommended. libadwaita apps ignore desktop themes, so on Plasma MegaSIP would look like
a GNOME app, which reverses the project's original reason for choosing Kirigami.

### gtk-rs directly vs relm4

Both sit on `gtk4-rs` and `libadwaita-rs`; relm4 is a framework on top.

- **relm4:** Elm-style components (model, `update(msg)`, `view!` macro), factories for lists.
  MegaSIP's current architecture is already state-down/commands-up (JSON properties down,
  invokables up), so the controller maps onto relm4 almost one-to-one: core state becomes
  the model, core events become messages. Raw gtk-rs is still available for the custom
  widgets (ladder, sparklines).
  - Risks: relm4 releases trail gtk4-rs, the `view!` macro gives poor error messages, and it
    is a smaller community than gtk-rs itself.
- **gtk-rs + Blueprint/`.ui` templates:** what GNOME's own docs and examples use, with
  better tooling (Workbench), and subclassing for custom widgets. More boilerplate: GObject
  subclassing in Rust is verbose, and you wire signals by hand.

My pick is relm4 because the shape matches, but the gap is small, and if the first relm4
spike fights you on the Preferences dialog, switching to gtk-rs early is cheap. Decide after
Phase 2's spike, not now.

Minimum versions to target: GTK 4.10 (`FileDialog`) and libadwaita 1.5
(`AdwPreferencesDialog`, `AdwDialog`). Ubuntu 24.04 ships libadwaita 1.5, so it is a
reasonable floor; check exact distro versions when starting.

## Proposed structure

```
Cargo.toml                 workspace
crates/
  megasip-core/            engine, media, config, db, ipc, secrets, logging, debug, store,
                           cli (headless), args (parse + hand off to running windows),
                           controller (the logic now in bridge.rs)
  megasip-desktop/         freedesktop integration shared by both UIs:
                           notifications (zbus), SNI tray (ksni), activation tokens,
                           autostart, optional GlobalShortcuts (ashpd)
  megasip-qt/              cxx-qt bridge (thin), cpp/, qml/, build.rs -> binary `megasip`
  megasip-gtk/             relm4/libadwaita frontend          -> binary `megasip-gnome`
tests/interop/             unchanged: drives `megasip --cli`, which lives in core
```

### The controller API

The controller owns what `PhoneRust` owns today (settings, contacts, history, calls,
accounts, active account, db connection, engine command sender, audio hub) and exposes typed
state instead of JSON:

```rust
pub struct Controller { /* ... */ }

pub enum Update {
    Accounts, Calls, History, Contacts, Settings, Status, ActiveAccount, Flags,
    Incoming { id: u64, name: String, number: String },
    Missed { name: String, number: String },
    Error(String),
    Activate { token: Option<String> },
    Dtmf { id: u64, digit: char },
}

impl Controller {
    pub fn new(start: StartRequest, notify: impl Fn(Update) + Send + 'static) -> Self;
    pub fn on_engine_event(&mut self, ev: engine::Event);
    pub fn on_ipc(&mut self, line: &str);
    pub fn dial(&mut self, number: &str, account: &str);
    pub fn save_settings(&mut self, base: Settings, draft: Settings) -> Result<(), String>;
    pub fn pick_account(&mut self, id: &str);
    // ... one method per current invokable
    pub fn calls(&self) -> impl Iterator<Item = &CallInfo>;
    // ... typed getters
}
```

Threading stays as now: the engine runs on its tokio thread; its events and IPC lines are
forwarded onto the UI thread (Qt: `qt_thread().queue`; GTK: an `async_channel` drained by
`glib::MainContext::spawn_local`), and only then call into the controller. So the controller
is single-threaded and needs no locking. Secret Service calls keep running off the tokio
runtime, as `secrets.rs` already requires. The 10 s "dial anyway" timer and the 1.5 s
startup dial become controller-requested timers that each frontend schedules
(`QTimer`/`glib::timeout_add_local_once`), or a small timer thread that posts back through the
same channel.

The Qt bridge then shrinks to: hold a `Controller`, turn each `Update` into the matching
JSON property set or signal, forward invokables. The QML does not change.

## Phased plan

Estimates assume one developer working with Claude, full days, and include tests and the
interop suite passing at the end of every phase. They are rough; the UI phases are the
least certain.

| Phase | Work | Estimate | Ships something? |
|---|---|---|---|
| 0 | **Workspace split.** Move modules into `megasip-core` unchanged; Qt binary builds from `megasip-qt`. No behaviour change. | 1 to 2 days | Same app, new layout |
| 1 | **Extract the controller** from `bridge.rs` with unit tests for activation, start-up dial, IPC, settings merge (most of this logic has no tests today). Qt bridge becomes a thin adapter. | 4 to 6 days | Same app, better tested |
| 2 | **Shared desktop integration**: fdo notifications with actions (replaces KNotification), activation-token forwarding over IPC, `ksni` tray with no-tray fallback, Custom Shortcuts docs. Includes the notification prototype from risk 3. Ends with Option A done: the Qt app behaves well on GNOME. | 5 to 8 days | Qt app works on GNOME |
| Gate | **Decision point:** is Option A enough? If yes, stop here. | | |
| 3 | **GTK spike, then skeleton**: relm4 vs gtk-rs spike (2 days), then `AdwApplicationWindow`, view switcher, dial pad, calls panel with hold/mute/transfer/conference, status, account picker, incoming via notification + banner, error toasts. Makes calls end to end. | 6 to 9 days | Usable GNOME phone |
| 4 | **Contacts, history, Preferences** (`AdwPreferencesDialog`, account editor with Advanced, codec order, devices, ringtone, logging). | 7 to 10 days | Feature parity minus debug |
| 5 | **Debug window**: SIP trace list with search/copy/save, call ladder on cairo, call quality graphs, logs view. GlobalShortcuts portal if wanted. | 4 to 7 days | Full parity |
| 6 | **Packaging**: desktop file and app id for the GTK build, AppStream metainfo, icon. Optional Flatpak, which first needs the PID-to-instance-id change from risk 5 (2 to 3 days on its own). | 3 to 6 days | Installable |

Totals: Option A (phases 0 to 2) is about **2 to 3 weeks**. Full Option B is about
**6 to 10 weeks**. I would plan on the high end, because UI parity work reliably takes
longer than the QML line count suggests.

## Tradeoffs and risks, candidly

- **Two UIs forever.** This is the biggest cost and it never goes away. The last two feature
  PRs (account windows, short names) each touched `bridge.rs` and several QML files. Under
  this plan they would touch the core once and each frontend once. Features that are mostly
  UI (a new Preferences field, a new debug tab) roughly double in cost. If MegaSIP is mainly
  for one person on Plasma, Option A is probably the honest answer.
- **The GNOME experience will be different, not just restyled.** No tray, no self-raise on
  incoming calls, no always-on-top. The notification becomes the ringing UI. Users coming
  from MicroSIP on Windows may find that weaker, and there is no workaround within GNOME's
  rules.
- **Notification behaviour is the critical unknown.** If GNOME Shell does not keep a critical
  notification with Answer/Decline visible reliably, the GNOME build has no dependable way
  to present an incoming call while the window is hidden. That is why the prototype is in
  phase 2, before any GTK work.
- **Multi-process is against the grain of GTK.** It works with `NON_UNIQUE`, but every
  GNOME tutorial assumes single-instance, so expect to rediscover small things (desktop
  file activation, `app.quit` semantics) the hard way.
- **Flatpak needs a core change.** Shipping on Flathub is the natural GNOME distribution
  route, and it breaks the PID-based lock design. Not hard to fix, but not optional.
- **Phase 1 is valuable regardless.** Extracting the controller gives the window logic its
  first unit tests and makes `bridge.rs` small. If only one phase of this plan happens,
  it should be that one.

## Open questions for Tim

1. Who is the GNOME version for: yourself on a second machine, or other people? That decides
   between Option A and B, and whether Flatpak matters.
2. Is losing tray-based account picking acceptable on GNOME, given the window has the same
   picker?
3. Binary and app id for the GTK build: `megasip-gnome` / `org.megasip.MegaSIP.Gnome`, or one
   app id with the two builds being alternatives (not installable side by side)?
