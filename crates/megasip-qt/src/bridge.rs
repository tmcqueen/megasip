//! The QObject the QML UI talks to: a thin adapter over the window `Controller` in
//! megasip-core. State goes to QML as JSON strings and plain properties; QML calls back
//! with invokables. Engine events, IPC requests and timers are queued onto the Qt thread
//! and handed to the controller there.

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
        /// Agent Training's board while a connected agent has it open (JSON; "" until then).
        #[qproperty(QString, training_json)]
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
        /// The file:// URL of an account icon (written if needed), "" if it can't be.
        #[qinvokable]
        fn account_icon(
            self: &Phone,
            icon: &QString,
            color: &QString,
            name: &QString,
            dim: bool,
        ) -> QString;
        /// The glyphs an account icon can use, as JSON [{key, name}].
        #[qinvokable]
        fn account_icon_glyphs(self: &Phone) -> QString;

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

use crate::controller::{self, Changed, Controller, Out, StartRequest};
use crate::mcp::{Exec, Job, McpState};
use crate::media::hub;
use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;

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
    training_json: QString,

    /// Set in `initialize`, before QML can call anything.
    ctl: Option<Controller>,
    /// What connected agents' tools keep beside the controller (see `mcp`).
    mcp: McpState,
}

fn qs(s: &str) -> QString {
    QString::from(s)
}

impl cxx_qt::Initialize for qobject::Phone {
    fn initialize(mut self: Pin<&mut Self>) {
        let (ctl, mut ev_rx) = Controller::start();
        self.as_mut().rust_mut().ctl = Some(ctl);
        for c in [
            Changed::Settings,
            Changed::History,
            Changed::Contacts,
            Changed::Calls,
            Changed::Source,
            Changed::Status,
        ] {
            self.as_mut().apply_changed(c);
        }
        self.as_mut().refresh_devices();
        // Errors from start-up (megasip.db can't be opened) wait for QML, like the rest.
        let early = self
            .as_mut()
            .rust_mut()
            .ctl
            .as_mut()
            .map(|c| c.take_outbox());
        let req = StartRequest {
            account: crate::START_ACCOUNT.get().cloned(),
            dial: crate::START_DIAL.get().cloned(),
            error: crate::START_ERROR.get().cloned(),
        };
        tracing::debug!(
            "window start-up: account {:?}, dial {:?}",
            req.account,
            req.dial
        );
        // Activate once QML is listening, so a lost race ("already active") is shown.
        let _ = self.qt_thread().queue(move |mut p| {
            for o in early.unwrap_or_default() {
                p.as_mut().apply(o);
            }
            p.run(|c| c.start_up(req));
        });

        // Settings and contacts saved by other windows.
        let qt = self.qt_thread();
        controller::watch_shared_files(move |s, c| {
            qt.queue(move |p| p.run(|ctl| ctl.reload_external(s, c)))
                .is_ok()
        });

        // Engine events -> Qt thread.
        let qt = self.qt_thread();
        std::thread::Builder::new()
            .name("megasip-events".into())
            .spawn(move || {
                while let Some(ev) = ev_rx.blocking_recv() {
                    if qt.queue(move |p| p.engine_event(ev)).is_err() {
                        break;
                    }
                }
            })
            .expect("event thread");

        // Agents (`megasip --mcp -a <this window's account>`) -> Qt thread.
        let qt = std::sync::Mutex::new(self.qt_thread());
        let exec: Exec = std::sync::Arc::new(move |job: Job| {
            qt.lock()
                .map(|q| q.queue(move |p| p.mcp_job(job)).is_ok())
                .unwrap_or(false)
        });
        crate::mcp::serve_window(exec);
        // Call quality samples for the agents' reports.
        let qt = self.qt_thread();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            if qt
                .queue(|p| p.mcp_job(Box::new(|ctl, st| st.sample(ctl))))
                .is_err()
            {
                break;
            }
        });

        // Second instances / tel: links -> Qt thread.
        if let Some(rx) = crate::ipc::take_receiver() {
            tracing::debug!("ipc: window is reading requests");
            let qt = self.qt_thread();
            std::thread::spawn(move || {
                while let Ok(line) = rx.recv() {
                    tracing::debug!("ipc: queueing {line:?} for the Qt thread");
                    if qt.queue(move |p| p.run(|c| c.on_ipc(&line))).is_err() {
                        tracing::debug!("ipc: Qt thread is gone; no more requests");
                        break;
                    }
                }
            });
        } else {
            tracing::warn!(
                "ipc: no socket listener, so other megasip commands can't reach this window"
            );
        }
    }
}

impl qobject::Phone {
    fn ctl(&self) -> &Controller {
        self.rust()
            .ctl
            .as_ref()
            .expect("controller is set in initialize")
    }

