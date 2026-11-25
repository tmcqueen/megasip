//! The QObject the QML UI talks to. State goes to QML as JSON strings; QML calls back
//! with invokables. Engine events arrive on a background thread and are queued onto the
//! Qt thread.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, accounts_json)]
        #[qproperty(QString, calls_json)]
        #[qproperty(QString, history_json)]
        #[qproperty(QString, contacts_json)]
        #[qproperty(QString, settings_json)]
        #[qproperty(QString, devices_json)]
        #[qproperty(QString, status_text)]
        #[qproperty(bool, muted)]
        #[qproperty(bool, conference)]
        #[qproperty(bool, dnd)]
        #[qproperty(bool, auto_answer)]
        #[qproperty(i32, missed_count)]
        /// Id of this window's active account ("" when none).
        #[qproperty(QString, active_account)]
        /// What `--source` sends instead of the microphone ("" when it's the microphone).
        #[qproperty(QString, source_label)]
        #[qproperty(QString, source_detail)]
        type Phone = super::PhoneRust;

        #[qinvokable]
        fn dial(self: Pin<&mut Phone>, number: &QString, account: &QString);
        #[qinvokable]
        fn redial(self: Pin<&mut Phone>);
        #[qinvokable]
        fn answer(self: Pin<&mut Phone>, id: i64);
        #[qinvokable]
        fn reject(self: Pin<&mut Phone>, id: i64);
        #[qinvokable]
        fn hangup(self: Pin<&mut Phone>, id: i64);
        #[qinvokable]
        fn hangup_all(self: Pin<&mut Phone>);
        #[qinvokable]
        fn hold(self: Pin<&mut Phone>, id: i64, on: bool);
        #[qinvokable]
        fn dtmf(self: Pin<&mut Phone>, id: i64, digit: &QString);
        #[qinvokable]
        fn transfer(self: Pin<&mut Phone>, id: i64, target: &QString);
        #[qinvokable]
        fn attended_transfer(self: Pin<&mut Phone>, id: i64, other: i64);
        #[qinvokable]
        fn set_conference_on(self: Pin<&mut Phone>, on: bool);
        #[qinvokable]
        fn set_mute_on(self: Pin<&mut Phone>, on: bool);
        #[qinvokable]
        fn set_dnd_on(self: Pin<&mut Phone>, on: bool);
        #[qinvokable]
        fn set_auto_answer_on(self: Pin<&mut Phone>, on: bool);
        #[qinvokable]
        /// Save the Preferences draft `json`, which was started from `base` (both JSON).
        fn save_settings(self: Pin<&mut Phone>, base: &QString, json: &QString) -> QString;
        #[qinvokable]
        fn new_account_json(self: &Phone) -> QString;
        #[qinvokable]
        fn refresh_devices(self: Pin<&mut Phone>);
        #[qinvokable]
        fn save_contact(self: Pin<&mut Phone>, json: &QString);
        #[qinvokable]
        fn delete_contact(self: Pin<&mut Phone>, id: &QString);
        #[qinvokable]
        fn import_contacts(self: Pin<&mut Phone>, path: &QString) -> i32;
        #[qinvokable]
        fn export_contacts(self: Pin<&mut Phone>, path: &QString) -> bool;
        #[qinvokable]
        fn delete_history(self: Pin<&mut Phone>, id: &QString);
        #[qinvokable]
        fn clear_history(self: Pin<&mut Phone>);
        #[qinvokable]
        fn seen_missed(self: Pin<&mut Phone>);
        #[qinvokable]
        fn contact_name(self: &Phone, number: &QString) -> QString;
        #[qinvokable]
        fn mic_level(self: &Phone) -> f64;
        #[qinvokable]
        fn speaker_level(self: &Phone) -> f64;
        #[qinvokable]
        fn autostart_enabled(self: &Phone) -> bool;
        #[qinvokable]
        fn set_autostart(self: Pin<&mut Phone>, on: bool);
        #[qinvokable]
        fn log_folder(self: &Phone) -> QString;
        #[qinvokable]
        fn debug_sip(self: &Phone, since: i64) -> QString;
        #[qinvokable]
        fn debug_logs(self: &Phone, since: i64) -> QString;
        #[qinvokable]
        fn debug_quality(self: &Phone) -> QString;
        #[qinvokable]
        fn debug_clear(self: &Phone);
        #[qinvokable]
        fn debug_set_capture(self: &Phone, level: &QString);
        #[qinvokable]
        fn save_text(self: &Phone, path: &QString, text: &QString) -> bool;
        #[qinvokable]
        fn quit(self: Pin<&mut Phone>);
        /// JSON array of account ids active in *other* windows.
        #[qinvokable]
        fn accounts_in_use(self: &Phone) -> QString;
        /// Tray pick: activate here when idle, else open it in a new window.
        #[qinvokable]
        fn pick_account(self: Pin<&mut Phone>, id: &QString);
        #[qinvokable]
        fn deactivate_account(self: Pin<&mut Phone>);

        #[qsignal]
        fn incoming_call(self: Pin<&mut Phone>, id: i64, name: QString, number: QString);
        #[qsignal]
        fn missed_call(self: Pin<&mut Phone>, name: QString, number: QString);
        #[qsignal]
        fn error_message(self: Pin<&mut Phone>, text: QString);
        #[qsignal]
        fn activate_requested(self: Pin<&mut Phone>);
        #[qsignal]
        fn dtmf_received(self: Pin<&mut Phone>, id: i64, digit: QString);
    }

    impl cxx_qt::Threading for Phone {}
    impl cxx_qt::Initialize for Phone {}
}

