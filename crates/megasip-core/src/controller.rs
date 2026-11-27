//! The window controller: what a MegaSIP window does, whatever toolkit draws it.
//!
//! It owns a window's state (settings, contacts, history, calls, the active account and its
//! lock in `megasip.db`, the engine and audio hub) and turns engine events, IPC requests
//! from other `megasip` processes and the user's actions into engine commands and state
//! changes. It never calls into the UI. Instead it queues [`Out`] messages, which the
//! frontend drains with [`Controller::take_outbox`] after each call and applies: a
//! [`Changed`] means "re-read that piece of state through the getters".
//!
//! Threading: the controller is single-threaded. The frontend forwards engine events,
//! IPC lines and timers onto its UI thread and calls the controller from there.

use crate::accounticon::{self, Identity};
use crate::config::{Account, Settings};
use crate::engine::{self, AccountStatus, CallInfo, Command, Event};
use crate::media::hub::AudioHub;
use crate::media::source::Source;
use crate::store::{self, Contact, HistoryEntry};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// A piece of state that changed; re-read it through the matching getter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Changed {
    /// `accounts_json`
    Accounts,
    /// `calls_json`
    Calls,
    /// `history_json`
    History,
    /// `contacts_json`
    Contacts,
    /// `settings_json`, `dnd`, `auto_answer`
    Settings,
    /// `status_text`
    Status,
    /// `active_account`
    ActiveAccount,
    /// `missed_count`
    MissedCount,
    /// `source_label`, `source_detail`
    Source,
    /// `muted`
    Muted,
    /// `conference`
    Conference,
}

/// Something that fires later: the frontend waits `after` (see [`Out::Schedule`]), then
/// calls [`Controller::on_timer`] with it on the UI thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Timer {
    /// `megasip -a foo 123`: dial anyway if the account hasn't registered by now.
    FlushPendingDial,
    /// `megasip 123` (no `-a`): dial once the window is up.
    StartupDial(String),
}

/// What the frontend should do, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    Changed(Changed),
    Incoming {
        id: u64,
        name: String,
        number: String,
    },
    Missed {
        name: String,
        number: String,
    },
    Error(String),
    /// Bring the window to the front.
    Activate,
    Dtmf {
        id: u64,
        digit: char,
    },
    /// Show the window as this app id and icon (taskbar, tray).
    Identity(Identity),
    Schedule {
        after: Duration,
        timer: Timer,
    },
}

/// The command line's requests for a new window.
#[derive(Debug, Clone, Default)]
pub struct StartRequest {
    /// `-a`: the account to activate, as an account id.
    pub account: Option<String>,
    /// A number or URI to dial once the window is up.
    pub dial: Option<String>,
    /// An error to show once the window is up.
    pub error: Option<String>,
}

pub struct Controller {
    db: Option<rusqlite::Connection>,
    /// The account this window has active (the lock in `db` is ours).
    active: Option<String>,
    /// `megasip -a foo 1234`: dial once foo is registered.
    pending_dial: Option<String>,
    /// What's sent instead of the microphone (`--source`), if anything.
    source: Option<Source>,
    /// A source handed over with a call (`megasip -a foo --source x 123` while this window
    /// has foo) lasts for that call: this is what to go back to once the calls are over.
    restore_source: Option<Option<Source>>,
    cmd: Option<UnboundedSender<Command>>,
    hub: Option<Arc<AudioHub>>,
    settings: Settings,
    calls: BTreeMap<u64, CallInfo>,
    history: Vec<HistoryEntry>,
    contacts: Vec<Contact>,
    accounts: Vec<AccountStatus>,
    missed_count: i32,
    muted: bool,
    conference: bool,
    status: String,
    outbox: Vec<Out>,
}

/// Digits (and `+`) of a number, for matching it against contacts; short or non-numeric
/// targets (extensions, SIP user names) compare as lowercase text.
pub fn norm_number(n: &str) -> String {
    let digits: String = n
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '+')
        .collect();
    if digits.len() >= 3 {
        digits
    } else {
        n.trim().to_ascii_lowercase()
    }
}

/// The target of an IPC `dial` line: `tel:` is stripped (with `callto:` before it, as in
/// `callto:tel:`); a `sip:` URI is kept, only a leading `callto:` comes off it.
pub fn dial_target(target: &str) -> String {
    let t = target.trim();
    if t.starts_with("tel:") {
        t.trim_start_matches("callto:")
            .trim_start_matches("tel:")
            .to_string()
    } else {
        t.trim_start_matches("callto:").to_string()
    }
}

fn autostart_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("autostart/megasip.desktop")
}

fn same_settings(a: &Settings, b: &Settings) -> bool {
    serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
}

/// Watch settings.json and contacts.json for saves by other windows, every 2 s, on a thread
/// of its own. `changed(settings, contacts)` returns false to stop.
pub fn watch_shared_files(mut changed: impl FnMut(bool, bool) -> bool + Send + 'static) {
    std::thread::Builder::new()
        .name("megasip-watch".into())
        .spawn(move || {
            let mtime =
                |p: std::path::PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
            let paths = || {
                (
                    Settings::path(),
                    crate::config::data_dir().join("contacts.json"),
                )
            };
            let (sp, cp) = paths();
            let (mut s0, mut c0) = (mtime(sp), mtime(cp));
            loop {
                std::thread::sleep(Duration::from_secs(2));
                let (sp, cp) = paths();
                let (s1, c1) = (mtime(sp), mtime(cp));
                let (sc, cc) = (s1 != s0, c1 != c0);
                (s0, c0) = (s1, c1);
                if (sc || cc) && !changed(sc, cc) {
                    break;
                }
            }
        })
        .expect("watch thread");
}

