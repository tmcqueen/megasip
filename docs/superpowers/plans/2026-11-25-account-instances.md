# One Account per Window Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Each MegaSIP window activates at most one account. The windows coordinate through a
SQLite lock table, and call history moves into SQLite, scoped to the active account.

**Architecture:**
- A new `db` module owns `megasip.db`: an `instances` table (pid, process start time, account
  or NULL, date) that is both the account lock and the list of running windows, plus a
  `call_history` table.
- `main.rs` routes command-line requests to the right running window over per-PID Unix sockets.
- The bridge holds the window's active account and hands the engine a copy of the settings with
  every other account disabled, so the engine itself doesn't change.

**Tech Stack:** Rust, rusqlite (bundled SQLite), cxx-qt 0.10, Kirigami/QML.

**Spec:** `docs/superpowers/specs/2026-11-25-account-instances-design.md`

## Global Constraints

- `megasip` with no options registers **nothing**.
- `-a foo` / `--account foo` registers foo **unless** foo is already active in another window.
  In that case, raise that window and exit.
- An account in use in another window is greyed out in the tray list; this window's own account
  is checked.
- A lock row is stale when its PID is gone or `/proc/<pid>/stat` field 22 (start time) differs
  from the stored `proc_start`. Stale rows are deleted whenever the table is read.
- History shows only the active account's calls. An idle window shows none. Each account keeps
  its newest 1000 entries. `history.json` is not imported.
- `--cli` registers only the account given with `-a`. Without `-a`, with an unknown name, or with the account active elsewhere, it prints `megasip: …` on stderr and exits 2. It holds the lock like a window, writes call history for its account, and accepts `--source`.
- Never print passwords. Commit only when the user asks.

## Review Focus

1. **Two windows started at the same moment with `-a foo`:** exactly one gets foo; the other
   opens idle with an error. Covered by `db::tests::acquire_is_exclusive`.
2. **A window killed with SIGKILL:** its account becomes free on the next read. Covered by
   `db::tests::stale_dead_pid_is_reclaimed`.
3. **A PID reused by an unrelated process:** the lock is still treated as stale. Covered by
   `db::tests::stale_reused_pid_is_reclaimed`.
4. **An account deleted in Preferences while active:** the window deactivates it and releases
   the lock. Covered by `bridge` behaviour in Task 4, step 4.
5. **`-a` with a name that matches nothing, or with a disabled account:** the window opens idle
   and shows "No account named …". Covered by `config::tests::find_account`.

---

### Task 1: Database module (locks and history)

**Files:**
- Create: `src/db.rs`.
- Modify: `Cargo.toml`, adding `rusqlite = { version = "0.37", features = ["bundled"] }` (or
  whatever the newest version is).
- Modify: `src/main.rs`, adding `mod db;`.

**Interfaces:**
- Produces:
  - `pub struct Instance { pub pid: u32, pub account_id: Option<String>, pub started_at: i64 }`.
  - `pub fn open() -> rusqlite::Result<Connection>`:
    - opens `data_dir()/megasip.db`, sets WAL and `busy_timeout` 5 s;
    - creates `instances(pid INTEGER PRIMARY KEY, proc_start INTEGER NOT NULL, account_id TEXT UNIQUE, started_at INTEGER NOT NULL)`;
    - creates `call_history(id TEXT PRIMARY KEY, account_id TEXT NOT NULL, number TEXT, name TEXT, kind TEXT, time INTEGER, duration INTEGER, reason TEXT)`;
    - creates an index on `(account_id, time)`.
  - `pub fn proc_start(pid: u32) -> Option<u64>`: field 22 of `/proc/<pid>/stat`, parsed after
    the last `)`.
  - `pub fn register_self(c: &Connection) -> Result<()>`: inserts or replaces this PID with
    account NULL.
  - `pub fn unregister_self(c: &Connection)`.
  - `pub fn live(c: &Connection) -> Result<Vec<Instance>>`: deletes stale rows, then returns the
    rest.
  - `pub fn acquire(c: &mut Connection, account: &str) -> Result<Result<(), u32>>`:
    - runs in an IMMEDIATE transaction and prunes stale rows first;
    - `Err(pid)` when another live PID holds the account;
    - otherwise sets this PID's `account_id`, inserting the row if it is missing.
  - `pub fn release(c: &Connection)`: sets this PID's `account_id` to NULL.
  - `pub fn holder(c: &Connection, account: &str) -> Result<Option<u32>>`.
  - `pub fn history(c: &Connection, account: &str) -> Result<Vec<HistoryEntry>>`: oldest first.
  - `pub fn add_history(c: &Connection, e: &HistoryEntry) -> Result<()>`: inserts, then trims
    that account to its newest 1000 rows.
  - `pub fn delete_history(c: &Connection, id: &str)`.
  - `pub fn clear_history(c: &Connection, account: &str)`.