use crate::config::{Account, Settings};
use crate::engine::{self, AccountStatus, CallInfo, Command, Event};
use crate::media::hub::{self, AudioHub};
use crate::store::{self, Contact, HistoryEntry};
use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

#[derive(Default)]
pub struct PhoneRust {
    accounts_json: QString,
    calls_json: QString,
    history_json: QString,
    contacts_json: QString,
    settings_json: QString,
    devices_json: QString,
    status_text: QString,
    muted: bool,
    conference: bool,
    dnd: bool,
    auto_answer: bool,
    missed_count: i32,
    active_account: QString,
    source_label: QString,
    source_detail: QString,

    db: Option<rusqlite::Connection>,
    /// The account this window has active (the lock in `db` is ours).
    active: Option<String>,
    /// `megasip -a foo 1234`: dial once foo is registered.
    pending_dial: Option<String>,
    /// What's sent instead of the microphone (`--source`), if anything.
    source: Option<crate::media::source::Source>,
    /// A source handed over with a call (`megasip -a foo --source x 123` while this window
    /// has foo) lasts for that call: this is what to go back to once the calls are over.
    restore_source: Option<Option<crate::media::source::Source>>,
    cmd: Option<UnboundedSender<Command>>,
    hub: Option<Arc<AudioHub>>,
    settings: Settings,
    calls: BTreeMap<u64, CallInfo>,
    history: Vec<HistoryEntry>,
    contacts: Vec<Contact>,
    accounts: Vec<AccountStatus>,
}

fn autostart_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("autostart/megasip.desktop")
}

fn qs(s: &str) -> QString {
    QString::from(s)
}

