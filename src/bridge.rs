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
        fn save_settings(self: Pin<&mut Phone>, json: &QString) -> QString;
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

    cmd: Option<UnboundedSender<Command>>,
    hub: Option<Arc<AudioHub>>,
    settings: Settings,
    calls: BTreeMap<u64, CallInfo>,
    history: Vec<HistoryEntry>,
    contacts: Vec<Contact>,
    accounts: Vec<AccountStatus>,
}

fn autostart_path() -> std::path::PathBuf {
    dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from(".")).join("autostart/megasip.desktop")
}

fn qs(s: &str) -> QString {
    QString::from(s)
}

fn norm_number(n: &str) -> String {
    let digits: String = n.chars().filter(|c| c.is_ascii_digit() || *c == '+').collect();
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
        let history = store::load_history();
        let contacts = store::load_contacts();
        let hub = AudioHub::start();
        let (ev_tx, mut ev_rx) = tokio::sync::mpsc::unbounded_channel::<Event>();
        let cmd = engine::spawn(settings.clone(), hub.clone(), ev_tx);
        {
            let mut r = self.as_mut().rust_mut();
            r.cmd = Some(cmd);
            r.hub = Some(hub);
            r.history = history;
            r.contacts = contacts;
            r.settings = settings;
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
                let online = list.iter().filter(|a| a.state == "registered").count();
                let status = if list.is_empty() {
                    "No account — open Settings to add one".to_string()
                } else if online == list.len() {
                    if list.len() == 1 { "Online".to_string() } else { format!("{online} accounts online") }
                } else if let Some(e) = list.iter().find(|a| a.state == "error") {
                    format!("{}: {}", e.title, e.text)
                } else if let Some(r) = list.iter().find(|a| a.state == "registering") {
                    format!("{}: {}", r.title, r.text)
                } else {
                    format!("{online} of {} online", list.len())
                };
                let json = serde_json::to_string(&list).unwrap_or_default();
                self.as_mut().rust_mut().accounts = list;
                self.as_mut().set_accounts_json(qs(&json));
                self.as_mut().set_status_text(qs(&status));
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
                let now = chrono::Utc::now().timestamp();
                let duration = info.connected_at.map(|t| (now - t).max(0)).unwrap_or(0);
                let kind = if missed {
                    "missed"
                } else if info.direction == "in" {
                    "in"
                } else {
                    "out"
                };
                let entry = HistoryEntry {
                    id: crate::engine::msg::random_token(10),
                    number: info.number.clone(),
                    name: info.name.clone(),
                    account_id: info.account_id.clone(),
                    kind: kind.into(),
                    time: info.started,
                    duration,
                    reason: info.reason.clone(),
                };
                {
                    let mut r = self.as_mut().rust_mut();
                    r.history.push(entry);
                    let start = r.history.len().saturating_sub(1000);
                    r.history.drain(..start);
                    store::save_history(&r.history);
                }
                if missed {
                    let n = self.rust().missed_count + 1;
                    self.as_mut().set_missed_count(n);
                    self.as_mut().missed_call(qs(&info.name), qs(&info.number));
                }
                if self.rust().calls.is_empty() {
                    self.as_mut().set_conference(false);
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
        if let Some(target) = line.strip_prefix("dial ") {
            let t = target.trim().trim_start_matches("callto:").trim_start_matches("tel:").to_string();
            let t = if target.trim().starts_with("tel:") { t } else { target.trim().trim_start_matches("callto:").to_string() };
            self.as_mut().activate_requested();
            let acc = self.rust().settings.default_account.clone();
            self.as_mut().do_dial(&t, &acc);
        } else if line == "answer" {
            if let Some(id) = self.rust().calls.values().find(|c| c.state == "incoming").map(|c| c.id) {
                self.rust().send(Command::Answer(id));
            }
        } else if line == "hangup" {
            self.rust().send(Command::HangupAll);
        } else if line == "quit" {
            self.quit();
        } else {
            self.as_mut().activate_requested();
        }
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
        c.sort_by(|a, b| b.favorite.cmp(&a.favorite).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        let json = serde_json::to_string(&c).unwrap_or_default();
        self.as_mut().set_contacts_json(qs(&json));
    }

    fn publish_settings(mut self: Pin<&mut Self>) {
        let json = serde_json::to_string(&self.rust().settings).unwrap_or_default();
        self.as_mut().set_settings_json(qs(&json));
    }

    fn apply_settings(mut self: Pin<&mut Self>, mut s: Settings) -> Result<(), String> {
        s.normalize();
        s.save().map_err(|e| format!("Could not save settings: {e}"))?;
        self.rust().send(Command::Configure(Box::new(s.clone())));
        crate::logging::apply(&s);
        let dnd = s.dnd;
        let aa = s.auto_answer != "off";
        self.as_mut().rust_mut().settings = s;
        self.as_mut().set_dnd(dnd);
        self.as_mut().set_auto_answer(aa);
        self.as_mut().publish_settings();
        Ok(())
    }

    fn do_dial(mut self: Pin<&mut Self>, number: &str, account: &str) {
        let number = number.trim().to_string();
        if number.is_empty() {
            return;
        }
        self.rust().send(Command::Dial { account: account.to_string(), number: number.clone() });
        let mut s = self.rust().settings.clone();
        if s.last_number != number {
            s.last_number = number;
            let _ = s.save();
            self.as_mut().rust_mut().settings = s;
            self.as_mut().publish_settings();
        }
    }

    fn dial(self: Pin<&mut Self>, number: &QString, account: &QString) {
        self.do_dial(&number.to_string(), &account.to_string());
    }

    fn redial(self: Pin<&mut Self>) {
        let n = self.rust().settings.last_number.clone();
        let acc = self.rust().settings.default_account.clone();
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
        self.rust().send(Command::Transfer(id as u64, target.to_string()));
    }

    fn attended_transfer(self: Pin<&mut Self>, id: i64, other: i64) {
        self.rust().send(Command::AttendedTransfer(id as u64, other as u64));
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
        if let Err(e) = self.as_mut().apply_settings(s) {
            self.as_mut().error_message(qs(&e));
        }
    }

    fn set_auto_answer_on(mut self: Pin<&mut Self>, on: bool) {
        let mut s = self.rust().settings.clone();
        s.auto_answer = if on { "all".into() } else { "off".into() };
        if let Err(e) = self.as_mut().apply_settings(s) {
            self.as_mut().error_message(qs(&e));
        }
    }

    fn save_settings(self: Pin<&mut Self>, json: &QString) -> QString {
        match serde_json::from_str::<Settings>(&json.to_string()) {
            Ok(s) => match self.apply_settings(s) {
                Ok(()) => QString::default(),
                Err(e) => qs(&e),
            },
            Err(e) => qs(&format!("Invalid settings: {e}")),
        }
    }

    fn new_account_json(&self) -> QString {
        let a = Account { id: crate::engine::msg::random_token(10), ..Default::default() };
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
        let Ok(mut c) = serde_json::from_str::<Contact>(&json.to_string()) else { return };
        if c.number.trim().is_empty() {
            return;
        }
        if c.id.is_empty() {
            c.id = crate::engine::msg::random_token(10);
        }
        {
            let mut r = self.as_mut().rust_mut();
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
            let mut r = self.as_mut().rust_mut();
            r.history.retain(|h| h.id != id);
            store::save_history(&r.history);
        }
        self.as_mut().publish_history();
    }

    fn clear_history(mut self: Pin<&mut Self>) {
        {
            let mut r = self.as_mut().rust_mut();
            r.history.clear();
            store::save_history(&r.history);
        }
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
        self.rust().hub.as_ref().map(|h| h.levels().0 as f64).unwrap_or(0.0)
    }

    fn speaker_level(&self) -> f64 {
        self.rust().hub.as_ref().map(|h| h.levels().1 as f64).unwrap_or(0.0)
    }

    fn autostart_enabled(&self) -> bool {
        autostart_path().exists()
    }

    fn set_autostart(mut self: Pin<&mut Self>, on: bool) {
        let p = autostart_path();
        let res = if on {
            let exe = std::env::current_exe().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|_| "megasip".into());
            let body = format!(
                "[Desktop Entry]\nType=Application\nName=MegaSIP\nExec={exe} --minimized\nIcon=call-start\nX-KDE-autostart-phase=2\nX-GNOME-Autostart-enabled=true\n"
            );
            p.parent().map(std::fs::create_dir_all).transpose().and_then(|_| std::fs::write(&p, body))
        } else {
            std::fs::remove_file(&p).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) })
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
        let (live, last) = self.rust().hub.as_ref().map(|h| h.stats()).unwrap_or_default();
        let (mic, spk) = self.rust().hub.as_ref().map(|h| h.levels()).unwrap_or_default();
        // Name each stream after its call.
        let calls = &self.rust().calls;
        let label = |id: u64| calls.get(&id).map(|c| if c.name.is_empty() { c.number.clone() } else { format!("{} ({})", c.name, c.number) });
        let named = |s: &crate::media::StreamStats| {
            let mut v = serde_json::to_value(s).unwrap_or_default();
            v["label"] = serde_json::Value::String(label(s.id).unwrap_or_else(|| format!("Call {}", s.id)));
            v
        };
        let json = serde_json::json!({
            "streams": live.iter().map(named).collect::<Vec<_>>(),
            "last": last.as_ref().map(named),
            "mic": mic,
            "speaker": spk,
        });
        qs(&json.to_string())
    }

    fn debug_clear(&self) {
        crate::debug::clear();
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

    fn quit(self: Pin<&mut Self>) {
        self.rust().send(Command::Shutdown);
        // Let BYE / un-REGISTER go out.
        std::thread::sleep(std::time::Duration::from_millis(700));
        crate::ipc::cleanup();
        std::process::exit(0);
    }
}
