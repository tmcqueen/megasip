# MegaSIP

A voice-only SIP softphone for Linux desktops (made on KDE Plasma, Wayland), with Windows
on the way: a Rust port of
[MicroSIP](https://www.microsip.org/) (GPLv2), "MicroSIP-Lite". It behaves like MicroSIP
except for settings, which are one Preferences window with the account editor built in.

- Pure-Rust SIP and media: [`rsiprtp`](https://crates.io/crates/rsiprtp) for SIP parsing,
  digest auth, SDP, RTP, G.711 / G.722 / Opus, DTMF and SRTP; the user-agent logic
  (transactions, registration, dialogs, hold, transfer, conference) is ours.
- Echo cancellation and noise suppression: [`sonora`](https://crates.io/crates/sonora),
  a pure-Rust port of WebRTC's audio processing (AEC3).
- Sound through PipeWire/PulseAudio (`cpal`), opened only while you're on a call, so
  Plasma's microphone indicator tells the truth.
- Qt 6 / QML UI through `cxx-qt`, with no KDE Frameworks needed, so the same UI builds on
  Windows. On Plasma it uses the desktop's own style and KDE notifications when they're
  installed; elsewhere, Qt's Fusion style (Fluent on Windows) and tray notifications.

## Features

Multiple accounts (UDP, TCP, TLS; DNS SRV; outbound proxy; NAT detection via rport and
optional STUN) · dial pad with DTMF (RFC 2833, SIP INFO or in-band) · several calls at
once with hold/resume, mute, blind and attended transfer, and a local conference ·
call history (missed / incoming / outgoing) · contacts (imports MicroSIP's
`Contacts.xml` or CSV) · auto answer (all calls, or when the server asks) · do not
disturb · call forwarding (always / busy / no answer) · call waiting · single-call mode · call length limit
· deny incoming · SRTP (SDES, optional or mandatory) · ringtone on its own device ·
redial · voicemail button · `tel:` / `sip:` / `callto:` links · start at login.

Not included: video, chat/messaging, presence/BLF, call recording, and MicroSIP's
Windows-only extras.

## Build and install

```sh
sudo apt install cargo build-essential pkg-config clang \
    qt6-base-dev qt6-declarative-dev libasound2-dev qt6-svg-plugins \
    qml6-module-qtquick-controls qml6-module-qtquick-layouts qml6-module-qtquick-dialogs \
    qml6-module-qtquick-effects qml6-module-qtqml-models qml6-module-qt-labs-platform
# Optional, on Plasma: its style and its notifications (with Answer / Decline)
sudo apt install qml6-module-org-kde-qqc2desktopstyle qml6-module-org-kde-notifications
sudo apt install lld   # optional: faster linking, and silences cxx-qt's linker warning
./install.sh          # builds, then installs into ~/.local (no sudo)
megasip
```

`install.sh` also installs the desktop entry, icon and notification config, and
registers MegaSIP for `sip:`/`tel:` links where no other app handles them.

The UI bundles the [Breeze](https://invent.kde.org/frameworks/breeze-icons) icons it uses
(LGPL-3.0-or-later; `crates/megasip-qt/icons`). They are the icon theme where the desktop has
none (Windows), and fill in names the desktop's theme lacks.

### Windows

The core builds and passes its tests on Windows. CI builds the Qt app there with the
official Qt 6.10 (MSVC), packages it with `windeployqt`, and checks the package by running it
with Qt off `PATH` and screenshotting every screen.

**To get a build:** open the latest run of the CI workflow under the repository's Actions
tab and download the `MegaSIP-<version>-windows-x64` artifact. Unzip it anywhere and run
`megasip.exe`; nothing needs installing (the Visual C++ runtime is included). Releases
carry the same zip (see [Releases](#releases)). There is no installer yet, so
there's no Start menu entry, `sip:`/`tel:` link handling or start at login on Windows.

Building on Linux for Windows isn't practical: cxx-qt runs the target Qt's `qmake`, and a
Windows Qt only has `qmake.exe`. `megasip-core` alone does cross-compile:
`cargo build -p megasip-core --target x86_64-pc-windows-gnu` (with `gcc-mingw-w64-x86-64`).
To build the app on Windows yourself, install Rust and Qt 6.10 for MSVC (with Qt SVG), put
Qt's `bin` on `PATH`, and run `cargo build --release -p megasip-qt`; `windeployqt` gathers
the DLLs next to `target\release\megasip.exe`.

### Releases

Each [GitHub release](https://github.com/tmcqueen/megasip/releases) carries the Windows zip
and, for both x86_64 and aarch64 (arm64) Linux, a `.deb`, an `.rpm` and a `.tgz`. The
`.deb` and `.tgz` are built on Debian 13 and run on Debian 13 or newer and Ubuntu 26.04 or
newer; the `.rpm` is built on Fedora 43, for Fedora 43 (CI installs and starts each one on
those systems). The `.tgz` holds the binary and an `install.sh` that installs it into
`~/.local` (or `/usr/local` with `sudo`); it needs Qt installed by hand, which its
`README.md` lists.

The Release workflow (`.github/workflows/release.yml`) builds them when a `v*` tag is
pushed and attaches them to that tag's release, creating it if needed. To build an
existing tag again, for example when its files are missing, open Actions > Release > Run
workflow and give it the tag; files of the same name on the release are replaced.

### Packaging: .deb and .rpm only, no Flatpak or Snap

MegaSIP is packaged as `.deb` (Debian, Ubuntu) and `.rpm` (Fedora), plus a plain `.tgz`,
not as a Flatpak or Snap. The reasons:

- **One account per window depends on process IDs.** Each window is its own process. The
  account locks in `megasip.db` and the IPC sockets (`megasip-<pid>.sock`) tell windows
  apart by PID, and spot a crashed window by checking `/proc/<pid>`. Flatpak starts every
  launch in its own PID namespace, so two windows could both be PID 2 and could not see
  each other in `/proc`. Locks would go wrong without any error. Fixing that means
  replacing PIDs with instance ids and file locks throughout, for no gain on a native
  install.
- **A sandbox changes how the desktop features work.** Passwords, the tray, notifications,
  autostart, file dialogs and `sip:`/`tel:` links would each go through a portal or a
  sandbox permission instead of the plain freedesktop D-Bus services MegaSIP uses now. Each
  of those is behaviour to re-test on both Plasma and GNOME, and a softphone that misses a
  ring because of a permission is worse than one that is harder to install.
- **Native packages use the system's Qt.** That is what gives MegaSIP the desktop's style
  and theme (Plasma's when it's installed). A Flatpak would bring its own Qt runtime, and a
  Snap would need content snaps; both are large downloads for a small app.
- **The target distributions already ship everything needed.** MegaSIP needs Qt 6.5 or
  newer (CI uses 6.10): Fedora 39+, Debian 13 and Ubuntu 24.10+ have it; Ubuntu 24.04 LTS (Qt 6.4) does
  not.

If MegaSIP is ever meant for a wider audience, Flatpak is the route to revisit, starting
with the PID change above.

## Use

| | |
|---|---|
| Type a number, Enter | Call (numbers, `user@domain`, `sip:` URIs, or an IP address for a direct call) |
| Call button with an empty field | Recalls the last number (press again to dial) |
| Keys during a call | Send DTMF |
| Esc on the dial pad | Hang up |
| Close the window | Keeps running in the tray (Quit from the tray menu or Ctrl+Q) |
| Ctrl+1 … Ctrl+3 | Dial pad, contacts, calls |
| Ctrl+, | Preferences |

## One account per window

Each MegaSIP window has at most one account active, and an account can be active in only
one window at a time. A plain `megasip` opens a window with nothing registered. Pick an
account from the tray menu or the account button at the top of the window:

- In a window with no account, the account is activated there.
- In a window that already has one, it opens in a new window.
- Accounts active in another window are greyed out.
- **Deactivate** gives the window's account up again.

Each account has a **display name**, shown in MegaSIP ("Telnyx - Tim McQueen"), and a
**short name** for the command line (`megasip -a telnyx-tim`). Short names use letters,
digits and `- _ . @`, and no two accounts can share one. Accounts without a short name get
one made from their display name.

Each window's **Calls** tab shows only its own account's history. Its logs go to
`sip-<account>.log` and `audio-<account>.log`.

### Telling account windows apart

An account can have its own **icon** (Preferences → Accounts → Icon): a phone, headset,
home, briefcase, building, person, star, globe or the first letter of its name, in white
on a badge of any color. While the account is active, its window shows that icon in the
taskbar and the tray. The tray fades it while the account isn't registered and still
switches to the missed-call icon when there is one. Accounts without an icon look like
MegaSIP.

On Wayland the taskbar (Plasma's and GNOME's) takes a window's icon from the `.desktop` file
that matches the window's app_id, not from the window. So a window whose account has an
icon uses its own app_id, `org.megasip.MegaSIP.acct_<id>`, and MegaSIP writes a hidden
`.desktop` file of that name to `~/.local/share/applications/` with the badge from
`~/.local/share/megasip/icons/`. MegaSIP removes those files again when the icon is turned
off or the account deleted. Two things follow from it:

- Those windows are no longer grouped with MegaSIP's own launcher in the taskbar. A
  launcher pinned from one of them opens that account (`megasip -a <account>`).
- A window can only change its app_id by being shown again, so when its account's icon
  changes (or an idle window activates an account with an icon), it closes and reopens
  for a moment.

From the command line:

```sh
megasip                         # a window with no account active (or the idle one, raised)
megasip -a work                 # a window with the account whose short name is "work";
                                # if another window has it, that window comes to the front
megasip -a work 1002            # ...and call 1002 once it's registered (from that window,
                                # if one already has "work")
megasip 1002                    # call from the window with the default account (or sip:…, tel:…)
megasip --answer                # answer the ringing call
megasip --hangup                # hang up all calls, in every window
megasip --quit                  # quit every window
megasip --minimized
```

### Sending a file or test signal instead of the microphone

`--source` sends something else in place of the microphone, for every call in that
window. With `-a`, if another window already has that account, that window makes the call with
the source and goes back to what it was sending (usually the microphone) once the call
ends. It won't switch while a call is up, since the source would go out on that call too.
Otherwise it opens a new window, which keeps the source for all its calls.

```sh
megasip -a work --source greeting.mp3 1002   # .wav, .ogg or .mp3, looped from the top of each call
megasip -a work --source milli               # 1004 Hz test tone at 0 dBm0
megasip -a work --source pink                # pink noise at -20 dBFS (also: white)
megasip -a work --source tone:440            # a sine at any frequency
```

The window shows **Source: …** next to the account, the call panel's level meter is
labelled with it, and the microphone choice in Preferences is disabled. Echo cancellation,
noise suppression and microphone gain don't touch it. Files are decoded up front, and
anything past 10 minutes is cut.

### Headless

`megasip --cli -a <account> [--source …]` runs one account with no window: commands on
stdin, events as JSON lines on stdout (see `crates/megasip-core/src/cli.rs`). It refuses to start (exit code 2)
without `-a`, or if the account is active elsewhere. Its calls are saved to that account's
history.

### AI agents (MCP)

`megasip --mcp -a <account> [--source …]` is an [MCP](https://modelcontextprotocol.io)
server on stdin/stdout, so an AI agent (Claude Code, Claude Desktop, or any MCP client) can
run VoIP tests for you. If a MegaSIP window already has that account, the agent drives that
window: its calls show up there and you can watch, answer or take over. Otherwise it runs
the account itself, with no window. For Claude Code:

```sh
claude mcp add megasip -- megasip --mcp -a <account>
```

The agent gets these tools:

| Tool | What it does |
|------|--------------|
| `status` | registration, calls in progress, mute, conference, what's sent instead of the mic |
| `dial` | place a call, optionally with a test signal for that call only; returns the call id |
| `answer`, `reject`, `hangup`, `hangup_all` | the obvious |
| `hold`, `mute`, `conference` | on or off |
| `send_dtmf` | digits, with a gap between them |
| `transfer`, `attended_transfer` | blind (REFER) and attended (REFER with Replaces) |
| `set_source` | switch to `milli`, `pink`, `white`, `tone:<hz>`, a sound file, or back to the mic |
| `wait_for` | block until registered, incoming, ringing, connected, ended, or DTMF received |
| `events` | everything that happened since a sequence number |
| `call_quality_report` | during a call, a spot report; after it, the final one (see below) |
| `sip_trace` | the SIP messages for one call or all of them (digest responses hidden) |
| `call_history` | this account's history |
| `get_settings`, `update_settings` | read and change settings and accounts |

A **call quality report** has the MOS and R-factor, loss, jitter, late and duplicate
packets, buffer underruns, SRTP failures, packet and byte counts, a timeline in 5-second
windows (so a burst of loss shows up even when the call's average looks fine), and
findings in plain words ("No RTP was received: one-way audio"). During a call it also has
the live mic and speaker levels and the dominant frequency coming back, so "dial the echo
test with `milli` and check I hear 1004 Hz" is something an agent can check. The MOS is a
simplified ITU-T G.107 E-model with a nominal network delay (there's no RTCP round trip
yet): good for comparing calls, not a certified score. Reports for the last 50 calls are
kept until the server exits.

**What the agent can't do.** Account passwords read back as `<redacted>` and can't be
set or changed over MCP; set them in Preferences. It can't delete the account it runs as.
Emergency numbers (911, 112, 999, 000, 110, 119, also behind a one-digit outside-line
prefix) are refused; 933, the US address readback line, is allowed. To fence it in
further, `--mcp-allow <prefix>` (repeatable) only lets it call numbers or URIs that start
with one of the prefixes. Your MCP client still asks before each tool call unless you
tell it not to.

Stdout is the MCP channel, so `--debug` can't be used with `--mcp`; set
`MEGASIP_LOG=debug` for logs on stderr. When the client disconnects, MegaSIP hangs up,
un-registers and exits; a window it was driving just carries on. A window takes one
agent at a time; a second one waits until the first disconnects.

## Preferences

Preferences open in their own window: the gear in the status bar, **Preferences…** in
the tray menu, or Ctrl+,. Categories sit on the left:

- **Accounts:** your accounts, and the selected one's details. Server, username, password
  and display name are up front; domain, login, proxy, transport, SRTP, public address,
  timers, voicemail number, dialing prefix and hide caller ID are under **Advanced**.
- **Audio:** devices, ringtone, voice processing, codec order.
- **Calls:** auto answer, call waiting, DTMF method, auto hang-up.
- **Availability:** do not disturb, call forwarding, deny incoming.
- **General** and **Network**.

Changes apply when you press **Save**, so accounts re-register once and not on every
keystroke. On first run with no accounts, Preferences opens on Accounts.

Files:

- `~/.config/megasip/settings.json` holds settings and accounts (owner-only), but no
  passwords. SIP passwords are kept in KWallet (any Secret Service wallet works) as
  "MegaSIP: <account>". Passwords that older versions left in the file are moved into the
  wallet the next time MegaSIP starts. If the wallet can't be reached, Save fails and
  shows the error; nothing is written in plain text.
- `~/.local/share/megasip/megasip.db` (SQLite) holds call history and which window has which
  account active. A window that crashes leaves its account free for the next one.
- `~/.local/share/megasip/` also holds contacts and the optional logs.

## Debug window

Ctrl+Shift+D, **Debug window…** in the tray menu, **Open debug window** under Preferences →
Logging, or the ⓘ button on a call. It has four tabs:

- **SIP trace:** every SIP message sent and received (the last 2,000 are kept even
  while the window is closed), with search, a full-message view, copy and save.
- **Call ladder:** pick a call (the newest is shown by default) and see its sequence
  diagram, with the time between messages. Click an arrow to read that message.
- **Call quality:** live per-call figures, with the last minute graphed.
  - Codec and SRTP.
  - Packets and bytes each way.
  - Loss, jitter and playout buffer depth.
  - Concealed packets, late and duplicate packets, underruns and drift corrections.
  - SRTP failures and DTMF.
  - An estimated MOS (simplified E-model).

  The last call's figures stay after it ends.
- **Logs:** recent log events, filtered by SIP/audio, level and text. Choosing Debug
  captures more detail from then on.

## Logging

SIP and audio are logged separately, so PulseAudio chatter doesn't bury the SIP trace.
Set it up under Preferences → Logging:

- **Levels:** one for SIP/calls and one for audio. Debug on SIP shows every SIP message.
- **Log files:** `~/.local/share/megasip/logs/sip.log` and `audio.log`. Each starts over
  after 20 MB, keeping the previous one as `*.log.old`.
- **[Venator](https://crates.io/crates/venator-app):** sends both logs to the viewer
  (default `127.0.0.1:8362`), where you filter by target: `megasip::engine` for SIP,
  `pulseaudio`, `cpal` or `megasip::media` for audio. Start Venator first. While it isn't
  running, MegaSIP retries every 5 s, which costs up to 100 ms on the logging thread, so
  leave it off when you're not using it.

Changes apply as soon as you save, without a restart.

On the terminal, `MEGASIP_LOG=debug megasip` prints the SIP trace. Audio is limited to
warnings unless you set `MEGASIP_AUDIO_LOG=info` (or `debug`).

`megasip --debug` logs at debug level to stdout instead: what the command line found in
settings and `megasip.db`, which window it hands a request to and over which socket, and
what that window did with it. Run both the window and the second command with `--debug`
to follow a hand-off from end to end. Audio libraries stay at info there.

## Tests

```sh
cargo test                 # unit tests
tests/interop/run.sh       # end-to-end against a private Asterisk on 127.0.0.1:5070
```

The interop suite starts its own Asterisk (the system service can stay off) and four
headless instances (`megasip --cli`). Each instance feeds its own test tone into the
call instead of using a microphone. The suite checks:

- registration
- calls over Opus, G.722, G.711 and SRTP
- that each side hears the right tone
- hold and resume
- DTMF
- decline, cancel and busy
- blind and attended transfer
- conference

`megasip --cli` is also handy for scripting: commands on stdin, events as JSON lines.

## License

GPL-2.0-or-later, like MicroSIP. See `LICENSE`.