impl Controller {
    /// Start a window: load settings and contacts, start the audio hub and the engine, and
    /// list this process in `megasip.db`. The engine's events come out of the receiver;
    /// hand each to [`Controller::on_event`] on the UI thread. Call
    /// [`Controller::start_up`] once the UI is listening.
    pub fn start() -> (Self, UnboundedReceiver<Event>) {
        let settings = Settings::load();
        let contacts = store::load_contacts();
        let hub = AudioHub::start();
        let source = crate::SOURCE.get().cloned();
        if let Some(src) = &source {
            hub.set_source(Some(src.clone()));
        }
        let mut errors = Vec::new();
        let db = match crate::db::open() {
            Ok(c) => {
                match crate::db::register_self(&c) {
                    Ok(()) => {
                        tracing::debug!("window pid {} listed in megasip.db", std::process::id())
                    }
                    Err(e) => tracing::warn!("couldn't list this window in megasip.db: {e}"),
                }
                Some(c)
            }
            Err(e) => {
                errors.push(format!("Can't open megasip.db ({e}): accounts can't be activated and call history isn't saved"));
                None
            }
        };
        let (ev_tx, ev_rx) = tokio::sync::mpsc::unbounded_channel::<Event>();
        let cmd = engine::spawn(settings.for_engine(None), hub.clone(), ev_tx);
        let mut c = Self::from_parts(settings, contacts, db, Some(cmd), Some(hub), source);
        c.outbox.extend(errors.into_iter().map(Out::Error));
        (c, ev_rx)
    }

    /// A controller over the given parts, with no account active. `start` uses this; tests
    /// pass a channel to see the engine commands, and no hub.
    pub fn from_parts(
        settings: Settings,
        contacts: Vec<Contact>,
        db: Option<rusqlite::Connection>,
        cmd: Option<UnboundedSender<Command>>,
        hub: Option<Arc<AudioHub>>,
        source: Option<Source>,
    ) -> Self {
        let mut c = Self {
            db,
            active: None,
            pending_dial: None,
            source,
            restore_source: None,
            cmd,
            hub,
            settings,
            calls: BTreeMap::new(),
            history: Vec::new(),
            contacts,
            accounts: Vec::new(),
            missed_count: 0,
            muted: false,
            conference: false,
            status: String::new(),
            outbox: Vec::new(),
        };
        c.refresh_status();
        c
    }