- The PID-taking variants for tests are `register_pid`, `acquire_as` and `release_pid`, with a
  `(pid, proc_start)` parameter. The `_self` functions call them with `std::process::id()`.

- [ ] **Step 1: Write the failing tests** in `src/db.rs` `mod tests`, each using a temp
  `MEGASIP_DATA_DIR`:
  - `acquire_is_exclusive`: `acquire_as` with the real self PID gives Ok. `acquire_as` for the
    same account as PID 1 (live, with its real proc_start) gives `Err(self pid)`.
  - `stale_dead_pid_is_reclaimed`: insert a row for PID 999999 holding "a". `live()` doesn't
    contain it, and `acquire` of "a" gives Ok.
  - `stale_reused_pid_is_reclaimed`: insert a row for PID 1 with proc_start 0 holding "a".
    `acquire` of "a" gives Ok.
  - `history_scoped_and_trimmed`: add 1005 entries to "a" and 1 to "b". `history("a").len()` is
    1000, its oldest is entry 5, and `history("b").len()` is 1.
- [ ] **Step 2:** Run `cargo test db::` and expect it to fail, because the module is missing.
- [ ] **Step 3:** Implement the functions with the signatures above.
- [ ] **Step 4:** Run `cargo test db::` and expect all four tests to pass.

### Task 2: Move history to SQLite; resolve accounts by name

**Files:**
- Modify: `src/store.rs`, removing `load_history`, `save_history` and `MAX_HISTORY` while
  keeping `HistoryEntry` and contacts.
- Modify: `src/config.rs`.

**Interfaces:**
- Produces:
  - `impl Settings { pub fn find_account(&self, name: &str) -> Option<&Account> }`: only
    enabled accounts with a server. It matches the label case-insensitively, then the id, then
    `username@sip_domain()` case-insensitively.
  - `impl Settings { pub fn for_engine(&self, active: Option<&str>) -> Settings }`: a clone
    with `enabled = false` on every account except `active`.

- [ ] **Step 1: Write the failing tests** in config tests:
  - `find_account`: given accounts "Work" (id w1, alice@ex.com) and a disabled "Home":
    - `find_account("work")`, `find_account("w1")` and `find_account("ALICE@ex.com")` all return
      w1;
    - `find_account("Home")` and `find_account("nope")` return None.
  - `for_engine`: with two enabled accounts, `for_engine(Some("w1"))` leaves only w1 enabled,
    and `for_engine(None)` leaves none enabled.
- [ ] **Step 2:** Run `cargo test config::` and expect it to fail.
- [ ] **Step 3:** Implement both functions and delete the JSON history functions from `store.rs`.
  The compile errors left in `bridge.rs` are fixed in Task 4.
- [ ] **Step 4:** Run `cargo test config::` and expect it to pass. Bridge errors are expected
  until Task 4.

### Task 3: Per-PID IPC and command-line routing

**Files:**
- Modify: `src/ipc.rs`.
- Modify: `src/main.rs`.

**Interfaces:**
- Produces:
  - `ipc::path_for(pid: u32) -> PathBuf`, which is `$XDG_RUNTIME_DIR/megasip-<pid>.sock`.
    `MEGASIP_INSTANCE` is no longer read by ipc.
  - `ipc::send_to(pid: u32, line: &str) -> bool`.
  - `ipc::listen()` and `ipc::cleanup()` now use this process's own PID.
  - `ipc::send_self(line) -> bool` for the ctrlc handler.
  - A global `pub static START_ACCOUNT: OnceLock<String>` in `main.rs`: the account id resolved
    from `-a`, which the bridge reads in Task 4.
