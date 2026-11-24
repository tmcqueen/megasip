# MegaSIP — design

A Rust port of MicroSIP (GPLv2, `~/projects/MicroSIP`) for KDE Plasma on Wayland.
Voice only ("MicroSIP-Lite"). Same behaviour as MicroSIP everywhere except settings,
which become one simplified view that includes the account (profile) editor.

## Why

MicroSIP is the right SIP softphone (small, fast, no nonsense) but only runs on Windows.

## Goals

- Native KDE look: Kirigami/QML UI, system tray, desktop notifications, `tel:`/`sip:` links.
- MicroSIP feature parity for voice: multiple accounts, dialer with DTMF, several calls
  at once, hold/resume, mute, blind and attended transfer, conference of active calls,
  redial, call history (missed/in/out), contacts, auto-answer, DND, call forwarding,
  deny incoming, call waiting, ringtone + ring device, echo cancellation, SRTP (SDES),
  UDP/TCP/TLS, STUN, DNS SRV.
- One settings page; accounts are edited inline, advanced fields folded away.

## Non-goals

Video, messaging/chat, presence/BLF/MWI, call recording, Windows-only extras (jump lists,
port knocker, headset HID, update checker, crash reports, per-event command hooks),
language packs.

## Approaches (decided)

- **SIP/media engine:** pure-Rust `rsiprtp` 0.4.1 (pinned) for SIP parsing/building, digest
  auth, SDP parsing, RTP packets, G.711/G.722/Opus, jitter buffer, DTMF (RFC 4733), SRTP and
  DNS resolution. The UA logic (transactions, registration, dialogs, re-INVITE, REFER,
  media plumbing) is ours. Chosen by the user over rsipstack (signaling only) and over
  pjsip (C build). Kept behind our own `engine` module so it can be swapped.
- **UI:** cxx-qt 0.10 + Kirigami QML. A small C++ stub creates a `QApplication` so the
  KDE desktop QtQuick style applies. Rust → QML state crosses as JSON properties; QML →
  Rust as invokables. Chosen over Slint/egui for a native Plasma look.
- **Audio:** cpal with its PulseAudio backend (served by pipewire-pulse on Plasma),
  48 kHz device side, resampled to the codec rate. Echo cancellation in-app.

## Architecture

```
QML (Kirigami) ⇄ bridge.rs (QObject "Phone", JSON props)  ⇄  engine (tokio thread)
                                                               ├─ transport (UDP/TCP/TLS)
                                                               ├─ registration per account
                                                               ├─ calls (dialog, SDP, REFER)
                                                               └─ media (RTP/SRTP, codecs,
                                                                   jitter, mixer, cpal I/O)
config.rs  ~/.config/megasip/settings.json   (settings + accounts)
store.rs   ~/.local/share/megasip/{history,contacts}.json
```

## Settings view

One Kirigami page with sections: **Accounts** (list; each opens an inline editor with
Name, Server, Username, Password, Display name, and an "Advanced" fold: proxy, domain,
login/auth ID, transport, media encryption, public address, register refresh, keep-alive,
voicemail number, dialing prefix, hide caller ID, session timers), **Audio** (speaker,
microphone, ring device, ringtone, echo cancellation, codec order), **Calls** (auto-answer,
DND, forwarding, deny incoming, call waiting, single-call mode, DTMF method),
**Network** (source port, RTP port range, STUN, rport, DNS SRV), **Application**
(start minimised to tray, bring to front on incoming call, log file).

## Calls made during the build

- rsiprtp's message builders can't copy Record-Route into responses or put parameters on
  Contact, so SIP messages are read and written by our own small text layer
  (`engine/msg.rs`); rsiprtp still does digest auth, SDP parsing, RTP, codecs, DTMF, SRTP
  and DNS.
- rsiprtp's jitter buffer assumes a fixed packet time and never recovers when timestamps
  jump backwards (after hold/resume or a stream change), so playout uses a small
  sequence-number reorder buffer plus a 48 kHz FIFO with drift correction
  (`media/mod.rs`).
- Echo cancellation is sonora (pure-Rust WebRTC APM: AEC3 + noise suppression).
- The ringtone defaults to Plasma's own incoming-call sound.

## Left to the builder

Exact QML layout, icon choices, timer values, file formats.