    /// Everything the frontend should do since the last call, in order.
    pub fn take_outbox(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.outbox)
    }

    fn out(&mut self, o: Out) {
        self.outbox.push(o);
    }

    fn changed(&mut self, c: Changed) {
        self.out(Out::Changed(c));
    }

    fn error(&mut self, text: impl Into<String>) {
        self.out(Out::Error(text.into()));
    }

    fn send(&self, c: Command) {
        if let Some(tx) = &self.cmd {
            let _ = tx.send(c);
        }
    }

    // ----- state, for the frontend -----

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn active_account(&self) -> Option<&str> {
        self.active.as_deref()
    }

    pub fn calls(&self) -> impl Iterator<Item = &CallInfo> {
        self.calls.values()
    }

    pub fn hub(&self) -> Option<&Arc<AudioHub>> {
        self.hub.as_ref()
    }

    pub fn status_text(&self) -> &str {
        &self.status
    }

    pub fn missed_count(&self) -> i32 {
        self.missed_count
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    pub fn conference(&self) -> bool {
        self.conference
    }

    pub fn dnd(&self) -> bool {
        self.settings.dnd
    }

    pub fn auto_answer(&self) -> bool {
        self.settings.auto_answer != "off"
    }

    /// What `--source` sends instead of the microphone: label and detail ("" for the mic).
    pub fn source_label(&self) -> (String, String) {
        self.source
            .as_ref()
            .map(|s| (s.label(), s.detail()))
            .unwrap_or_default()
    }

    pub fn accounts_json(&self) -> String {
        serde_json::to_string(&self.accounts).unwrap_or_default()
    }

    pub fn calls_json(&self) -> String {
        let list: Vec<&CallInfo> = self.calls.values().collect();
        serde_json::to_string(&list).unwrap_or_default()
    }

    /// Newest first, with names from the contacts where the call had none.
    pub fn history_json(&self) -> String {
        let mut list: Vec<serde_json::Value> = Vec::new();
        for h in self.history.iter().rev() {
            let mut v = serde_json::to_value(h).unwrap_or_default();
            if h.name.is_empty() {
                if let Some(n) = self.lookup(&h.number) {
                    v["name"] = serde_json::Value::String(n);
                }
            }
            list.push(v);
        }
        serde_json::to_string(&list).unwrap_or_default()
    }

    /// Favourites first, then by name.
    pub fn contacts_json(&self) -> String {
        let mut c = self.contacts.clone();
        c.sort_by(|a, b| {
            b.favorite
                .cmp(&a.favorite)
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        serde_json::to_string(&c).unwrap_or_default()
    }

    pub fn settings_json(&self) -> String {
        serde_json::to_string(&self.settings).unwrap_or_default()
    }

    /// A contact's name for `number`, if one matches.
    pub fn lookup(&self, number: &str) -> Option<String> {
        let n = norm_number(number);
        if n.is_empty() {
            return None;
        }
        self.contacts
            .iter()
            .find(|c| {
                let cn = norm_number(&c.number);
                cn == n || (cn.len() >= 7 && n.len() >= 7 && (cn.ends_with(&n) || n.ends_with(&cn)))
            })
            .map(|c| c.name.clone())
            .filter(|s| !s.is_empty())
    }

    // ----- inputs -----

    pub fn on_event(&mut self, ev: Event) {
        match ev {
            Event::Accounts(list) => {
                tracing::debug!(
                    "accounts: {:?}",
                    list.iter()
                        .map(|a| format!("{}={}", a.id, a.state))
                        .collect::<Vec<_>>()
                );
                let registered = self
                    .active
                    .as_ref()
                    .is_some_and(|id| list.iter().any(|a| &a.id == id && a.state == "registered"));
                self.accounts = list;
                self.changed(Changed::Accounts);
                self.refresh_status();
                if registered {
                    self.flush_pending_dial();
                }
            }
            Event::Call(mut info) => {
                self.name_call(&mut info);
                self.calls.insert(info.id, info);
                self.changed(Changed::Calls);
            }
            Event::Incoming(mut info) => {
                self.name_call(&mut info);
                let (id, name, number) = (info.id, info.name.clone(), info.number.clone());
                self.calls.insert(info.id, info);
                self.changed(Changed::Calls);
                self.out(Out::Incoming { id, name, number });
                if self.settings.bring_to_front {
                    self.out(Out::Activate);
                }
            }
            Event::Ended { mut info, missed } => {
                self.name_call(&mut info);
                self.calls.remove(&info.id);
                // History belongs to the account; calls on the local identity aren't kept.
                if self.active.as_deref() == Some(info.account_id.as_str()) {
                    let entry = store::history_entry(&info, missed);
                    let saved = self.db.as_ref().map(|c| crate::db::add_history(c, &entry));
                    if let Some(Err(e)) = saved {
                        self.error(format!("Couldn't save call history: {e}"));
                    }
                    self.load_history();
                }
                if missed {
                    self.missed_count += 1;
                    self.changed(Changed::MissedCount);
                    self.out(Out::Missed {
                        name: info.name.clone(),
                        number: info.number.clone(),
                    });
                }
                if self.calls.is_empty() {
                    self.conference = false;
                    self.changed(Changed::Conference);
                    if let Some(prev) = self.restore_source.take() {
                        self.use_source(prev);
                    }
                }
                self.changed(Changed::History);
                self.changed(Changed::Calls);
            }
            Event::Dtmf(id, digit) => self.out(Out::Dtmf { id, digit }),
            Event::Error(e) => self.error(e),
        }
        if let Some(h) = self.hub.clone() {
            for e in h.take_errors() {
                self.error(e);
            }
        }
    }

    fn name_call(&self, info: &mut CallInfo) {
        if info.name.is_empty() {
            if let Some(n) = self.lookup(&info.number) {
                info.name = n;
            }
        }
    }

    /// A request from another `megasip` process (see `ipc`).
    pub fn on_ipc(&mut self, line: &str) {
        let line = line.trim();
        tracing::debug!(
            "ipc: handling {line:?} (active account {:?}, {} call(s) up)",
            self.active,
            self.calls.len()
        );
        if let Some(rest) = line.strip_prefix("dial ") {
            let (target, source) = crate::ipc::split_dial(rest);
            tracing::debug!("ipc: dial target {target:?} source {source:?}");
            if let Some(src) = source {
                if !self.switch_source(src) {
                    tracing::debug!("ipc: source {src:?} refused; not dialing");
                    self.out(Out::Activate);
                    return;
                }
                tracing::debug!("ipc: now sending {src:?}");
            }
            let t = dial_target(target);
            self.out(Out::Activate);
            let acc = self.active.clone().unwrap_or_default();
            tracing::debug!("ipc: dialing {t:?} on account {acc:?}");
            self.dial(&t, &acc);
        } else if line == "answer" {
            if let Some(id) = self
                .calls
                .values()
                .find(|c| c.state == "incoming")
                .map(|c| c.id)
            {
                self.send(Command::Answer(id));
            }
        } else if line == "hangup" {
            self.send(Command::HangupAll);
        } else if line == "quit" {
            self.quit();
        } else if let Some(e) = line.strip_prefix("error ") {
            self.error(e);
        } else if let Some(src) = line.strip_prefix("source ") {
            self.switch_source(src.trim());
            self.out(Out::Activate);
        } else {
            self.out(Out::Activate);
        }
    }

    pub fn on_timer(&mut self, t: Timer) {
        match t {
            Timer::FlushPendingDial => self.flush_pending_dial(),
            Timer::StartupDial(d) => {
                self.out(Out::Activate);
                let acc = self.active.clone().unwrap_or_default();
                self.dial(&d, &acc);
            }
        }
    }

    /// Another window saved settings or contacts.
    pub fn reload_external(&mut self, settings: bool, contacts: bool) {
        if settings {
            let s = Settings::load_beside(&self.settings);
            if !same_settings(&s, &self.settings) {
                crate::logging::apply(&s);
                self.settings = s.clone();
                self.configure_with(&s);
                self.changed(Changed::Settings);
                self.drop_vanished_account();
                self.refresh_status();
                self.refresh_identity();
            }
        }
        if contacts {
            let c = store::load_contacts();
            if c != self.contacts {
                self.contacts = c;
                self.changed(Changed::Contacts);
                self.changed(Changed::History);
            }
        }
    }

    /// Start-up requests from the command line: show `error`, take the `-a` account, then
    /// dial. Call once the UI is listening, so a lost race ("already active") is shown.
    pub fn start_up(&mut self, req: StartRequest) {
        let StartRequest {
            account,
            dial,
            error,
        } = req;
        tracing::debug!("start_up: account {account:?} dial {dial:?}");
        if let Some(id) = &account {
            self.activate(id);
            if self.active.is_none() {
                // The window started out as that account; it didn't get it after all.
                self.refresh_identity();
            }
        }
        if let Some(e) = error {
            self.error(e);
        }
        // A wallet problem at start-up (locked, missing) shows up in the window.
        if !self.settings.secret_error.is_empty() {
            let e = format!("Passwords: {}", self.settings.secret_error);
            self.error(e);
        }
        let Some(d) = dial else { return };
        if account.is_some() {
            if self.active.is_none() {
                // `-a` didn't get its account (another window won the race): don't call
                // from the wrong identity; hand the number to the window that has it.
                let holder = account.as_deref().and_then(|id| {
                    self.db
                        .as_ref()
                        .and_then(|c| crate::db::holder(c, id).ok().flatten())
                });
                tracing::debug!("start_up: lost the account race; holder {holder:?}");
                if let Some(pid) = holder {
                    let src = crate::SOURCE.get().map(|s| s.arg());
                    crate::ipc::send_lines_to(
                        pid,
                        &crate::ipc::handoff_lines(Some(&d), src.as_deref()),
                    );
                }
                return;
            }
            // Dial once registered (on_event), or after 10 s regardless.
            tracing::debug!("start_up: will dial {d:?} once registered (or in 10 s)");
            self.pending_dial = Some(d);
            self.out(Out::Schedule {
                after: Duration::from_secs(10),
                timer: Timer::FlushPendingDial,
            });
        } else {
            self.out(Out::Schedule {
                after: Duration::from_millis(1500),
                timer: Timer::StartupDial(d),
            });
        }
    }

    // ----- the user's actions -----

    pub fn dial(&mut self, number: &str, account: &str) {
        let number = number.trim().to_string();
        if number.is_empty() {
            tracing::debug!("dial: empty number, nothing to do");
            return;
        }
        tracing::debug!("dial: Command::Dial {number:?} on account {account:?}");
        self.send(Command::Dial {
            account: account.to_string(),
            number: number.clone(),
        });
        let mut s = self.settings.clone();
        if s.last_number != number {
            s.last_number = number;
            if let Err(e) = self.apply_settings(None, s) {
                tracing::warn!("couldn't save the last number: {e}");
            }
        }
    }

    pub fn redial(&mut self) {
        let n = self.settings.last_number.clone();
        let acc = self.active.clone().unwrap_or_default();
        self.dial(&n, &acc);
    }

    pub fn answer(&mut self, id: u64) {
        self.send(Command::Answer(id));
    }

    pub fn reject(&mut self, id: u64) {
        self.send(Command::Reject(id));
    }

    pub fn hangup(&mut self, id: u64) {
        self.send(Command::Hangup(id));
    }

    pub fn hangup_all(&mut self) {
        self.send(Command::HangupAll);
    }

    pub fn hold(&mut self, id: u64, on: bool) {
        self.send(Command::Hold(id, on));
    }

    pub fn dtmf(&mut self, id: u64, digit: char) {
        self.send(Command::Dtmf(id, digit));
    }

    pub fn transfer(&mut self, id: u64, target: &str) {
        self.send(Command::Transfer(id, target.to_string()));
    }

    pub fn attended_transfer(&mut self, id: u64, other: u64) {
        self.send(Command::AttendedTransfer(id, other));
    }

    pub fn set_conference(&mut self, on: bool) {
        self.send(Command::Conference(on));
        self.conference = on;
        self.changed(Changed::Conference);
    }

    pub fn set_mute(&mut self, on: bool) {
        self.send(Command::Mute(on));
        self.muted = on;
        self.changed(Changed::Muted);
    }

    pub fn set_dnd(&mut self, on: bool) {
        let mut s = self.settings.clone();
        s.dnd = on;
        if let Err(e) = self.apply_settings(None, s) {
            self.error(e);
        }
    }

    pub fn set_auto_answer(&mut self, on: bool) {
        let mut s = self.settings.clone();
        s.auto_answer = if on { "all".into() } else { "off".into() };
        if let Err(e) = self.apply_settings(None, s) {
            self.error(e);
        }
    }

    /// Save the Preferences draft `json`, which was started from `base` (both JSON).
    /// Returns the error to show, or "" when saved.
    pub fn save_settings_json(&mut self, base: &str, json: &str) -> String {
        let base = serde_json::from_str::<Settings>(base).ok();
        match serde_json::from_str::<Settings>(json) {
            Ok(s) => match self.apply_settings(base, s) {
                Ok(()) => String::new(),
                Err(e) => e,
            },
            Err(e) => format!("Invalid settings: {e}"),
        }
    }

    /// Save `s`, a change of `base` (default: this window's settings), merged with whatever
    /// other windows saved meanwhile.
    pub fn apply_settings(
        &mut self,
        base: Option<Settings>,
        mut s: Settings,
    ) -> Result<(), String> {
        s.normalize();
        let mut base = base.unwrap_or_else(|| self.settings.clone());
        base.normalize();
        let disk = Settings::load_beside(&self.settings);
        if !same_settings(&disk, &base) {
            s = crate::config::merge_settings(&base, &s, &disk);
            s.normalize();
        }
        s.validate()?;
        s.save()?;
        self.configure_with(&s);
        crate::logging::apply(&s);
        self.settings = s;
        self.changed(Changed::Settings);
        self.drop_vanished_account();
        self.refresh_identity();
        accounticon::prune(&self.settings.accounts);
        Ok(())
    }

    pub fn new_account_json(&self) -> String {
        let a = Account {
            id: crate::engine::msg::random_token(10),
            ..Default::default()
        };
        serde_json::to_string(&a).unwrap_or_default()
    }

    pub fn save_contact_json(&mut self, json: &str) {
        let Ok(mut c) = serde_json::from_str::<Contact>(json) else {
            return;
        };
        if c.number.trim().is_empty() {
            return;
        }
        if c.id.is_empty() {
            c.id = crate::engine::msg::random_token(10);
        }
        // Start from the file: another window may have changed it.
        self.contacts = store::load_contacts();
        match self.contacts.iter_mut().find(|x| x.id == c.id) {
            Some(x) => *x = c,
            None => self.contacts.push(c),
        }
        store::save_contacts(&self.contacts);
        self.changed(Changed::Contacts);
        self.changed(Changed::History);
    }

    pub fn delete_contact(&mut self, id: &str) {
        // Start from the file: another window may have changed it.
        self.contacts = store::load_contacts();
        self.contacts.retain(|c| c.id != id);
        store::save_contacts(&self.contacts);
        self.changed(Changed::Contacts);
        self.changed(Changed::History);
    }

    /// Add the contacts in the file at `path` (a path or file:// URL) whose numbers are
    /// new. Returns how many the file had.
    pub fn import_contacts(&mut self, path: &str) -> i32 {
        let p = path.strip_prefix("file://").unwrap_or(path);
        let new = store::import_contacts(p);
        let n = new.len() as i32;
        // Start from the file: another window may have changed it.
        self.contacts = store::load_contacts();
        for c in new {
            if !self.contacts.iter().any(|x| x.number == c.number) {
                self.contacts.push(c);
            }
        }
        store::save_contacts(&self.contacts);
        self.changed(Changed::Contacts);
        self.changed(Changed::History);
        n
    }

    pub fn export_contacts(&self, path: &str) -> bool {
        let p = path.strip_prefix("file://").unwrap_or(path);
        store::export_contacts_csv(p, &self.contacts).is_ok()
    }

    pub fn delete_history(&mut self, id: &str) {
        if let Some(c) = &self.db {
            crate::db::delete_history(c, id);
        }
        self.load_history();
    }

    pub fn clear_history(&mut self) {
        if let (Some(c), Some(id)) = (&self.db, &self.active) {
            crate::db::clear_history(c, id);
        }
        self.load_history();
        self.missed_count = 0;
        self.changed(Changed::MissedCount);
        self.changed(Changed::History);
    }

    pub fn seen_missed(&mut self) {
        self.missed_count = 0;
        self.changed(Changed::MissedCount);
    }

    pub fn mic_level(&self) -> f64 {
        self.hub
            .as_ref()
            .map(|h| h.levels().0 as f64)
            .unwrap_or(0.0)
    }

    pub fn speaker_level(&self) -> f64 {
        self.hub
            .as_ref()
            .map(|h| h.levels().1 as f64)
            .unwrap_or(0.0)
    }

    pub fn autostart_enabled(&self) -> bool {
        autostart_path().exists()
    }

    pub fn set_autostart(&mut self, on: bool) {
        let p = autostart_path();
        let res = if on {
            let exe = std::env::current_exe()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_else(|_| "megasip".into());
            let body = format!(
                "[Desktop Entry]\nType=Application\nName=MegaSIP\nExec={exe} --minimized\nIcon=call-start\nX-KDE-autostart-phase=2\nX-GNOME-Autostart-enabled=true\n"
            );
            p.parent()
                .map(std::fs::create_dir_all)
                .transpose()
                .and_then(|_| std::fs::write(&p, body))
        } else {
            std::fs::remove_file(&p).or_else(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(e)
                }
            })
        };
        if let Err(e) = res {
            self.error(format!("Autostart: {e}"));
        }
    }

    /// Live and last-finished stream statistics for the Debug window, each stream named
    /// after its call.
    pub fn debug_quality_json(&self) -> String {
        let (live, last) = self.hub.as_ref().map(|h| h.stats()).unwrap_or_default();
        let (mic, spk) = self.hub.as_ref().map(|h| h.levels()).unwrap_or_default();
        let label = |id: u64| {
            self.calls.get(&id).map(|c| {
                if c.name.is_empty() {
                    c.number.clone()
                } else {
                    format!("{} ({})", c.name, c.number)
                }
            })
        };
        let named = |s: &crate::media::StreamStats| {
            let mut v = serde_json::to_value(s).unwrap_or_default();
            v["label"] =
                serde_json::Value::String(label(s.id).unwrap_or_else(|| format!("Call {}", s.id)));
            v
        };
        serde_json::json!({
            "source": self.source_label().0,
            "streams": live.iter().map(named).collect::<Vec<_>>(),
            "last": last.as_ref().map(named),
            "mic": mic,
            "speaker": spk,
        })
        .to_string()
    }

    pub fn debug_clear(&self) {
        crate::debug::clear();
        if let Some(h) = self.hub.as_ref() {
            h.clear_stats();
        }
    }

    /// JSON array of account ids active in *other* windows.
    pub fn accounts_in_use_json(&self) -> String {
        let me = std::process::id();
        let ids: Vec<String> = self
            .db
            .as_ref()
            .and_then(|c| crate::db::live(c).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|i| i.pid != me)
            .filter_map(|i| i.account_id)
            .collect();
        serde_json::to_string(&ids).unwrap_or_default()
    }

    /// Tray pick: come to the front if it's ours, raise the window that has it, activate
    /// it here when idle, else open it in a new window.
    pub fn pick_account(&mut self, id: &str) {
        if self.active.as_deref() == Some(id) {
            self.out(Out::Activate);
            return;
        }
        let holder = self
            .db
            .as_ref()
            .and_then(|c| crate::db::holder(c, id).ok().flatten());
        if let Some(pid) = holder {
            crate::ipc::send_to(pid, "show");
            return;
        }
        if self.active.is_none() {
            self.activate(id);
            return;
        }
        let exe = std::env::current_exe().unwrap_or_else(|_| "megasip".into());
        match std::process::Command::new(exe)
            .args(["--account", id])
            .stdin(std::process::Stdio::null())
            .spawn()
        {
            // Reap it when it exits so it doesn't linger as a zombie.
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(e) => self.error(format!("Couldn't open a new window: {e}")),
        }
    }

    /// Make `id` this window's account, if no other window has it.
    pub fn activate(&mut self, id: &str) {
        let title = self.account_title(id);
        let got = match self.db.as_mut() {
            Some(c) => crate::db::acquire(c, id),
            None => {
                self.error("Can't activate an account without megasip.db");
                return;
            }
        };
        match got {
            Ok(Ok(())) => {}
            Ok(Err(pid)) => {
                self.error(format!(
                    "{title} is already active in another window (pid {pid})"
                ));
                return;
            }
            Err(e) => {
                self.error(format!("Couldn't activate {title}: {e}"));
                return;
            }
        }
        self.active = Some(id.to_string());
        self.changed(Changed::ActiveAccount);
        crate::logging::set_log_name(Some(&self.account_short_name(id)));
        let s = self.settings.clone();
        self.configure_with(&s);
        self.missed_count = 0;
        self.changed(Changed::MissedCount);
        self.load_history();
        self.refresh_status();
        self.refresh_identity();
    }

    pub fn deactivate_account(&mut self) {
        if self.active.take().is_none() {
            return;
        }
        if let Some(c) = &self.db {
            crate::db::release(c);
        }
        self.changed(Changed::ActiveAccount);
        crate::logging::set_log_name(None);
        let s = self.settings.clone();
        self.configure_with(&s);
        self.pending_dial = None;
        self.missed_count = 0;
        self.changed(Changed::MissedCount);
        self.load_history();
        self.refresh_status();
        self.refresh_identity();
    }

    /// Hang up, un-register, leave `megasip.db` and exit the process.
    pub fn quit(&self) {
        if let Some(c) = &self.db {
            crate::db::unregister_self(c);
        }
        self.send(Command::Shutdown);
        // Let BYE / un-REGISTER go out.
        std::thread::sleep(Duration::from_millis(700));
        crate::ipc::cleanup();
        std::process::exit(0);
    }

    // ----- internals -----

    /// `megasip -a foo --source <what> [number]` while this window has foo: send `<what>` on
    /// the next call, then go back to what the window had. Refused (with a message) while a
    /// call is up, since the new source would go out on that call too.
    fn switch_source(&mut self, arg: &str) -> bool {
        if self.source.as_ref().map(|s| s.arg()).as_deref() == Some(arg) {
            return true;
        }
        let src = match Source::parse(arg) {
            Ok(s) => s,
            Err(e) => {
                tracing::debug!("source {arg:?} doesn't parse: {e}");
                self.error(e);
                return false;
            }
        };
        if !self.calls.is_empty() {
            tracing::debug!(
                "not switching to {arg:?}: calls up {:?}",
                self.calls
                    .values()
                    .map(|c| format!("{}:{}", c.id, c.state))
                    .collect::<Vec<_>>()
            );
            self.error(format!(
                "Not switching to {}: a call is in progress and would get it too. Hang up and try again.",
                src.label()
            ));
            return false;
        }
        if self.restore_source.is_none() {
            self.restore_source = Some(self.source.clone());
        }
        self.use_source(Some(src));
        true
    }

    /// Send `src` instead of the microphone (None: the microphone), and show it.
    fn use_source(&mut self, src: Option<Source>) {
        if let Some(h) = &self.hub {
            h.set_source(src.clone());
        }
        self.source = src;
        self.changed(Changed::Source);
    }

    fn flush_pending_dial(&mut self) {
        if let Some(d) = self.pending_dial.take() {
            tracing::debug!("dialing pending {d:?}");
            self.out(Out::Activate);
            let acc = self.active.clone().unwrap_or_default();
            self.dial(&d, &acc);
        }
    }

    /// Reload this window's account's history from megasip.db.
    fn load_history(&mut self) {
        self.history = match (&self.db, &self.active) {
            (Some(c), Some(id)) => crate::db::history(c, id).unwrap_or_else(|e| {
                tracing::warn!("couldn't read call history: {e}");
                Vec::new()
            }),
            _ => Vec::new(),
        };
        self.changed(Changed::History);
    }

    fn refresh_status(&mut self) {
        let status = match &self.active {
            None if self.settings.accounts.is_empty() => {
                "No account — open Preferences to add one".to_string()
            }
            None => "Pick one from the tray or here".to_string(),
            // The account's name is shown next to this already.
            Some(id) => match self.accounts.iter().find(|a| &a.id == id) {
                Some(a) if a.state == "registered" => "Online".to_string(),
                Some(a) => a.text.clone(),
                None => "Starting…".to_string(),
            },
        };
        if status != self.status {
            self.status = status;
            self.changed(Changed::Status);
        }
    }

    /// Show this window as its account in the taskbar and tray (or as MegaSIP).
    fn refresh_identity(&mut self) {
        let acc = self
            .active
            .as_ref()
            .and_then(|id| self.settings.accounts.iter().find(|a| &a.id == id));
        let identity = accounticon::identity_for(acc);
        self.out(Out::Identity(identity));
    }

    /// Hand the engine `s` with only this window's account enabled.
    fn configure_with(&self, s: &Settings) {
        self.send(Command::Configure(Box::new(
            s.for_engine(self.active.as_deref()),
        )));
    }

    /// The active account was deleted or disabled: give it up.
    fn drop_vanished_account(&mut self) {
        let Some(id) = self.active.clone() else {
            return;
        };
        let still = self
            .settings
            .accounts
            .iter()
            .any(|a| a.id == id && a.enabled && !a.server.trim().is_empty());
        if !still {
            self.deactivate_account();
        }
    }

    fn account_short_name(&self, id: &str) -> String {
        self.settings
            .accounts
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.short_name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    fn account_title(&self, id: &str) -> String {
        self.settings
            .accounts
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.title())
            .unwrap_or_else(|| id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::unbounded_channel;

    fn call(id: u64, number: &str, state: &str) -> CallInfo {
        CallInfo {
            id,
            number: number.into(),
            state: state.into(),
            ..Default::default()
        }
    }

    fn contact(name: &str, number: &str) -> Contact {
        serde_json::from_value(serde_json::json!({"name": name, "number": number}))
            .expect("contact")
    }

    /// A controller with no db, no hub and no account; returns the engine's command queue.
    fn bare(contacts: Vec<Contact>) -> (Controller, UnboundedReceiver<Command>) {
        let (tx, rx) = unbounded_channel();
        let mut c =
            Controller::from_parts(Settings::default(), contacts, None, Some(tx), None, None);
        // The initial status.
        assert_eq!(c.take_outbox(), [Out::Changed(Changed::Status)]);
        (c, rx)
    }

    fn commands(rx: &mut UnboundedReceiver<Command>) -> Vec<String> {
        let mut v = Vec::new();
        while let Ok(c) = rx.try_recv() {
            v.push(match c {
                Command::Configure(_) => "Configure".to_string(),
                Command::Dial { account, number } => format!("Dial({account}, {number})"),
                Command::Answer(id) => format!("Answer({id})"),
                Command::Reject(id) => format!("Reject({id})"),
                Command::Hangup(id) => format!("Hangup({id})"),
                Command::HangupAll => "HangupAll".into(),
                Command::Hold(id, on) => format!("Hold({id}, {on})"),
                Command::Dtmf(id, d) => format!("Dtmf({id}, '{d}')"),
                Command::Transfer(id, t) => format!("Transfer({id}, {t})"),
                Command::AttendedTransfer(a, b) => format!("AttendedTransfer({a}, {b})"),
                Command::Conference(on) => format!("Conference({on})"),
                Command::Mute(on) => format!("Mute({on})"),
                Command::Shutdown => "Shutdown".into(),
            });
        }
        v
    }

    #[test]
    fn numbers_normalise_for_lookup() {
        assert_eq!(norm_number("+1 (555) 123-4567"), "+15551234567");
        assert_eq!(norm_number(" Bob "), "bob");
        assert_eq!(norm_number("12"), "12");
    }

    #[test]
    fn lookup_matches_exact_and_trailing_digits() {
        let (c, _) = bare(vec![
            contact("Alice", "+1 555 123 4567"),
            contact("Desk", "1002"),
        ]);
        assert_eq!(c.lookup("5551234567").as_deref(), Some("Alice"));
        assert_eq!(c.lookup("1002").as_deref(), Some("Desk"));
        // Short numbers need an exact match, not a suffix.
        assert_eq!(c.lookup("002"), None);
        assert_eq!(c.lookup(""), None);
    }

    #[test]
    fn dial_targets_strip_tel_and_callto() {
        assert_eq!(dial_target(" tel:+15551234 "), "+15551234");
        assert_eq!(dial_target("callto:1002"), "1002");
        assert_eq!(dial_target("sip:bob@example.com"), "sip:bob@example.com");
        assert_eq!(dial_target("callto:sip:bob@x"), "sip:bob@x");
    }

    #[test]
    fn status_without_accounts_points_to_preferences() {
        let (c, _) = bare(vec![]);
        assert_eq!(c.status_text(), "No account — open Preferences to add one");
    }

    #[test]
    fn incoming_call_gets_its_contact_name_and_is_announced() {
        let (mut c, _) = bare(vec![contact("Alice", "5551234567")]);
        c.on_event(Event::Incoming(call(7, "5551234567", "incoming")));
        let out = c.take_outbox();
        assert_eq!(
            out,
            [
                Out::Changed(Changed::Calls),
                Out::Incoming {
                    id: 7,
                    name: "Alice".into(),
                    number: "5551234567".into()
                },
                // bring_to_front is on by default.
                Out::Activate,
            ]
        );
        assert!(c.calls_json().contains("\"Alice\""));
    }

    #[test]
    fn missed_call_counts_and_last_call_ends_conference() {
        let (mut c, _) = bare(vec![]);
        c.on_event(Event::Incoming(call(1, "1002", "incoming")));
        c.set_conference(true);
        c.take_outbox();
        c.on_event(Event::Ended {
            info: call(1, "1002", "ended"),
            missed: true,
        });
        assert_eq!(c.missed_count(), 1);
        assert!(!c.conference());
        let out = c.take_outbox();
        assert!(out.contains(&Out::Missed {
            name: String::new(),
            number: "1002".into()
        }));
        assert!(out.contains(&Out::Changed(Changed::Conference)));
        assert_eq!(c.calls().count(), 0);
        c.seen_missed();
        assert_eq!(c.missed_count(), 0);
    }

    #[test]
    fn ipc_answer_picks_the_ringing_call_and_hangup_ends_all() {
        let (mut c, mut rx) = bare(vec![]);
        c.on_event(Event::Call(call(1, "1001", "connected")));
        c.on_event(Event::Incoming(call(2, "1002", "incoming")));
        c.on_ipc("answer");
        c.on_ipc("hangup");
        assert_eq!(commands(&mut rx), ["Answer(2)", "HangupAll"]);
    }

    #[test]
    fn ipc_unknown_and_show_raise_the_window() {
        let (mut c, _) = bare(vec![]);
        c.on_ipc("show");
        c.on_ipc("error Nope");
        assert_eq!(c.take_outbox(), [Out::Activate, Out::Error("Nope".into())]);
    }

    #[test]
    fn source_switch_is_refused_during_a_call() {
        let (mut c, mut rx) = bare(vec![]);
        c.on_event(Event::Call(call(1, "1001", "connected")));
        c.take_outbox();
        c.on_ipc("dial 1002\tpink");
        let out = c.take_outbox();
        assert!(matches!(&out[0], Out::Error(e) if e.starts_with("Not switching to")));
        assert_eq!(out[1], Out::Activate);
        assert!(commands(&mut rx).is_empty(), "no dial while refused");
    }

    #[test]
    fn source_handed_over_lasts_until_the_calls_end() {
        let (mut c, _) = bare(vec![]);
        c.on_ipc("source pink");
        assert_eq!(c.source_label().0, Source::parse("pink").unwrap().label());
        c.on_event(Event::Call(call(1, "1001", "connected")));
        c.on_event(Event::Ended {
            info: call(1, "1001", "ended"),
            missed: false,
        });
        assert_eq!(c.source_label().0, "", "back to the microphone");
    }

    #[test]
    fn start_up_without_account_dials_after_a_moment() {
        let (mut c, _) = bare(vec![]);
        c.start_up(StartRequest {
            dial: Some("1002".into()),
            ..Default::default()
        });
        assert_eq!(
            c.take_outbox(),
            [Out::Schedule {
                after: Duration::from_millis(1500),
                timer: Timer::StartupDial("1002".into())
            }]
        );
    }

    #[test]
    fn start_up_with_an_account_but_no_db_reports_and_does_not_dial() {
        let (mut c, mut rx) = bare(vec![]);
        c.start_up(StartRequest {
            account: Some("acc1".into()),
            dial: Some("1002".into()),
            error: Some("No account named \"x\"".into()),
        });
        let out = c.take_outbox();
        assert_eq!(
            out[0],
            Out::Error("Can't activate an account without megasip.db".into())
        );
        assert!(matches!(out[1], Out::Identity(_)));
        assert_eq!(out[2], Out::Error("No account named \"x\"".into()));
        assert_eq!(out.len(), 3);
        assert!(commands(&mut rx).is_empty());
    }

    #[test]
    fn mute_and_conference_reach_the_engine() {
        let (mut c, mut rx) = bare(vec![]);
        c.set_mute(true);
        c.set_conference(true);
        c.hold(3, true);
        c.dtmf(3, '5');
        assert!(c.muted() && c.conference());
        assert_eq!(
            commands(&mut rx),
            [
                "Mute(true)",
                "Conference(true)",
                "Hold(3, true)",
                "Dtmf(3, '5')"
            ]
        );
    }
}