- Command-line routing, done in `main()` before Qt starts:
  - Load the settings, open the db, and call `live()`. If the db can't be opened, warn on
    stderr and start an idle window.
  - **`-a <name>` / `--account <name>`:** resolve it with `find_account`.
    - Not found: start idle and queue `error No account named "<name>"` to self.
    - Found and held by a live PID: send that PID `show` and exit 0.
    - Otherwise: set `START_ACCOUNT` and start.
  - **No arguments:** if a live instance with `account_id` NULL exists, send it `show` and exit;
    otherwise start.
  - **`<number>`:** send `dial …` to the instance holding `default_account`, else the first one
    with an account, else any instance. If there is none, start idle and queue the dial, as
    today.
  - **`--answer`, `--hangup`, `--quit`:** send to every live instance and exit. If there are
    none, exit.
  - Update `HELP` to add `megasip -a, --account <name>  start with that account active`, and to
    say that plain `megasip` "opens a window with no account active".

- [ ] **Step 1:** Write the failing test `ipc::tests::path_is_per_pid`: `path_for(42)` ends in
  `megasip-42.sock`.
- [ ] **Step 2:** Implement and run `cargo test ipc::`. Expect a pass.

### Task 4: Active account in the bridge

**Files:**
- Modify: `src/bridge.rs`.
- Modify: `src/logging.rs`.

**Interfaces:**
- Consumes: `db::*` (Task 1), `Settings::for_engine` and `find_account` (Task 2), and
  `START_ACCOUNT` (Task 3).
- Produces for QML:
  - `#[qproperty(QString, active_account)]`: the account id, or "".
  - `#[qinvokable] fn accounts_in_use(self: &Phone) -> QString`: a JSON array of account ids
    held by *other* live windows.
  - `#[qinvokable] fn pick_account(self: Pin<&mut Phone>, id: &QString)`:
    - when this window is idle, activates the account here;
    - otherwise spawns `current_exe() -a <id>` detached;
    - when the account is held elsewhere, sends `show` to the holder.
  - `#[qinvokable] fn deactivate_account(self: Pin<&mut Phone>)`.
- `logging::set_log_name(label: Option<&str>)`:
  - log files become `sip-<slug>.log` and `audio-<slug>.log`, where the slug is the label
    lowercased with characters other than `[a-z0-9]` replaced by `-`;
  - with None they are `sip.log` and `audio.log`;
  - it reopens the files. `FileSink` gains `reopen(Option<PathBuf>)`.

- [ ] **Step 1: Startup (`initialize`).**
  - Open the db, keep the `Connection` in `PhoneRust`, and call `register_self`.
  - Start the engine with `settings.for_engine(None)`.
  - If `START_ACCOUNT` is set, call `activate(id)`.
- [ ] **Step 2: `fn activate(self: Pin<&mut Self>, id: &str)`.**
  - Call `db::acquire`. If the account is held, emit the error
    "<title> is already active in another window" and stay idle.
  - On Ok:
    - set `active_account`;
    - send `Command::Configure(for_engine(Some(id)))`;
    - call `set_log_name(Some(title))`;
    - load the history from the db and publish it.
  - `deactivate_account` reverses this: release the lock, configure with None, use the plain log
    names, and empty the history.
- [ ] **Step 3: History.**
  - `Event::Ended` calls `db::add_history` only when `info.account_id` equals the active
    account. It then reloads `r.history` from the db and publishes it.
  - `delete_history` and `clear_history` go to the db; `clear_history` is scoped to the active
    account.
- [ ] **Step 4: Settings.** Every `Command::Configure` goes through `for_engine(active)`. When
  the saved settings no longer contain the active account, or it is disabled, call
  `deactivate_account`.
- [ ] **Step 5: Changes from other windows.**
  - A thread polls the mtime of `settings.json` and `contacts.json` every 2 s.
  - On a change, it queues `reload_external` on the Qt thread. That function reloads whichever
    file changed, but only if its content differs from what is in memory, then publishes it and
    reconfigures the engine through `for_engine`.