    /// Call into the controller, then do what it asked for.
    fn run<R>(mut self: Pin<&mut Self>, f: impl FnOnce(&mut Controller) -> R) -> R {
        let (r, outbox) = {
            let mut r = self.as_mut().rust_mut();
            let c = r.ctl.as_mut().expect("controller is set in initialize");
            let res = f(c);
            (res, c.take_outbox())
        };
        for o in outbox {
            self.as_mut().apply(o);
        }
        r
    }

    /// An engine event: the agents' records first, then the controller.
    fn engine_event(mut self: Pin<&mut Self>, ev: crate::engine::Event) {
        {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            if let Some(ctl) = r.ctl.as_ref() {
                r.mcp.observe(&ev, ctl);
            }
        }
        self.run(|c| c.on_event(ev));
    }

    /// Run an agent's tool job on the controller, then do what it asked for.
    fn mcp_job(mut self: Pin<&mut Self>, job: Job) {
        let (outbox, board) = {
            let mut r = self.as_mut().rust_mut();
            let r = &mut *r;
            let Some(ctl) = r.ctl.as_mut() else { return };
            job(ctl, &mut r.mcp);
            (ctl.take_outbox(), r.mcp.take_training_change())
        };
        for o in outbox {
            self.as_mut().apply(o);
        }
        if let Some(b) = board {
            self.as_mut().set_training_json(qs(&b));
        }
    }

    fn apply(mut self: Pin<&mut Self>, o: Out) {
        self.as_mut().rust_mut().mcp.observe_out(&o);
        match o {
            Out::Changed(c) => self.apply_changed(c),
            Out::Incoming { id, name, number } => {
                self.incoming_call(id as i64, qs(&name), qs(&number))
            }
            Out::Missed { name, number } => self.missed_call(qs(&name), qs(&number)),
            Out::Error(e) => self.error_message(qs(&e)),
            Out::Activate => self.activate_requested(),
            Out::Dtmf { id, digit } => self.dtmf_received(id as i64, qs(&digit.to_string())),
            Out::Identity(i) => crate::appcpp::ffi::set_app_identity(&i.app_id, &i.icon_str()),
            Out::Schedule { after, timer } => {
                let qt = self.qt_thread();
                std::thread::spawn(move || {
                    std::thread::sleep(after);
                    let _ = qt.queue(move |p| p.run(|c| c.on_timer(timer)));
                });
            }
        }
    }

    /// Copy one piece of the controller's state into the QML properties.
    fn apply_changed(mut self: Pin<&mut Self>, c: Changed) {
        let ctl = self.ctl();
        match c {
            Changed::Accounts => {
                let v = qs(&ctl.accounts_json());
                self.as_mut().set_accounts_json(v);
            }
            Changed::Calls => {
                let v = qs(&ctl.calls_json());
                self.as_mut().set_calls_json(v);
            }
            Changed::History => {
                let v = qs(&ctl.history_json());
                self.as_mut().set_history_json(v);
            }
            Changed::Contacts => {
                let v = qs(&ctl.contacts_json());
                self.as_mut().set_contacts_json(v);
            }
            Changed::Settings => {
                let (v, dnd, aa) = (qs(&ctl.settings_json()), ctl.dnd(), ctl.auto_answer());
                self.as_mut().set_settings_json(v);
                self.as_mut().set_dnd(dnd);
                self.as_mut().set_auto_answer(aa);
            }
            Changed::Status => {
                let v = qs(ctl.status_text());
                self.as_mut().set_status_text(v);
            }
            Changed::ActiveAccount => {
                let v = qs(ctl.active_account().unwrap_or_default());
                self.as_mut().set_active_account(v);
            }
            Changed::MissedCount => {
                let v = ctl.missed_count();
                self.as_mut().set_missed_count(v);
            }
            Changed::Source => {
                let (label, detail) = ctl.source_label();
                self.as_mut().set_source_label(qs(&label));
                self.as_mut().set_source_detail(qs(&detail));
            }
            Changed::Muted => {
                let v = ctl.muted();
                self.as_mut().set_muted(v);
            }
            Changed::Conference => {
                let v = ctl.conference();
                self.as_mut().set_conference(v);
            }
        }
    }

    fn dial(self: Pin<&mut Self>, number: &QString, account: &QString) {
        let (n, a) = (number.to_string(), account.to_string());
        self.run(|c| c.dial(&n, &a));
    }

    fn redial(self: Pin<&mut Self>) {
        self.run(|c| c.redial());
    }

    fn answer(self: Pin<&mut Self>, id: i64) {
        self.run(|c| c.answer(id as u64));
    }

    fn reject(self: Pin<&mut Self>, id: i64) {
        self.run(|c| c.reject(id as u64));
    }

    fn hangup(self: Pin<&mut Self>, id: i64) {
        self.run(|c| c.hangup(id as u64));
    }

    fn hangup_all(self: Pin<&mut Self>) {
        self.run(|c| c.hangup_all());
    }

