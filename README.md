# MegaSIP

A voice-only SIP softphone for KDE Plasma on Wayland: a Rust port of
[MicroSIP](https://www.microsip.org/) (GPLv2), "MicroSIP-Lite". It behaves like MicroSIP
except for settings, which are one Preferences window with the account editor built in.

- Pure-Rust SIP and media: [`rsiprtp`](https://crates.io/crates/rsiprtp) for SIP parsing,
  digest auth, SDP, RTP, G.711 / G.722 / Opus, DTMF and SRTP; the user-agent logic
  (transactions, registration, dialogs, hold, transfer, conference) is ours.
- Echo cancellation and noise suppression: [`sonora`](https://crates.io/crates/sonora),
  a pure-Rust port of WebRTC's audio processing (AEC3).
- Sound through PipeWire/PulseAudio (`cpal`), opened only while you're on a call, so
  Plasma's microphone indicator tells the truth.
- Kirigami UI (Qt 6 / QML through `cxx-qt`), system tray, KDE notifications.

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
    qt6-base-dev qt6-declarative-dev libkf6i18n-dev libasound2-dev \
    qml6-module-org-kde-kirigami qml6-module-org-kde-kirigamiaddons-formcard \
    qml6-module-org-kde-kirigamiaddons-components qml6-module-org-kde-notifications \
    qml6-module-qt-labs-platform
sudo apt install lld   # optional: faster linking, and silences cxx-qt's linker warning
./install.sh          # builds, then installs into ~/.local (no sudo)
megasip
```

`install.sh` also installs the desktop entry, icon and notification config, and
registers MegaSIP for `sip:`/`tel:` links where no other app handles them.

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

Each window's **Calls** tab shows only its own account's history. Its logs go to
`sip-<account>.log` and `audio-<account>.log`.

From the command line:

```sh
megasip                         # a window with no account active (or the idle one, raised)
megasip -a work                 # a window with the account "work" active (label, id or user@domain);
                                # if another window has it, that window comes to the front
megasip -a work 1002            # ...and call 1002 once it's registered
megasip 1002                    # call from the window with the default account (or sip:…, tel:…)
megasip --answer                # answer the ringing call
megasip --hangup                # hang up all calls, in every window
megasip --quit                  # quit every window
megasip --minimized
```

### Sending a file or test signal instead of the microphone

`--source` sends something else in place of the microphone, for every call in that
window. It always opens a new window.

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
stdin, events as JSON lines on stdout (see `src/cli.rs`). It refuses to start (exit code 2)
without `-a`, or if the account is active elsewhere. Its calls are saved to that account's
history.

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