- [ ] **Step 6: IPC and quit.**
  - `on_ipc` handles `error <text>` by emitting `error_message`.
  - `quit()` calls `db::unregister_self` before exiting.
  - The ctrlc handler uses `ipc::send_self`.
- [ ] **Step 7: Status text.** When idle, show "No account active — pick one from the tray".
- [ ] **Step 8:** Run `cargo build && cargo test && cargo clippy --all-targets`. Expect it to
  compile, all tests to pass, and no warnings.

### Task 4b: `--cli` takes one account

**Files:**
- Modify: `src/cli.rs` and `src/main.rs`.
- Modify: `tests/interop/harness.py`, so `Popen([BIN, "--cli", "-a", name], …)` and the
  `MEGASIP_INSTANCE` env var is dropped.

**Interfaces:**
- Changes: `cli::run(account_id: String)`.

- [ ] **Step 1: In `main.rs`, for `--cli`.**
  - Parse `-a`/`--account` and resolve it with `find_account`.
    - Missing: `megasip: --cli needs -a <account>`.
    - Unknown: `megasip: no account named "<name>"`.
  - Call `db::acquire`. If it returns `Err(pid)`: `megasip: <title> is already active (pid <pid>)`.
  - Every one of these errors exits 2.
- [ ] **Step 2: In `cli::run`.**
  - Start the engine with `settings.for_engine(Some(&id))`, and call `db::unregister_self` on exit.
  - On `Event::Ended` for its account, write a `HistoryEntry` with `db::add_history`, built the same way as the bridge's. Move that construction into `fn history_entry(info: &CallInfo, missed: bool) -> HistoryEntry` in `store.rs` so both callers share it.
  - Add `history` to the harness's "basic call" test: after hangup, `megasip.db` in alice's data dir has one `out` row for alice's account id (check it with `sqlite3`, or Python's `sqlite3` module).
- [ ] **Step 3:** Run `./target/debug/megasip --cli` with no `-a` and expect exit code 2 with
  the message.

### Task 5: Tray account list and window title

**Files:**
- Modify: `qml/Main.qml`.
- Modify: `qml/StatusBar.qml`.

- [ ] **Step 1: The tray menu.**
  - Below Show/Hide, add an "Accounts" section using `Instantiator` over the enabled accounts in
    `root.settings.accounts` that have a server.
  - Each item is a checkable `Platform.MenuItem` with the account's title:
    - `checked` when its id is `phone.activeAccount`;
    - `enabled: !inUse.includes(id)`;
    - `onTriggered: phone.pickAccount(id)`.
  - Then add "Deactivate <title>", visible when `phone.activeAccount` is set.
  - `property var inUse` is refreshed from `JSON.parse(phone.accountsInUse())` by
    `menu.aboutToShow` and by a 3 s Timer.
- [ ] **Step 2: Window and status bar.**
  - The window title is `"MegaSIP — " + <active title>`, or `"MegaSIP"` when idle.
  - The StatusBar shows the active account.
- [ ] **Step 3: Live check.** Start `megasip` and confirm it is idle. From the tray, pick account
  A: it registers in this window. Pick B: a second window opens with B. In each tray menu the
  other account is greyed out.
  - Run `megasip -a A`: window 1 is raised and no third window opens.
  - `kill -9` the second window, open the tray in the first, and check that B is selectable
    again within 3 s.
  - Make a call in each window and check that each History tab shows only its own call.

### Task 6: Docs

- [ ] **Step 1:** Update `README.md` with the usage text and multiple windows, and the main
  design doc's storage line (`megasip.db`).
- [ ] **Step 2:** Run the interop suite (`tests/interop/run.sh`) and expect 10/10, now with each phone started by `--cli -a <name>`.

### Task 7: `--source <file>` replaces the microphone

**Files:**
- Modify: `Cargo.toml`, adding `symphonia = { version = "0.5", default-features = false, features = ["mp3"] }`.
- Modify: `src/media/dsp.rs`, `src/media/hub.rs`, `src/main.rs`, `src/cli.rs`.

