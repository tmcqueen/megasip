# One account per MegaSIP window

## Why

You can run several MegaSIP windows at once, each with its own account active, and choose
which account to bring up from the tray. An account can be active in only one window at a time.

## Behaviour (as decided)

- `megasip` with no options opens a window that registers **nothing**; the user picks the
  account to activate.
- `megasip --account foo` / `-a foo` opens a window and registers `foo` — **unless** `foo` is
  already active in another window.
- The tray menu lists the accounts. Ones active in any window are greyed out (this window's
  own is shown checked).
- Account locks live in a SQLite database: a lock table of account, date and PID, so a lock
  left by a process that died can be recognised as stale and taken over.
- Call history moves into a table in the same database. The existing `history.json` is not imported.
- The History tab shows only the active account's calls, so no window has to refresh because another window made a call. An idle window shows no history.

## Approaches considered

- **SQLite lock table (chosen, the user's pick).** One file every window can read for "who has
  what"; it survives crashes through the PID check and gives call history a proper home that is
  safe to write from several processes at once.
- Lock files with `flock` per account. The kernel releases them on a crash, but you can't list
  "which accounts are active" cheaply and they do nothing for history.
- One process with several windows. Fewer moving parts, but it isn't what was asked for
  (separate instances) and one crash takes down every line.

## Design details (my calls; tell me if any are wrong)

- **Database:** `~/.local/share/megasip/megasip.db` (rusqlite, bundled SQLite), WAL mode,
  busy timeout.
  - `account_locks(account_id PRIMARY KEY, pid, proc_start, started_at)`.
  - `call_history(id, number, name, account_id, kind, time, duration, reason)`, indexed on `(account_id, time)`; each account keeps its newest 1000 entries.
- **Stale lock:** the lock is stale if the PID is gone, or the PID's start time in
  `/proc/<pid>/stat` doesn't match `proc_start` (the PID was reused). The lock is taken inside
  `BEGIN IMMEDIATE`, so two windows can't both win. It is released on deactivate and on quit.
- **One account per window.** Picking a free account in the tray:
  - in a window with no account, activates it there;
  - otherwise, launches `megasip -a <id>` as a new window.
  - The tray also gets "Deactivate <account>", which returns the window to idle.
  - The window title and status bar show the active account.
- **Naming the account for `-a`:** matched against the label (case-insensitive), then the id,
  then `user@domain`. If nothing matches, the window opens idle and shows an error.
- **`-a foo` when foo is already active:** that window is brought to the front, and no new
  window opens.
- **Plain `megasip` when an idle window already exists:** that window is raised instead of
  stacking up empty windows.
- **IPC:** one socket per process, `megasip-<pid>.sock`.
  - `megasip <number>` / tel: links go to the window that has the default account, otherwise
    any window with an account, otherwise a new idle window that is asked to dial.
  - `--answer` and `--hangup` go to every window.
  - `--quit` quits them all.
- **Shared settings:** each window reloads `settings.json` when another window saves it, and
  contacts likewise.
  - DND, auto-answer and the other global options stay shared.
  - The default account becomes "the account `megasip <number>` dials out on".
- **Logs:** a window with an active account writes `sip-<label>.log` and `audio-<label>.log`;
  an idle window writes the plain names.
- **Disabled accounts** don't appear in the tray list.
- **`--cli` (as decided):** it registers only the account given with `-a`. Without `-a`, or when the name matches no account, it prints an error and exits with code 2. It takes the same lock as a window, so if the account is active elsewhere it reports that and exits with code 2. Its calls are written to call history for that account, the same as a window's. `--source` works with it. The interop harness passes `-a <phone name>`.

## Left to the builder

The tray and status-bar layout of the account list, and how often the greyed state refreshes
(on menu open, plus a short poll).

## Addendum: `--source <file>`

`megasip --source <path>` plays a `.wav`, `.ogg` or `.mp3` file over the connection in place of
the microphone. It can be combined with `-a`. The calls below are mine:

- It applies to every call in that window. The file starts from the beginning when a call's
  audio starts and loops at the end. Mute still mutes it, and the mic level meter shows it.
- The microphone device is not opened, and echo cancellation and noise suppression are skipped
  for the file.
- The file is decoded up front to 48 kHz mono, up to 10 minutes; anything longer is cut, with a
  warning.
  - WAV and Ogg use the existing `load_sound` path.
  - MP3 is added with a pure-Rust decoder (`symphonia`, mp3 feature only).
- A file that is missing or can't be decoded is reported on stderr, and MegaSIP exits with
  code 2.
- A start with `--source` always opens its own window. It is never handed to a running one,
  because that window already has its own input.
- `--cli` accepts `--source` too (as decided).