fn norm_number(n: &str) -> String {
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

impl PhoneRust {
    fn lookup(&self, number: &str) -> Option<String> {
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

    fn send(&self, c: Command) {
        if let Some(tx) = &self.cmd {
            let _ = tx.send(c);
        }
    }
}

impl cxx_qt::Initialize for qobject::Phone {
    fn initialize(mut self: Pin<&mut Self>) {
        let settings = Settings::load();
        let contacts = store::load_contacts();
        let hub = AudioHub::start();
        if let Some(src) = crate::SOURCE.get() {
            hub.set_source(Some(src.clone()));
            self.as_mut().set_source_label(qs(&src.label()));
            self.as_mut().set_source_detail(qs(&src.detail()));
            self.as_mut().rust_mut().source = Some(src.clone());
        }
        let db = match crate::db::open() {
            Ok(c) => {
                if let Err(e) = crate::db::register_self(&c) {
                    tracing::warn!("couldn't list this window in megasip.db: {e}");
                }
                Some(c)
            }
            Err(e) => {
                let msg = format!("Can't open megasip.db ({e}): accounts can't be activated and call history isn't saved");
                let _ = self
                    .qt_thread()
                    .queue(move |mut p| p.as_mut().error_message(QString::from(&msg)));
                None
            }
        };
        let (ev_tx, mut ev_rx) = tokio::sync::mpsc::unbounded_channel::<Event>();
        let cmd = engine::spawn(settings.for_engine(None), hub.clone(), ev_tx);
        {
            let mut r = self.as_mut().rust_mut();
            r.cmd = Some(cmd);
            r.hub = Some(hub);
            r.contacts = contacts;
            r.settings = settings;
            r.db = db;
        }
        let dnd = self.rust().settings.dnd;
        let aa = self.rust().settings.auto_answer != "off";
        self.as_mut().set_dnd(dnd);
        self.as_mut().set_auto_answer(aa);
        self.as_mut().publish_settings();
        self.as_mut().publish_history();
        self.as_mut().publish_contacts();
        self.as_mut().publish_calls();
        self.as_mut().refresh_devices();

        self.as_mut().update_status();
        // Activate once QML is listening, so a lost race ("already active") is shown.
        let start_account = crate::START_ACCOUNT.get().cloned();
        let start_dial = crate::START_DIAL.get().cloned();
        let _ = self
            .qt_thread()
            .queue(move |mut p| p.as_mut().start_up(start_account, start_dial));
        if let Some(e) = crate::START_ERROR.get() {
            let e = e.clone();
            let _ = self
                .qt_thread()
                .queue(move |mut p| p.as_mut().error_message(QString::from(&e)));
        }
        // Settings and contacts saved by other windows.
        {
            let qt = self.qt_thread();
            std::thread::Builder::new()
                .name("megasip-watch".into())
                .spawn(move || {
                    let mtime = |p: std::path::PathBuf| {
                        std::fs::metadata(p).and_then(|m| m.modified()).ok()
                    };
                    let paths = || {
                        (
                            Settings::path(),
                            crate::config::data_dir().join("contacts.json"),
                        )
                    };
                    let (sp, cp) = paths();
                    let (mut s0, mut c0) = (mtime(sp), mtime(cp));
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        let (sp, cp) = paths();
                        let (s1, c1) = (mtime(sp), mtime(cp));
                        let (sc, cc) = (s1 != s0, c1 != c0);
                        (s0, c0) = (s1, c1);
                        if (sc || cc) && qt.queue(move |p| p.reload_external(sc, cc)).is_err() {
                            break;
                        }
                    }
                })
                .expect("watch thread");
        }

        // A wallet problem at startup (locked, missing) shows up in the window.
        let secret_error = self.rust().settings.secret_error.clone();
        if !secret_error.is_empty() {
            let _ = self.qt_thread().queue(move |mut p| {
                p.as_mut()
                    .error_message(QString::from(&format!("Passwords: {secret_error}")))
            });
        }

        // Engine events -> Qt thread.
        let qt = self.qt_thread();
        std::thread::Builder::new()
            .name("megasip-events".into())
            .spawn(move || {
                while let Some(ev) = ev_rx.blocking_recv() {
                    if qt.queue(move |p| p.on_event(ev)).is_err() {
                        break;
                    }
                }
            })
            .expect("event thread");

        // Second instances / tel: links -> Qt thread.
        if let Some(rx) = crate::ipc::take_receiver() {
            let qt = self.qt_thread();
            std::thread::spawn(move || {
                while let Ok(line) = rx.recv() {
                    if qt.queue(move |p| p.on_ipc(line)).is_err() {
                        break;
                    }
                }
            });
        }
    }
}

impl qobject::Phone {
    fn on_event(mut self: Pin<&mut Self>, ev: Event) {
        match ev {
            Event::Accounts(list) => {
                let json = serde_json::to_string(&list).unwrap_or_default();
                let registered =
                    self.rust().active.as_ref().is_some_and(|id| {
                        list.iter().any(|a| &a.id == id && a.state == "registered")
                    });
                self.as_mut().rust_mut().accounts = list;
                self.as_mut().set_accounts_json(qs(&json));
                self.as_mut().update_status();
                if registered {
                    self.as_mut().flush_pending_dial();
                }
            }
            Event::Call(mut info) => {
                if info.name.is_empty() {
                    if let Some(n) = self.rust().lookup(&info.number) {
                        info.name = n;
                    }
                }
                self.as_mut().rust_mut().calls.insert(info.id, info);
                self.as_mut().publish_calls();
            }
            Event::Incoming(mut info) => {
                if info.name.is_empty() {
                    if let Some(n) = self.rust().lookup(&info.number) {
                        info.name = n;
                    }
                }
                let (id, name, number) = (info.id as i64, info.name.clone(), info.number.clone());
                self.as_mut().rust_mut().calls.insert(info.id, info);
                self.as_mut().publish_calls();
                self.as_mut().incoming_call(id, qs(&name), qs(&number));
                if self.rust().settings.bring_to_front {
                    self.as_mut().activate_requested();
                }
            }
            Event::Ended { mut info, missed } => {
                if info.name.is_empty() {
                    if let Some(n) = self.rust().lookup(&info.number) {
                        info.name = n;
                    }
                }
                self.as_mut().rust_mut().calls.remove(&info.id);
                // History belongs to the account; calls on the local identity aren't kept.
                if self.rust().active.as_deref() == Some(info.account_id.as_str()) {
                    let entry = store::history_entry(&info, missed);
                    let saved = self
                        .rust()
                        .db
                        .as_ref()
                        .map(|c| crate::db::add_history(c, &entry));
                    if let Some(Err(e)) = saved {
                        self.as_mut()
                            .error_message(qs(&format!("Couldn't save call history: {e}")));
                    }
                    self.as_mut().load_history();
                }
                if missed {
                    let n = self.rust().missed_count + 1;
                    self.as_mut().set_missed_count(n);
                    self.as_mut().missed_call(qs(&info.name), qs(&info.number));
                }
                if self.rust().calls.is_empty() {
                    self.as_mut().set_conference(false);
                    if let Some(prev) = self.as_mut().rust_mut().restore_source.take() {
                        self.as_mut().use_source(prev);
                    }
                }
                self.as_mut().publish_history();
                self.as_mut().publish_calls();
            }
            Event::Dtmf(id, d) => {
                self.as_mut().dtmf_received(id as i64, qs(&d.to_string()));
            }
            Event::Error(e) => {
                self.as_mut().error_message(qs(&e));
            }
        }
        if let Some(h) = self.rust().hub.clone() {
            for e in h.take_errors() {
                self.as_mut().error_message(qs(&e));
            }
        }
    }

    fn on_ipc(mut self: Pin<&mut Self>, line: String) {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("dial ") {
            let (target, source) = crate::ipc::split_dial(rest);
            if let Some(src) = source {
                if !self.as_mut().switch_source(src) {
                    self.as_mut().activate_requested();
                    return;
                }
            }
            let t = target
                .trim()
                .trim_start_matches("callto:")
                .trim_start_matches("tel:")
                .to_string();
            let t = if target.trim().starts_with("tel:") {
                t
            } else {
                target.trim().trim_start_matches("callto:").to_string()
            };
            self.as_mut().activate_requested();
            let acc = self.rust().active.clone().unwrap_or_default();
            self.as_mut().do_dial(&t, &acc);
        } else if line == "answer" {
            if let Some(id) = self
                .rust()
                .calls
                .values()
                .find(|c| c.state == "incoming")
                .map(|c| c.id)
            {
                self.rust().send(Command::Answer(id));
            }
        } else if line == "hangup" {
            self.rust().send(Command::HangupAll);
        } else if line == "quit" {
            self.quit();
        } else if let Some(e) = line.strip_prefix("error ") {
            self.as_mut().error_message(qs(e));
        } else if let Some(src) = line.strip_prefix("source ") {
            self.as_mut().switch_source(src.trim());
            self.as_mut().activate_requested();
        } else {
            self.as_mut().activate_requested();
        }
    }

    /// `megasip -a foo --source <what> [number]` while this window has foo: send `<what>` on
    /// the next call, then go back to what the window had. Refused (with a message) while a
    /// call is up, since the new source would go out on that call too.
    fn switch_source(mut self: Pin<&mut Self>, arg: &str) -> bool {
        if self.rust().source.as_ref().map(|s| s.arg()).as_deref() == Some(arg) {
            return true;
        }
        let src = match crate::media::source::Source::parse(arg) {
            Ok(s) => s,
            Err(e) => {
                self.as_mut().error_message(qs(&e));
                return false;
            }
        };
        if !self.rust().calls.is_empty() {
            self.as_mut().error_message(qs(&format!(
                "Not switching to {}: a call is in progress and would get it too. Hang up and try again.",
                src.label()
            )));
            return false;
        }
        {
            let mut r = self.as_mut().rust_mut();
            if r.restore_source.is_none() {
                let prev = r.source.clone();
                r.restore_source = Some(prev);
            }
        }
        self.as_mut().use_source(Some(src));
        true
    }

    /// Send `src` instead of the microphone (None: the microphone), and show it.
    fn use_source(mut self: Pin<&mut Self>, src: Option<crate::media::source::Source>) {
        if let Some(h) = &self.rust().hub {
            h.set_source(src.clone());
        }
        let (label, detail) = src
            .as_ref()
            .map(|s| (s.label(), s.detail()))
            .unwrap_or_default();
        self.as_mut().set_source_label(qs(&label));
        self.as_mut().set_source_detail(qs(&detail));
        self.as_mut().rust_mut().source = src;
    }

    fn publish_calls(mut self: Pin<&mut Self>) {
        let list: Vec<&CallInfo> = self.rust().calls.values().collect();
        let json = serde_json::to_string(&list).unwrap_or_default();
        self.as_mut().set_calls_json(qs(&json));
    }

    fn publish_history(mut self: Pin<&mut Self>) {
        let mut list: Vec<serde_json::Value> = Vec::new();
        for h in self.rust().history.iter().rev() {
            let mut v = serde_json::to_value(h).unwrap_or_default();
            if h.name.is_empty() {
                if let Some(n) = self.rust().lookup(&h.number) {
                    v["name"] = serde_json::Value::String(n);
                }
            }
            list.push(v);
        }
        let json = serde_json::to_string(&list).unwrap_or_default();
        self.as_mut().set_history_json(qs(&json));
    }

    fn publish_contacts(mut self: Pin<&mut Self>) {
        let mut c = self.rust().contacts.clone();
        c.sort_by(|a, b| {
            b.favorite
                .cmp(&a.favorite)
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        let json = serde_json::to_string(&c).unwrap_or_default();
        self.as_mut().set_contacts_json(qs(&json));
    }

    fn publish_settings(mut self: Pin<&mut Self>) {
        let json = serde_json::to_string(&self.rust().settings).unwrap_or_default();
        self.as_mut().set_settings_json(qs(&json));
    }

    /// Save `s`, a change of `base` (default: this window's settings), merged with whatever
    /// other windows saved meanwhile.
    fn apply_settings(
        mut self: Pin<&mut Self>,
        base: Option<Settings>,
        mut s: Settings,
    ) -> Result<(), String> {
        s.normalize();
        let mut base = base.unwrap_or_else(|| self.rust().settings.clone());
        base.normalize();
        let disk = Settings::load_beside(&self.rust().settings);
        let same = |a: &Settings, b: &Settings| {
            serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
        };
        if !same(&disk, &base) {
            s = crate::config::merge_settings(&base, &s, &disk);
            s.normalize();
        }
        s.validate()?;
        s.save()?;
        self.as_mut().configure_with(s.clone());
        crate::logging::apply(&s);
        let dnd = s.dnd;
        let aa = s.auto_answer != "off";
        self.as_mut().rust_mut().settings = s;
        self.as_mut().set_dnd(dnd);
        self.as_mut().set_auto_answer(aa);
        self.as_mut().publish_settings();
        self.as_mut().drop_vanished_account();
        Ok(())
    }

    /// Hand the engine `s` with only this window's account enabled.
    fn configure_with(self: Pin<&mut Self>, s: Settings) {
        let active = self.rust().active.clone();
        self.rust().send(Command::Configure(Box::new(
            s.for_engine(active.as_deref()),
        )));
    }

    /// The active account was deleted or disabled: give it up.
    fn drop_vanished_account(self: Pin<&mut Self>) {
        let Some(id) = self.rust().active.clone() else {
            return;
        };
        let still = self
            .rust()
            .settings
            .accounts
            .iter()
            .any(|a| a.id == id && a.enabled && !a.server.trim().is_empty());
        if !still {
            self.deactivate_account();
        }
    }

    fn account_short_name(&self, id: &str) -> String {
        self.rust()
            .settings
            .accounts
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.short_name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    fn account_title(&self, id: &str) -> String {
        self.rust()
            .settings
            .accounts
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.title())
            .unwrap_or_else(|| id.to_string())
    }

    /// Make `id` this window's account, if no other window has it.
    fn activate(mut self: Pin<&mut Self>, id: &str) {
        let title = self.account_title(id);
        let got = match self.as_mut().rust_mut().db.as_mut() {
            Some(c) => crate::db::acquire(c, id),
            None => {
                self.as_mut()
                    .error_message(qs("Can't activate an account without megasip.db"));
                return;
            }
        };
        match got {
            Ok(Ok(())) => {}
            Ok(Err(pid)) => {
                self.as_mut().error_message(qs(&format!(
                    "{title} is already active in another window (pid {pid})"
                )));
                return;
            }
            Err(e) => {
                self.as_mut()
                    .error_message(qs(&format!("Couldn't activate {title}: {e}")));
                return;
            }
        }
        self.as_mut().rust_mut().active = Some(id.to_string());
        self.as_mut().set_active_account(qs(id));
        crate::logging::set_log_name(Some(&self.account_short_name(id)));
        let s = self.rust().settings.clone();
        self.as_mut().configure_with(s);
        self.as_mut().set_missed_count(0);
        self.as_mut().load_history();
        self.as_mut().update_status();
    }

    /// Start-up requests from the command line: the `-a` account, then a number to dial.
    fn start_up(mut self: Pin<&mut Self>, account: Option<String>, dial: Option<String>) {
        if let Some(id) = &account {
            self.as_mut().activate(id);
        }
        let Some(d) = dial else { return };
        if account.is_some() {
            if self.rust().active.is_none() {
                // `-a` didn't get its account (another window won the race): don't call
                // from the wrong identity; hand the number to the window that has it.
                let holder = account.as_deref().and_then(|id| {
                    self.rust()
                        .db
                        .as_ref()
                        .and_then(|c| crate::db::holder(c, id).ok().flatten())
                });
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
            self.as_mut().rust_mut().pending_dial = Some(d);
            let qt = self.qt_thread();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(10));
                let _ = qt.queue(|p| p.flush_pending_dial());
            });
        } else {
            let qt = self.qt_thread();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(1500));
                let _ = qt.queue(move |mut p| {
                    p.as_mut().activate_requested();
                    let acc = p.rust().active.clone().unwrap_or_default();
                    p.do_dial(&d, &acc);
                });
            });
        }
    }

    fn flush_pending_dial(mut self: Pin<&mut Self>) {
        if let Some(d) = self.as_mut().rust_mut().pending_dial.take() {
            self.as_mut().activate_requested();
            let acc = self.rust().active.clone().unwrap_or_default();
            self.do_dial(&d, &acc);
        }
    }

    /// Reload this window's account's history from megasip.db.
    fn load_history(mut self: Pin<&mut Self>) {
        let h = match (&self.rust().db, &self.rust().active) {
            (Some(c), Some(id)) => crate::db::history(c, id).unwrap_or_else(|e| {
                tracing::warn!("couldn't read call history: {e}");
                Vec::new()
            }),
            _ => Vec::new(),
        };
        self.as_mut().rust_mut().history = h;
        self.as_mut().publish_history();
    }

    fn update_status(self: Pin<&mut Self>) {
        let status = match self.rust().active.clone() {
            None if self.rust().settings.accounts.is_empty() => {
                "No account — open Preferences to add one".to_string()
            }
            None => "Pick one from the tray or here".to_string(),
            // The account's name is shown next to this already.
            Some(id) => match self.rust().accounts.iter().find(|a| a.id == id) {
                Some(a) if a.state == "registered" => "Online".to_string(),
                Some(a) => a.text.clone(),
                None => "Starting…".to_string(),
            },
        };
        self.set_status_text(qs(&status));
    }

    /// Another window saved settings or contacts.
    fn reload_external(mut self: Pin<&mut Self>, settings: bool, contacts: bool) {
        if settings {
            let s = Settings::load_beside(&self.rust().settings);
            let same =
                serde_json::to_value(&s).ok() == serde_json::to_value(&self.rust().settings).ok();
            if !same {
                crate::logging::apply(&s);
                let (dnd, aa) = (s.dnd, s.auto_answer != "off");
                self.as_mut().rust_mut().settings = s.clone();
                self.as_mut().configure_with(s);
                self.as_mut().set_dnd(dnd);
                self.as_mut().set_auto_answer(aa);
                self.as_mut().publish_settings();
                self.as_mut().drop_vanished_account();
                self.as_mut().update_status();
            }
        }
        if contacts {
            let c = store::load_contacts();
            if c != self.rust().contacts {
                self.as_mut().rust_mut().contacts = c;
                self.as_mut().publish_contacts();
                self.as_mut().publish_history();
            }
        }
    }

    fn do_dial(mut self: Pin<&mut Self>, number: &str, account: &str) {
        let number = number.trim().to_string();
        if number.is_empty() {
            return;
        }
        self.rust().send(Command::Dial {
            account: account.to_string(),
            number: number.clone(),
        });
        let mut s = self.rust().settings.clone();
        if s.last_number != number {
            s.last_number = number;
            if let Err(e) = self.as_mut().apply_settings(None, s) {
                tracing::warn!("couldn't save the last number: {e}");
            }
        }
    }

    fn dial(self: Pin<&mut Self>, number: &QString, account: &QString) {
        self.do_dial(&number.to_string(), &account.to_string());
    }

    fn redial(self: Pin<&mut Self>) {
        let n = self.rust().settings.last_number.clone();
        let acc = self.rust().active.clone().unwrap_or_default();
        self.do_dial(&n, &acc);
    }

    fn answer(self: Pin<&mut Self>, id: i64) {
        self.rust().send(Command::Answer(id as u64));
    }

    fn reject(self: Pin<&mut Self>, id: i64) {
        self.rust().send(Command::Reject(id as u64));
    }

    fn hangup(self: Pin<&mut Self>, id: i64) {
        self.rust().send(Command::Hangup(id as u64));
    }

    fn hangup_all(self: Pin<&mut Self>) {
        self.rust().send(Command::HangupAll);
    }

    fn hold(self: Pin<&mut Self>, id: i64, on: bool) {
        self.rust().send(Command::Hold(id as u64, on));
    }

    fn dtmf(self: Pin<&mut Self>, id: i64, digit: &QString) {
        if let Some(d) = digit.to_string().chars().next() {
            self.rust().send(Command::Dtmf(id as u64, d));
        }
    }

    fn transfer(self: Pin<&mut Self>, id: i64, target: &QString) {
        self.rust()
            .send(Command::Transfer(id as u64, target.to_string()));
    }

    fn attended_transfer(self: Pin<&mut Self>, id: i64, other: i64) {
        self.rust()
            .send(Command::AttendedTransfer(id as u64, other as u64));
    }

    fn set_conference_on(mut self: Pin<&mut Self>, on: bool) {
        self.rust().send(Command::Conference(on));
        self.as_mut().set_conference(on);
    }

    fn set_mute_on(mut self: Pin<&mut Self>, on: bool) {
        self.rust().send(Command::Mute(on));
        self.as_mut().set_muted(on);
    }

    fn set_dnd_on(mut self: Pin<&mut Self>, on: bool) {
        let mut s = self.rust().settings.clone();
        s.dnd = on;
        if let Err(e) = self.as_mut().apply_settings(None, s) {
            self.as_mut().error_message(qs(&e));
        }
    }

    fn set_auto_answer_on(mut self: Pin<&mut Self>, on: bool) {
        let mut s = self.rust().settings.clone();
        s.auto_answer = if on { "all".into() } else { "off".into() };
        if let Err(e) = self.as_mut().apply_settings(None, s) {
            self.as_mut().error_message(qs(&e));
        }
    }

    fn save_settings(self: Pin<&mut Self>, base: &QString, json: &QString) -> QString {
        let base = serde_json::from_str::<Settings>(&base.to_string()).ok();
        match serde_json::from_str::<Settings>(&json.to_string()) {
            Ok(s) => match self.apply_settings(base, s) {
                Ok(()) => QString::default(),
                Err(e) => qs(&e),
            },
            Err(e) => qs(&format!("Invalid settings: {e}")),
        }
    }

    fn new_account_json(&self) -> QString {
        let a = Account {
            id: crate::engine::msg::random_token(10),
            ..Default::default()
        };
        qs(&serde_json::to_string(&a).unwrap_or_default())
    }

    fn refresh_devices(self: Pin<&mut Self>) {
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            let (outs, ins) = hub::list_devices();
            let json = serde_json::json!({ "outputs": outs, "inputs": ins }).to_string();
            let _ = qt.queue(move |mut p| p.as_mut().set_devices_json(QString::from(&json)));
        });
    }

    fn save_contact(mut self: Pin<&mut Self>, json: &QString) {
        let Ok(mut c) = serde_json::from_str::<Contact>(&json.to_string()) else {
            return;
        };
        if c.number.trim().is_empty() {
            return;
        }
        if c.id.is_empty() {
            c.id = crate::engine::msg::random_token(10);
        }
        {
            let mut r = self.as_mut().rust_mut();
            // Start from the file: another window may have changed it.
            r.contacts = store::load_contacts();
            match r.contacts.iter_mut().find(|x| x.id == c.id) {
                Some(x) => *x = c,
                None => r.contacts.push(c),
            }
            store::save_contacts(&r.contacts);
        }
        self.as_mut().publish_contacts();
        self.as_mut().publish_history();
    }

    fn delete_contact(mut self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        {
            let mut r = self.as_mut().rust_mut();
            // Start from the file: another window may have changed it.
            r.contacts = store::load_contacts();
            r.contacts.retain(|c| c.id != id);
            store::save_contacts(&r.contacts);
        }
        self.as_mut().publish_contacts();
        self.as_mut().publish_history();
    }

    fn import_contacts(mut self: Pin<&mut Self>, path: &QString) -> i32 {
        let p = path.to_string();
        let p = p.strip_prefix("file://").unwrap_or(&p).to_string();
        let new = store::import_contacts(&p);
        let n = new.len() as i32;
        {
            let mut r = self.as_mut().rust_mut();
            // Start from the file: another window may have changed it.
            r.contacts = store::load_contacts();
            for c in new {
                if !r.contacts.iter().any(|x| x.number == c.number) {
                    r.contacts.push(c);
                }
            }
            store::save_contacts(&r.contacts);
        }
        self.as_mut().publish_contacts();
        self.as_mut().publish_history();
        n
    }

    fn export_contacts(self: Pin<&mut Self>, path: &QString) -> bool {
        let p = path.to_string();
        let p = p.strip_prefix("file://").unwrap_or(&p).to_string();
        store::export_contacts_csv(&p, &self.rust().contacts).is_ok()
    }

    fn delete_history(mut self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        {
            if let Some(c) = &self.rust().db {
                crate::db::delete_history(c, &id);
            }
        }
        self.as_mut().load_history();
    }

    fn clear_history(mut self: Pin<&mut Self>) {
        {
            if let (Some(c), Some(id)) = (&self.rust().db, &self.rust().active) {
                crate::db::clear_history(c, id);
            }
        }
        self.as_mut().load_history();
        self.as_mut().set_missed_count(0);
        self.as_mut().publish_history();
    }

    fn seen_missed(self: Pin<&mut Self>) {
        self.set_missed_count(0);
    }

    fn contact_name(&self, number: &QString) -> QString {
        qs(&self.rust().lookup(&number.to_string()).unwrap_or_default())
    }

    fn mic_level(&self) -> f64 {
        self.rust()
            .hub
            .as_ref()
            .map(|h| h.levels().0 as f64)
            .unwrap_or(0.0)
    }

    fn speaker_level(&self) -> f64 {
        self.rust()
            .hub
            .as_ref()
            .map(|h| h.levels().1 as f64)
            .unwrap_or(0.0)
    }

    fn autostart_enabled(&self) -> bool {
        autostart_path().exists()
    }

    fn set_autostart(mut self: Pin<&mut Self>, on: bool) {
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
            self.as_mut().error_message(qs(&format!("Autostart: {e}")));
        }
    }

    fn debug_sip(&self, since: i64) -> QString {
        qs(&crate::debug::sip_since(since.max(0) as u64))
    }

    fn debug_logs(&self, since: i64) -> QString {
        qs(&crate::debug::logs_since(since.max(0) as u64))
    }

    fn debug_quality(&self) -> QString {
        let (live, last) = self
            .rust()
            .hub
            .as_ref()
            .map(|h| h.stats())
            .unwrap_or_default();
        let (mic, spk) = self
            .rust()
            .hub
            .as_ref()
            .map(|h| h.levels())
            .unwrap_or_default();
        // Name each stream after its call.
        let calls = &self.rust().calls;
        let label = |id: u64| {
            calls.get(&id).map(|c| {
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
        let json = serde_json::json!({
            "source": self.rust().source_label.to_string(),
            "streams": live.iter().map(named).collect::<Vec<_>>(),
            "last": last.as_ref().map(named),
            "mic": mic,
            "speaker": spk,
        });
        qs(&json.to_string())
    }

    fn debug_clear(&self) {
        crate::debug::clear();
        if let Some(h) = self.rust().hub.as_ref() {
            h.clear_stats();
        }
    }

    fn debug_set_capture(&self, level: &QString) {
        crate::logging::set_capture_level(&level.to_string());
    }

    fn save_text(&self, path: &QString, text: &QString) -> bool {
        let p = path.to_string();
        let p = p.strip_prefix("file://").unwrap_or(&p).to_string();
        std::fs::write(p, text.to_string()).is_ok()
    }

    fn log_folder(&self) -> QString {
        let d = crate::logging::log_dir();
        let _ = std::fs::create_dir_all(&d);
        qs(&d.to_string_lossy())
    }

    fn accounts_in_use(&self) -> QString {
        let me = std::process::id();
        let ids: Vec<String> = self
            .rust()
            .db
            .as_ref()
            .and_then(|c| crate::db::live(c).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|i| i.pid != me)
            .filter_map(|i| i.account_id)
            .collect();
        qs(&serde_json::to_string(&ids).unwrap_or_default())
    }

    fn pick_account(mut self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        if self.rust().active.as_deref() == Some(id.as_str()) {
            self.as_mut().activate_requested();
            return;
        }
        let holder = self
            .rust()
            .db
            .as_ref()
            .and_then(|c| crate::db::holder(c, &id).ok().flatten());
        if let Some(pid) = holder {
            crate::ipc::send_to(pid, "show");
            return;
        }
        if self.rust().active.is_none() {
            self.as_mut().activate(&id);
            return;
        }
        let exe = std::env::current_exe().unwrap_or_else(|_| "megasip".into());
        match std::process::Command::new(exe)
            .args(["--account", &id])
            .stdin(std::process::Stdio::null())
            .spawn()
        {
            // Reap it when it exits so it doesn't linger as a zombie.
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(e) => self
                .as_mut()
                .error_message(qs(&format!("Couldn't open a new window: {e}"))),
        }
    }

    fn deactivate_account(mut self: Pin<&mut Self>) {
        if self.as_mut().rust_mut().active.take().is_none() {
            return;
        }
        if let Some(c) = &self.rust().db {
            crate::db::release(c);
        }
        self.as_mut().set_active_account(QString::default());
        crate::logging::set_log_name(None);
        let s = self.rust().settings.clone();
        self.as_mut().configure_with(s);
        self.as_mut().rust_mut().pending_dial = None;
        self.as_mut().set_missed_count(0);
        self.as_mut().load_history();
        self.as_mut().update_status();
    }

    fn quit(self: Pin<&mut Self>) {
        if let Some(c) = &self.rust().db {
            crate::db::unregister_self(c);
        }
        self.rust().send(Command::Shutdown);
        // Let BYE / un-REGISTER go out.
        std::thread::sleep(std::time::Duration::from_millis(700));
        crate::ipc::cleanup();
        std::process::exit(0);
    }
}