**Interfaces:**
- Produces:
  - `dsp::load_sound_max(path: &str, max_secs: u32) -> Option<Vec<i16>>`, returning 48 kHz
    mono. It adds `.mp3` through symphonia. `load_sound(path)` becomes
    `load_sound_max(path, 60)`.
  - `AudioHub::set_source(&self, samples: Option<Arc<Vec<i16>>>)`.

- [ ] **Step 1:** Write the failing test `dsp::tests::loads_mp3_and_wav`. Decode a short
  generated WAV, and an MP3 fixture at `tests/data/tone.mp3` made with `ffmpeg -f lavfi -i
  sine=440:d=1 tone.mp3`. Both are about 48 000 samples (±5%), with a peak over 1000.
- [ ] **Step 2:** Implement `load_sound_max`, then run `cargo test dsp::` and expect a pass.
- [ ] **Step 3: The mixer, in `hub.rs`.**
  - While a source is set, don't open the mic device.
  - Each 20 ms tick, take the next `FRAME` samples from the source (looping) as the
    microphone frame, and skip the APM for it.
  - Reset the position to 0 when the stream count goes from 0 to more than 0.
  - Mute and the mic level meter work as they do for the mic.
- [ ] **Step 4: The command line, in `main.rs` and `cli.rs`.**
  - Parse `--source <path>` and load it with `load_sound_max(path, 600)`. If it fails, print
    `megasip: can't play <path>` and exit 2.
  - With `--source`, skip forwarding to running windows.
  - Hand the samples to the hub after `AudioHub::start()`, through a
    `pub static SOURCE: OnceLock<Arc<Vec<i16>>>`.
  - Add the option to `HELP`.
- [ ] **Step 5: Live check.** Start `megasip -a A --source tests/data/tone.mp3`, call another
  window or baresip, and hear the tone. In the debug window's Call quality tab, Sent is
  climbing.

#### Task 7 additions (agreed after the plan was written)

- **Built-in generators.** `--source milli|pink|white|tone:<hz>` takes priority over a file of
  the same name; write `./pink` to play a file called `pink`.
  - `milli` is a 1004 Hz sine at 0 dBm0, a peak of 22 653 (about −3.17 dBFS).
  - `white` is uniform noise at −20 dBFS RMS.
  - `pink` is the same noise through Paul Kellet's filter, scaled to −20 dBFS RMS.
  - `tone:<hz>` is a sine at the milli level, for 20 Hz to 20 kHz.
- **Types.** `media::source::Source` is an enum `File(Arc<Vec<i16>>) | Tone(f32) | Pink | White`,
  with `fn parse(arg: &str) -> Result<Source, String>`, `fn label(&self) -> String` (for
  example "1004 Hz tone", "Pink noise", "bar.wav"), and a `SourcePlayer` whose
  `next_frame(&mut [i16; FRAME])` method does the looping and generation.
- **Tests:**
  - `source::tests::parse_keywords`: milli gives Tone(1004.0), and `tone:440` gives
    Tone(440.0).
  - `source::tests::levels`: the milli peak is 22 653 ± 50, and white and pink RMS are within
    1 dB of −20 dBFS over 10 s.
- **`MEGASIP_TEST_TONE_HZ` is removed.** The harness passes `--source tone:<hz>`, and null-audio
  mode with no source sends silence.
- **A queued `dial` waits for registration.** A window started with `-a` and a number dials
  once that account is registered, or after 10 s.
- **A held account is an error with `--source`.** `-a foo` with `--source`, while foo is active
  elsewhere, prints `megasip: foo is already active (pid …)` and exits 2.
- **UI.** `Phone` gains `#[qproperty(QString, source_label)]`, empty when the microphone is in
  use. It is shown as:
  - a status bar chip "Source: <label>", whose tooltip is the full path;
  - the CallsPanel mic meter label;
  - the tray tooltip suffix " · playing <label>";
  - an "Input: <label>" row on the Call quality card, through `debugQuality` JSON field
    `source`;
  - the Preferences → Audio microphone picker, disabled with the note "Using <label> from the
    command line".