    fn hold(self: Pin<&mut Self>, id: i64, on: bool) {
        self.run(|c| c.hold(id as u64, on));
    }

    fn dtmf(self: Pin<&mut Self>, id: i64, digit: &QString) {
        if let Some(d) = digit.to_string().chars().next() {
            self.run(|c| c.dtmf(id as u64, d));
        }
    }

    fn transfer(self: Pin<&mut Self>, id: i64, target: &QString) {
        let t = target.to_string();
        self.run(|c| c.transfer(id as u64, &t));
    }

    fn attended_transfer(self: Pin<&mut Self>, id: i64, other: i64) {
        self.run(|c| c.attended_transfer(id as u64, other as u64));
    }

    fn set_conference_on(self: Pin<&mut Self>, on: bool) {
        self.run(|c| c.set_conference(on));
    }

    fn set_mute_on(self: Pin<&mut Self>, on: bool) {
        self.run(|c| c.set_mute(on));
    }

    fn set_dnd_on(self: Pin<&mut Self>, on: bool) {
        self.run(|c| c.set_dnd(on));
    }

    fn set_auto_answer_on(self: Pin<&mut Self>, on: bool) {
        self.run(|c| c.set_auto_answer(on));
    }

    fn save_settings(self: Pin<&mut Self>, base: &QString, json: &QString) -> QString {
        let (b, j) = (base.to_string(), json.to_string());
        qs(&self.run(|c| c.save_settings_json(&b, &j)))
    }

    fn new_account_json(&self) -> QString {
        qs(&self.ctl().new_account_json())
    }

    fn refresh_devices(self: Pin<&mut Self>) {
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            let (outs, ins) = hub::list_devices();
            let json = serde_json::json!({ "outputs": outs, "inputs": ins }).to_string();
            let _ = qt.queue(move |mut p| p.as_mut().set_devices_json(QString::from(&json)));
        });
    }

    fn save_contact(self: Pin<&mut Self>, json: &QString) {
        let j = json.to_string();
        self.run(|c| c.save_contact_json(&j));
    }

    fn delete_contact(self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        self.run(|c| c.delete_contact(&id));
    }

    fn import_contacts(self: Pin<&mut Self>, path: &QString) -> i32 {
        let p = path.to_string();
        self.run(|c| c.import_contacts(&p))
    }

    fn export_contacts(self: Pin<&mut Self>, path: &QString) -> bool {
        self.ctl().export_contacts(&path.to_string())
    }

    fn delete_history(self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        self.run(|c| c.delete_history(&id));
    }

    fn clear_history(self: Pin<&mut Self>) {
        self.run(|c| c.clear_history());
    }

    fn seen_missed(self: Pin<&mut Self>) {
        self.run(|c| c.seen_missed());
    }

    fn contact_name(&self, number: &QString) -> QString {
        qs(&self.ctl().lookup(&number.to_string()).unwrap_or_default())
    }

    fn mic_level(&self) -> f64 {
        self.ctl().mic_level()
    }

    fn speaker_level(&self) -> f64 {
        self.ctl().speaker_level()
    }

    fn autostart_enabled(&self) -> bool {
        self.ctl().autostart_enabled()
    }

    fn set_autostart(self: Pin<&mut Self>, on: bool) {
        self.run(|c| c.set_autostart(on));
    }

    fn debug_sip(&self, since: i64) -> QString {
        qs(&crate::debug::sip_since(since.max(0) as u64))
    }

    fn debug_logs(&self, since: i64) -> QString {
        qs(&crate::debug::logs_since(since.max(0) as u64))
    }

    fn debug_quality(&self) -> QString {
        qs(&self.ctl().debug_quality_json())
    }

    fn debug_clear(&self) {
        self.ctl().debug_clear();
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
        qs(&self.ctl().accounts_in_use_json())
    }

    fn pick_account(self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        self.run(|c| c.pick_account(&id));
    }

    fn deactivate_account(self: Pin<&mut Self>) {
        self.run(|c| c.deactivate_account());
    }

    fn account_icon(&self, icon: &QString, color: &QString, name: &QString, dim: bool) -> QString {
        let icon = icon.to_string();
        if !crate::accounticon::is_glyph(&icon) {
            return QString::default();
        }
        match crate::accounticon::write_icon(&icon, &color.to_string(), &name.to_string(), dim) {
            Ok(p) => qs(&crate::accounticon::file_url(&p)),
            Err(e) => {
                tracing::warn!("couldn't write an account icon: {e}");
                QString::default()
            }
        }
    }

    fn account_icon_glyphs(&self) -> QString {
        let list: Vec<serde_json::Value> = crate::accounticon::GLYPHS
            .iter()
            .map(|(k, n)| serde_json::json!({ "key": k, "name": n }))
            .collect();
        qs(&serde_json::Value::Array(list).to_string())
    }

    fn quit(self: Pin<&mut Self>) {
        self.ctl().quit();
    }
}
