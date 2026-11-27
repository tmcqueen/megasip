//! The SIP user agent. Runs on its own tokio runtime thread; the UI talks to it with
//! [`Command`]s and hears back through [`Event`]s.

pub mod call;
pub mod msg;
pub mod sdp;
pub mod stun;
pub mod transport;

use crate::config::{Account, Settings};
use crate::media::hub::{AudioConfig, AudioHub};
use crate::media::StreamEvent;
use msg::{new_branch, param, random_token, SipMsg};
use rsiprtp::sip::{DigestChallenge, DigestCredentials, DigestResponse};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use transport::{Flow, Proto, TransportEvent, Transports};

pub type CallId = u64;

pub const ALLOW: &str = "INVITE, ACK, CANCEL, BYE, OPTIONS, INFO, NOTIFY, REFER, UPDATE";
pub const T1: Duration = Duration::from_millis(500);
pub const T2: Duration = Duration::from_secs(4);
const TXN_TIMEOUT: Duration = Duration::from_secs(32);

pub enum Command {
    Configure(Box<Settings>),
    Dial { account: String, number: String },
    Answer(CallId),
    Reject(CallId),
    Hangup(CallId),
    HangupAll,
    Hold(CallId, bool),
    Dtmf(CallId, char),
    Transfer(CallId, String),
    AttendedTransfer(CallId, CallId),
    Conference(bool),
    Mute(bool),
    Shutdown,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    pub id: String,
    pub title: String,
    /// "off" | "registering" | "registered" | "error"
    pub state: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CallInfo {
    pub id: CallId,
    pub account_id: String,
    /// "in" | "out"
    pub direction: String,
    pub number: String,
    pub name: String,
    pub uri: String,
    /// "calling" | "ringing" | "early" | "incoming" | "connected" | "ended"
    pub state: String,
    pub held: bool,
    pub remote_held: bool,
    pub codec: String,
    pub secure: bool,
    pub started: i64,
    pub connected_at: Option<i64>,
    pub reason: String,
    pub transfer: String,
    /// The SIP Call-ID of the dialog, for matching the SIP trace.
    pub sip_call_id: String,
}

pub enum Event {
    Accounts(Vec<AccountStatus>),
    Call(CallInfo),
    /// The call is over; `missed` for unanswered incoming calls.
    Ended {
        info: CallInfo,
        missed: bool,
    },
    Incoming(CallInfo),
    Dtmf(CallId, char),
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum RegState {
    Off,
    Resolving,
    Registering,
    Registered,
    Error(String),
}

pub(crate) struct Acc {
    pub cfg: Account,
    pub proto: Proto,
    pub dest: Option<SocketAddr>,
    pub flow: Flow,
    /// Our address as the server should reach us (Contact/Via).
    pub contact_addr: Option<SocketAddr>,
    pub call_id: String,
    pub cseq: u32,
    pub from_tag: String,
    pub state: RegState,
    pub refresh_at: Option<Instant>,
    pub retry_at: Option<Instant>,
    pub register_txn: Option<String>,
    pub auth_tries: u32,
    pub challenge: Option<(DigestChallenge, bool)>,
    pub nc: u32,
    pub keepalive_at: Instant,
    pub generation: u64,
    pub local_only: bool,
    pub unregistering: bool,
}

impl Acc {
    pub fn aor(&self) -> String {
        format!("sip:{}@{}", self.cfg.username, self.cfg.sip_domain())
    }

    pub fn contact(&self) -> String {
        let a = self
            .contact_addr
            .map(|a| a.to_string())
            .unwrap_or_else(|| "0.0.0.0:5060".into());
        let user = if self.cfg.username.is_empty() {
            "megasip".to_string()
        } else {
            self.cfg.username.clone()
        };
        format!("<sip:{}@{}{}>", user, a, self.proto.uri_param())
    }

    pub fn via_host(&self) -> String {
        self.contact_addr
            .map(|a| a.to_string())
            .unwrap_or_else(|| "0.0.0.0:5060".into())
    }

    pub fn auth_user(&self) -> String {
        if self.cfg.auth_id.trim().is_empty() {
            self.cfg.username.clone()
        } else {
            self.cfg.auth_id.trim().to_string()
        }
    }

    pub fn ready(&self) -> bool {
        self.dest.is_some() && self.contact_addr.is_some()
    }
}

#[derive(Debug, Clone)]
pub(crate) enum TxnOwner {
    Register(String),
    Call(CallId),
}

pub(crate) struct ClientTxn {
    pub req: SipMsg,
    pub bytes: Vec<u8>,
    pub flow: Flow,
    pub dest: SocketAddr,
    pub reliable: bool,
    pub started: Instant,
    pub next_retx: Instant,
    pub interval: Duration,
    pub provisional: bool,
    pub owner: TxnOwner,
    pub auth_tries: u32,
}

pub(crate) struct ServerTxn {
    pub last: Option<Vec<u8>>,
    pub flow: Flow,
    pub dest: SocketAddr,
    pub expires: Instant,
    /// For INVITE final responses over UDP: retransmit until ACK.
    pub retx: Option<(Instant, Duration)>,
    pub call_id: String,
    pub cseq: u32,
}

enum Internal {
    Resolved {
        acc: String,
        generation: u64,
        result: Option<(SocketAddr, Proto)>,
    },
    Connected {
        acc: String,
        generation: u64,
        result: Result<(transport::BoxStream, SocketAddr), String>,
        peer: SocketAddr,
        proto: Proto,
    },
    SipStun(Option<SocketAddr>),
}

pub struct Engine {
    pub(crate) settings: Settings,
    pub(crate) tr: Transports,
    pub(crate) hub: Arc<AudioHub>,
    pub(crate) accs: BTreeMap<String, Acc>,
    pub(crate) calls: BTreeMap<CallId, call::Call>,
    pub(crate) next_call: CallId,
    pub(crate) ctx: HashMap<String, ClientTxn>,
    pub(crate) stx: HashMap<(String, String), ServerTxn>,
    pub(crate) events: mpsc::UnboundedSender<Event>,
    pub(crate) stream_tx: mpsc::UnboundedSender<StreamEvent>,
    internal_tx: mpsc::UnboundedSender<Internal>,
    pub(crate) conference: bool,
    pub(crate) muted: bool,
    pub(crate) public_ip: Option<IpAddr>,
    stun_server: Option<SocketAddr>,
    stun_txid: [u8; 12],
    stun_at: Instant,
    pub(crate) user_agent: String,
    last_acc_status: Vec<(String, String, String)>,
}

/// Start the engine on its own thread. Returns the command sender.
pub fn spawn(
    settings: Settings,
    hub: Arc<AudioHub>,
    events: mpsc::UnboundedSender<Event>,
) -> mpsc::UnboundedSender<Command> {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    std::thread::Builder::new()
        .name("megasip-sip".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(run(settings, hub, events, cmd_rx));
        })
        .expect("sip thread");
    cmd_tx
}

async fn run(
    settings: Settings,
    hub: Arc<AudioHub>,
    events: mpsc::UnboundedSender<Event>,
    mut cmd_rx: mpsc::UnboundedReceiver<Command>,
) {
    let (tev_tx, mut tev_rx) = mpsc::unbounded_channel();
    let tr = loop {
        match Transports::new(settings.source_port, tev_tx.clone()).await {
            Ok(t) => break t,
            Err(e) => {
                let _ = events.send(Event::Error(format!("Cannot open SIP socket: {e}")));
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    };
    tracing::info!("SIP UDP on {}", tr.udp_local);
    let (stream_tx, mut stream_rx) = mpsc::unbounded_channel();
    let (internal_tx, mut internal_rx) = mpsc::unbounded_channel();
    let mut eng = Engine {
        settings: Settings::default(),
        tr,
        hub,
        accs: BTreeMap::new(),
        calls: BTreeMap::new(),
        next_call: 1,
        ctx: HashMap::new(),
        stx: HashMap::new(),
        events,
        stream_tx,
        internal_tx,
        conference: false,
        muted: false,
        public_ip: None,
        stun_server: None,
        stun_txid: [0; 12],
        stun_at: Instant::now(),
        user_agent: format!("MegaSIP/{}", env!("CARGO_PKG_VERSION")),
        last_acc_status: Vec::new(),
    };
    eng.configure(settings);
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // A bug triggered by one odd packet must not take the whole phone down: contain
    // panics to the message that caused them and keep serving.
    fn guarded(what: &str, f: impl FnOnce()) {
        if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()));
            tracing::error!(
                "internal error while handling {what}: {}",
                msg.unwrap_or_default()
            );
        }
    }
    loop {
        tokio::select! {
            c = cmd_rx.recv() => match c {
                Some(Command::Shutdown) | None => { eng.shutdown().await; break; }
                Some(c) => guarded("a command", || eng.command(c)),
            },
            Some(ev) = tev_rx.recv() => guarded("a SIP message", || eng.transport_event(ev)),
            Some(ev) = stream_rx.recv() => match ev {
                StreamEvent::Dtmf(id, d) => { let _ = eng.events.send(Event::Dtmf(id, d)); }
            },
            Some(i) = internal_rx.recv() => guarded("a network event", || eng.internal(i)),
            _ = tick.tick() => guarded("timers", || eng.tick()),
        }
    }
}

impl Engine {
    pub(crate) fn emit(&self, e: Event) {
        let _ = self.events.send(e);
    }

    fn command(&mut self, c: Command) {
        match c {
            Command::Configure(s) => self.configure(*s),
            Command::Dial { account, number } => self.dial(&account, &number),
            Command::Answer(id) => self.answer(id),
            Command::Reject(id) => self.reject(id, 603, "Decline"),
            Command::Hangup(id) => self.hangup(id),
            Command::HangupAll => {
                let ids: Vec<_> = self.calls.keys().copied().collect();
                for id in ids {
                    self.hangup(id);
                }
            }
            Command::Hold(id, on) => self.set_hold(id, on),
            Command::Dtmf(id, d) => self.send_dtmf(id, d),
            Command::Transfer(id, to) => self.blind_transfer(id, &to),
            Command::AttendedTransfer(a, b) => self.attended_transfer(a, b),
            Command::Conference(on) => self.set_conference(on),
            Command::Mute(on) => {
                self.muted = on;
                self.hub.set_mute(on);
            }
            Command::Shutdown => {}
        }
    }

    fn configure(&mut self, s: Settings) {
        let old = std::mem::replace(&mut self.settings, s);
        self.hub.configure(AudioConfig {
            speaker: self.settings.speaker_device.clone(),
            microphone: self.settings.microphone_device.clone(),
            ring_device: self.settings.ring_device.clone(),
            ringtone: self.settings.ringtone.clone(),
            speaker_volume: self.settings.speaker_volume,
            mic_volume: self.settings.mic_volume,
            ring_volume: self.settings.ring_volume,
            echo_cancellation: self.settings.echo_cancellation,
        });
        self.user_agent = if self.settings.user_agent.trim().is_empty() {
            format!("MegaSIP/{}", env!("CARGO_PKG_VERSION"))
        } else {
            self.settings.user_agent.trim().to_string()
        };
        if old.stun != self.settings.stun || self.stun_server.is_none() {
            self.public_ip = None;
            self.stun_server = None;
            let stun = self.settings.stun.trim().to_string();
            if !stun.is_empty() {
                let tx = self.internal_tx.clone();
                tokio::spawn(async move {
                    let (h, p) = transport::split_host_port(&stun);
                    let r = tokio::net::lookup_host((h, p.unwrap_or(3478)))
                        .await
                        .ok()
                        .and_then(|mut it| it.find(|a| a.is_ipv4()));
                    let _ = tx.send(Internal::SipStun(r));
                });
            }
        }
        // Accounts: unregister removed/changed ones, (re)register the rest.
        let mut wanted: BTreeMap<String, Account> = BTreeMap::new();
        for a in &self.settings.accounts {
            if a.enabled && !a.server.trim().is_empty() {
                wanted.insert(a.id.clone(), a.clone());
            }
        }
        let existing: Vec<String> = self.accs.keys().cloned().collect();
        for id in existing {
            if id == "local" {
                continue;
            }
            let changed = wanted.get(&id).is_none_or(|a| *a != self.accs[&id].cfg);
            if changed {
                self.unregister(&id);
                self.accs.remove(&id);
            }
        }
        for (id, a) in wanted {
            if !self.accs.contains_key(&id) {
                self.add_account(a);
            }
        }
        if !self.accs.contains_key("local") {
            let local = Account {
                id: "local".into(),
                label: "Local".into(),
                ..Default::default()
            };
            let mut acc = self.new_acc(local);
            acc.local_only = true;
            acc.proto = Proto::Udp;
            acc.state = RegState::Off;
            self.accs.insert("local".into(), acc);
        }
        self.publish_accounts();
    }

    fn new_acc(&self, cfg: Account) -> Acc {
        let proto = match cfg.transport.as_str() {
            "tcp" => Proto::Tcp,
            "tls" => Proto::Tls,
            _ => Proto::Udp,
        };
        Acc {
            proto,
            dest: None,
            flow: Flow::Udp,
            contact_addr: None,
            call_id: format!("{}@megasip", random_token(20)),
            cseq: 1,
            from_tag: random_token(12),
            state: RegState::Resolving,
            refresh_at: None,
            retry_at: None,
            register_txn: None,
            auth_tries: 0,
            challenge: None,
            nc: 0,
            keepalive_at: Instant::now() + Duration::from_secs(cfg.keep_alive.max(5) as u64),
            generation: rand::random(),
            local_only: false,
            unregistering: false,
            cfg,
        }
    }

    /// An account by id, falling back to the built-in local account (an account can be
    /// removed in Settings while one of its calls is still up).
    pub(crate) fn acc(&self, id: &str) -> &Acc {
        self.accs
            .get(id)
            .or_else(|| self.accs.get("local"))
            .expect("local account always exists")
    }

    /// A copy of an account's settings-relevant fields (for borrow-free use).
    pub(crate) fn new_acc_snapshot(&self, id: &str) -> Acc {
        let a = self.acc(id);
        let mut c = self.new_acc(a.cfg.clone());
        c.proto = a.proto;
        c
    }

    fn add_account(&mut self, a: Account) {
        let acc = self.new_acc(a);
        let id = acc.cfg.id.clone();
        let generation = acc.generation;
        self.accs.insert(id.clone(), acc);
        self.start_resolve(&id, generation);
    }

    fn start_resolve(&mut self, id: &str, generation: u64) {
        let Some(acc) = self.accs.get_mut(id) else {
            return;
        };
        acc.state = RegState::Resolving;
        let target = if acc.cfg.proxy.trim().is_empty() {
            acc.cfg.server.clone()
        } else {
            acc.cfg.proxy.clone()
        };
        let target = target
            .trim()
            .trim_start_matches("sip:")
            .trim_start_matches("sips:")
            .to_string();
        let target = target.split(';').next().unwrap_or("").to_string();
        let want = match acc.cfg.transport.as_str() {
            "udp" => Some(Proto::Udp),
            "tcp" => Some(Proto::Tcp),
            "tls" => Some(Proto::Tls),
            _ => None,
        };
        let srv = self.settings.dns_srv;
        let tx = self.internal_tx.clone();
        let acc_id = id.to_string();
        tokio::spawn(async move {
            let result = transport::resolve(&target, want, srv).await;
            let _ = tx.send(Internal::Resolved {
                acc: acc_id,
                generation,
                result,
            });
        });
    }

    fn internal(&mut self, i: Internal) {
        match i {
            Internal::Resolved {
                acc,
                generation,
                result,
            } => {
                let Some(a) = self.accs.get(&acc) else { return };
                if a.generation != generation {
                    return;
                }
                match result {
                    None => {
                        let a = self.accs.get_mut(&acc).unwrap();
                        a.state = RegState::Error("Server not found".into());
                        a.retry_at = Some(Instant::now() + Duration::from_secs(30));
                    }
                    Some((dest, Proto::Udp)) => {
                        let ip =
                            transport::local_ip_for(dest).unwrap_or(IpAddr::from([127, 0, 0, 1]));
                        let local = SocketAddr::new(ip, self.tr.udp_local.port());
                        let public = self.public_addr_for(&acc, local);
                        let a = self.accs.get_mut(&acc).unwrap();
                        a.dest = Some(dest);
                        a.proto = Proto::Udp;
                        a.flow = Flow::Udp;
                        a.contact_addr = Some(public);
                        self.register(&acc);
                    }
                    Some((dest, proto)) => {
                        let host = if a.cfg.proxy.trim().is_empty() {
                            a.cfg.server.trim()
                        } else {
                            a.cfg.proxy.trim()
                        };
                        let name = transport::split_host_port(
                            host.trim_start_matches("sip:")
                                .split(';')
                                .next()
                                .unwrap_or(""),
                        )
                        .0
                        .to_string();
                        let tls = self.tr.tls_config();
                        let tx = self.internal_tx.clone();
                        let a = self.accs.get_mut(&acc).unwrap();
                        a.dest = Some(dest);
                        a.proto = proto;
                        a.state = RegState::Registering;
                        tokio::spawn(async move {
                            let result = transport::dial(proto, dest, name, tls)
                                .await
                                .map_err(|e| e.to_string());
                            let _ = tx.send(Internal::Connected {
                                acc,
                                generation,
                                result,
                                peer: dest,
                                proto,
                            });
                        });
                    }
                }
                self.publish_accounts();
            }
            Internal::Connected {
                acc,
                generation,
                result,
                peer,
                proto,
            } => {
                let ok = self
                    .accs
                    .get(&acc)
                    .is_some_and(|a| a.generation == generation);
                if !ok {
                    return;
                }
                match result {
                    Ok((stream, local)) => {
                        let id = self.tr.attach(stream, peer, local, proto);
                        let public = self.public_addr_for(&acc, local);
                        let a = self.accs.get_mut(&acc).unwrap();
                        a.flow = Flow::Stream(id);
                        a.contact_addr = Some(public);
                        self.register(&acc);
                    }
                    Err(e) => {
                        let a = self.accs.get_mut(&acc).unwrap();
                        a.state = RegState::Error(e);
                        a.retry_at = Some(Instant::now() + Duration::from_secs(30));
                    }
                }
                self.publish_accounts();
            }
            Internal::SipStun(server) => {
                self.stun_server = server;
                self.stun_at = Instant::now();
                self.send_stun();
            }
        }
    }

    fn send_stun(&mut self) {
        if let Some(s) = self.stun_server {
            use rand::RngCore;
            rand::thread_rng().fill_bytes(&mut self.stun_txid);
            self.tr.send(Flow::Udp, s, &stun::request(&self.stun_txid));
            self.stun_at = Instant::now();
        }
    }

    /// Apply a configured public address to a local socket address.
    fn public_addr_for(&self, acc: &str, local: SocketAddr) -> SocketAddr {
        let a = self.acc(acc);
        let pa = a.cfg.public_addr.trim();
        if !pa.is_empty() {
            let (h, p) = transport::split_host_port(pa);
            if let Ok(ip) = h.parse::<IpAddr>() {
                return SocketAddr::new(ip, p.unwrap_or(local.port()));
            }
        }
        local
    }

    /// The IP to put in SDP for an account.
    pub(crate) fn media_ip(&self, acc: &str) -> IpAddr {
        let a = self.acc(acc);
        let pa = a.cfg.public_addr.trim();
        if !pa.is_empty() {
            if let Ok(ip) = transport::split_host_port(pa).0.parse::<IpAddr>() {
                return ip;
            }
        }
        if let Some(ip) = self.public_ip {
            return ip;
        }
        if let Some(c) = a.contact_addr {
            if !c.ip().is_unspecified() {
                return c.ip();
            }
        }
        a.dest
            .and_then(transport::local_ip_for)
            .or_else(|| transport::local_ip_for("8.8.8.8:53".parse().unwrap()))
            .unwrap_or(IpAddr::from([127, 0, 0, 1]))
    }

    pub(crate) fn stun_for_rtp(&self) -> Option<SocketAddr> {
        self.stun_server
    }

    pub(crate) fn publish_accounts(&mut self) {
        let mut list = Vec::new();
        for a in self.settings.accounts.iter() {
            let (state, text) = match self.accs.get(&a.id).map(|x| &x.state) {
                None => (
                    "off".to_string(),
                    if a.enabled {
                        "No server".to_string()
                    } else {
                        "Disabled".to_string()
                    },
                ),
                Some(RegState::Off) => ("off".into(), "Not registered".into()),
                Some(RegState::Resolving) => ("registering".into(), "Looking up server…".into()),
                Some(RegState::Registering) => ("registering".into(), "Registering…".into()),
                Some(RegState::Registered) => ("registered".into(), "Online".into()),
                Some(RegState::Error(e)) => ("error".into(), e.clone()),
            };
            list.push(AccountStatus {
                id: a.id.clone(),
                title: a.title(),
                state,
                text,
            });
        }
        let key: Vec<_> = list
            .iter()
            .map(|s| (s.id.clone(), s.state.clone(), s.text.clone()))
            .collect();
        if key != self.last_acc_status {
            self.last_acc_status = key;
            self.emit(Event::Accounts(list));
        }
    }

    // ---------------------------------------------------------------- registration

    fn register(&mut self, id: &str) {
        self.send_register(id, None);
    }

    fn send_register(&mut self, id: &str, expires_override: Option<u32>) {
        let rport = self.settings.rport;
        let ua = self.user_agent.clone();
        let Some(a) = self.accs.get_mut(id) else {
            return;
        };
        if a.local_only || !a.ready() {
            return;
        }
        let expires = expires_override.unwrap_or(a.cfg.register_refresh.max(30));
        let domain = a.cfg.sip_domain();
        let uri = format!(
            "sip:{}{}",
            domain,
            if a.proto == Proto::Udp {
                ""
            } else {
                a.proto.uri_param()
            }
        );
        let mut req = SipMsg::request("REGISTER", &uri);
        let branch = new_branch();
        req.add(
            "Via",
            format!(
                "SIP/2.0/{} {};branch={}{}",
                a.proto.via_name(),
                a.via_host(),
                branch,
                if rport { ";rport" } else { "" }
            ),
        );
        req.add("Max-Forwards", "70");
        let from = if a.cfg.display_name.trim().is_empty() {
            format!("<{}>;tag={}", a.aor(), a.from_tag)
        } else {
            format!(
                "{} <{}>;tag={}",
                msg::quote_display(a.cfg.display_name.trim()),
                a.aor(),
                a.from_tag
            )
        };
        req.add("From", from);
        req.add("To", format!("<{}>", a.aor()));
        req.add("Call-ID", a.call_id.clone());
        req.add("CSeq", format!("{} REGISTER", a.cseq));
        a.cseq += 1;
        if !a.cfg.proxy.trim().is_empty() {
            req.add("Route", route_for_proxy(&a.cfg.proxy, a.proto));
        }
        req.add("Contact", a.contact());
        req.add("Expires", expires.to_string());
        req.add("Allow", ALLOW);
        req.add("User-Agent", ua);
        if let Some((ch, proxy)) = a.challenge.clone() {
            a.nc += 1;
            if let Some(h) = digest(&ch, &a.auth_user(), &a.cfg.password, "REGISTER", &uri, a.nc) {
                req.add(
                    if proxy {
                        "Proxy-Authorization"
                    } else {
                        "Authorization"
                    },
                    h,
                );
            }
        }
        if expires_override == Some(0) {
            a.unregistering = true;
        } else if a.state != RegState::Registered {
            a.state = RegState::Registering;
        }
        let (flow, dest) = (a.flow, a.dest.unwrap());
        a.register_txn = Some(branch.clone());
        a.retry_at = None;
        a.refresh_at = None;
        self.send_request(req, branch, flow, dest, TxnOwner::Register(id.to_string()));
        self.publish_accounts();
    }

    fn unregister(&mut self, id: &str) {
        let registered = self
            .accs
            .get(id)
            .is_some_and(|a| a.state == RegState::Registered);
        if registered {
            self.send_register(id, Some(0));
        }
        if let Some(a) = self.accs.get(id) {
            if let Flow::Stream(s) = a.flow {
                // Let the un-REGISTER go out before the connection drops.
                let _ = s;
            }
        }
    }

    fn on_register_response(&mut self, id: &str, req: &SipMsg, resp: &SipMsg) {
        let code = resp.code();
        if code < 200 {
            return;
        }
        let Some(a) = self.accs.get_mut(id) else {
            return;
        };
        a.register_txn = None;
        let unreg = req.header("Expires") == Some("0");
        match code {
            200..=299 => {
                a.auth_tries = 0;
                if unreg {
                    a.state = RegState::Off;
                    self.publish_accounts();
                    return;
                }
                // Granted expiry: our Contact's expires param, else Expires header.
                let ours = a.contact();
                let ours_uri = msg::parse_name_addr(&ours).uri;
                let mut granted: Option<u32> = None;
                for c in resp.headers_all("Contact") {
                    let na = msg::parse_name_addr(c);
                    if na.uri.eq_ignore_ascii_case(&ours_uri) {
                        granted = param(c, "expires").and_then(|v| v.parse().ok());
                    }
                }
                let granted = granted
                    .or_else(|| resp.header("Expires").and_then(|v| v.trim().parse().ok()))
                    .unwrap_or(a.cfg.register_refresh.max(30));
                // NAT: the server tells us how it sees us (rport/received).
                if a.cfg.allow_rewrite
                    && a.proto == Proto::Udp
                    && a.cfg.public_addr.trim().is_empty()
                {
                    if let Some(via) = resp.header("Via") {
                        let recv = param(via, "received").and_then(|v| v.parse::<IpAddr>().ok());
                        let rport = param(via, "rport").and_then(|v| v.parse::<u16>().ok());
                        if let (Some(cur), Some(ip)) =
                            (a.contact_addr, recv.or(Some(a.contact_addr.unwrap().ip())))
                        {
                            let port = rport.unwrap_or(cur.port());
                            let seen = SocketAddr::new(ip, port);
                            if seen != cur && recv.is_some() {
                                tracing::info!(
                                    "account {}: NAT detected, contact {} -> {}",
                                    a.cfg.title(),
                                    cur,
                                    seen
                                );
                                // Remove the old binding, then register the public one.
                                a.contact_addr = Some(seen);
                                let id2 = id.to_string();
                                self.send_register(&id2, None);
                                return;
                            }
                        }
                    }
                }
                a.state = RegState::Registered;
                let secs = if granted > 60 {
                    granted as u64 * 85 / 100
                } else {
                    (granted as u64).saturating_sub(5).max(5)
                };
                a.refresh_at = Some(Instant::now() + Duration::from_secs(secs));
            }
            401 | 407 => {
                let hdr = if code == 401 {
                    resp.header("WWW-Authenticate")
                } else {
                    resp.header("Proxy-Authenticate")
                };
                let ch = hdr.and_then(|h| DigestChallenge::parse(h).ok());
                let stale = hdr.is_some_and(|h| h.to_ascii_lowercase().contains("stale=true"));
                if let Some(ch) = ch {
                    if a.auth_tries < 2 || stale {
                        a.auth_tries += 1;
                        a.challenge = Some((ch, code == 407));
                        a.nc = 0;
                        let id2 = id.to_string();
                        let exp = if unreg { Some(0) } else { None };
                        self.send_register(&id2, exp);
                        return;
                    }
                }
                a.state = RegState::Error("Wrong username or password".into());
                a.retry_at = Some(Instant::now() + Duration::from_secs(60));
                a.auth_tries = 0;
            }
            423 => {
                let min = resp
                    .header("Min-Expires")
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(3600);
                a.cfg.register_refresh = min;
                let id2 = id.to_string();
                self.send_register(&id2, None);
                return;
            }
            _ => {
                if unreg {
                    a.state = RegState::Off;
                } else {
                    a.state = RegState::Error(format!("{} {}", code, resp.reason()));
                    a.retry_at = Some(
                        Instant::now() + Duration::from_secs(if code == 408 { 20 } else { 60 }),
                    );
                }
            }
        }
        self.publish_accounts();
    }

    // ---------------------------------------------------------------- transactions

    /// Send a request in a new client transaction.
    pub(crate) fn send_request(
        &mut self,
        req: SipMsg,
        branch: String,
        flow: Flow,
        dest: SocketAddr,
        owner: TxnOwner,
    ) {
        let bytes = req.to_bytes();
        self.tr.send(flow, dest, &bytes);
        if req.method() == "ACK" {
            return;
        }
        let now = Instant::now();
        self.ctx.insert(
            branch,
            ClientTxn {
                req,
                bytes,
                flow,
                dest,
                reliable: flow != Flow::Udp,
                started: now,
                next_retx: now + T1,
                interval: T1,
                provisional: false,
                owner,
                auth_tries: 0,
            },
        );
    }

    /// Send a response and remember it for retransmitted requests.
    pub(crate) fn respond(&mut self, req: &SipMsg, resp: SipMsg, flow: Flow, from: SocketAddr) {
        let bytes = resp.to_bytes();
        let dest = response_dest(req, flow, from);
        self.tr.send(flow, dest, &bytes);
        let key = (req.branch().unwrap_or_default(), req.method().to_string());
        let is_invite_final = req.method() == "INVITE" && resp.code() >= 200;
        let reliable = flow != Flow::Udp;
        let e = self.stx.entry(key).or_insert(ServerTxn {
            last: None,
            flow,
            dest,
            expires: Instant::now() + TXN_TIMEOUT,
            retx: None,
            call_id: req.call_id().to_string(),
            cseq: req.cseq().0,
        });
        e.last = Some(bytes);
        e.expires = Instant::now() + TXN_TIMEOUT;
        if is_invite_final && !reliable {
            e.retx = Some((Instant::now() + T1, T1));
        }
    }

    fn transport_event(&mut self, ev: TransportEvent) {
        match ev {
            TransportEvent::Closed(id) => {
                self.tr.closed(id);
                let ids: Vec<String> = self
                    .accs
                    .iter()
                    .filter(|(_, a)| a.flow == Flow::Stream(id))
                    .map(|(k, _)| k.clone())
                    .collect();
                for k in ids {
                    let a = self.accs.get_mut(&k).unwrap();
                    a.state = RegState::Error("Connection lost".into());
                    a.contact_addr = None;
                    a.dest = None;
                    a.generation = rand::random();
                    a.retry_at = Some(Instant::now() + Duration::from_secs(5));
                }
                self.publish_accounts();
            }
            TransportEvent::Message { data, from, flow } => {
                if stun::is_stun(&data) {
                    if let Some((txid, mapped)) = stun::parse_response(&data) {
                        if txid == self.stun_txid {
                            if self.public_ip != Some(mapped.ip()) {
                                tracing::info!("STUN: public address {mapped}");
                            }
                            self.public_ip = Some(mapped.ip());
                        }
                    }
                    return;
                }
                crate::debug::sip("in", self.tr.flow_name(flow), from, &data);
                let Some(m) = SipMsg::parse(&data) else {
                    tracing::debug!("unparseable datagram from {from}");
                    return;
                };
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(
                        "RECV {:?} <- {from}\n{}",
                        flow,
                        String::from_utf8_lossy(&data)
                    );
                }
                if m.is_request() {
                    self.on_request(m, flow, from);
                } else {
                    self.on_response(m);
                }
            }
        }
    }

    fn on_response(&mut self, resp: SipMsg) {
        let Some(branch) = resp.branch() else { return };
        let (_, method) = resp.cseq();
        let code = resp.code();
        let Some(txn) = self.ctx.get_mut(&branch) else {
            // Retransmitted 2xx to INVITE after the transaction ended: re-ACK.
            if method == "INVITE" && (200..300).contains(&code) {
                self.on_stray_invite_2xx(&resp);
            }
            return;
        };
        if txn.req.method() != method && !(method == "INVITE" && txn.req.method() == "INVITE") {
            return;
        }
        if code < 200 {
            txn.provisional = true;
            if txn.req.method() == "INVITE" {
                // Stop retransmitting INVITE; keep the timeout long (ringing can take a while).
                txn.next_retx = Instant::now() + Duration::from_secs(3600);
                txn.started = Instant::now() + Duration::from_secs(150);
            } else {
                txn.interval = T2;
            }
            let owner = txn.owner.clone();
            let req = txn.req.clone();
            if let TxnOwner::Call(id) = owner {
                self.call_response(id, &req, &resp);
            }
            return;
        }
        let txn = self.ctx.remove(&branch).unwrap();
        // Digest challenge for anything but REGISTER (handled with account state).
        if (code == 401 || code == 407)
            && !matches!(txn.owner, TxnOwner::Register(_))
            && txn.auth_tries < 2
            && self.retry_with_auth(&txn, &resp)
        {
            return;
        }
        match txn.owner.clone() {
            TxnOwner::Register(id) => self.on_register_response(&id, &txn.req, &resp),
            TxnOwner::Call(id) => self.call_response(id, &txn.req, &resp),
        }
    }

    /// Re-send a request with credentials after 401/407. Returns false if we cannot.
    fn retry_with_auth(&mut self, txn: &ClientTxn, resp: &SipMsg) -> bool {
        let acc_id = match &txn.owner {
            TxnOwner::Call(id) => match self.calls.get(id) {
                Some(c) => c.acc.clone(),
                None => return false,
            },
            _ => return false,
        };
        let Some(acc) = self.accs.get_mut(&acc_id) else {
            return false;
        };
        let proxy = resp.code() == 407;
        let hdr = if proxy {
            resp.header("Proxy-Authenticate")
        } else {
            resp.header("WWW-Authenticate")
        };
        let Some(ch) = hdr.and_then(|h| DigestChallenge::parse(h).ok()) else {
            return false;
        };
        let user = acc.auth_user();
        let pass = acc.cfg.password.clone();
        acc.challenge = Some((ch.clone(), proxy));
        acc.nc = 1;
        let mut req = txn.req.clone();
        let method = req.method().to_string();
        let uri = req.uri().to_string();
        let Some(h) = digest(&ch, &user, &pass, &method, &uri, 1) else {
            return false;
        };
        // ACK the challenge first for INVITE.
        if method == "INVITE" {
            let ack = call::ack_for_failure(&txn.req, resp);
            self.tr.send(txn.flow, txn.dest, &ack.to_bytes());
        }
        req.remove("Authorization");
        req.remove("Proxy-Authorization");
        req.add(
            if proxy {
                "Proxy-Authorization"
            } else {
                "Authorization"
            },
            h,
        );
        let branch = new_branch();
        // New Via branch and CSeq.
        let via = req.header("Via").unwrap_or("").to_string();
        let new_via = replace_branch(&via, &branch);
        let mut hs = Vec::new();
        let mut first_via = true;
        for (n, v) in req.headers.drain(..) {
            if n.eq_ignore_ascii_case("Via") && first_via {
                first_via = false;
                hs.push((n, new_via.clone()));
            } else {
                hs.push((n, v));
            }
        }
        req.headers = hs;
        let next_cseq = if let TxnOwner::Call(id) = &txn.owner {
            let c = self.calls.get_mut(id).unwrap();
            c.local_cseq += 1;
            if method == "INVITE" {
                c.invite_branch = branch.clone();
                c.invite_cseq = c.local_cseq;
            }
            c.local_cseq
        } else {
            req.cseq().0 + 1
        };
        req.set("CSeq", format!("{next_cseq} {method}"));
        let mut t = ClientTxn {
            bytes: Vec::new(),
            req: req.clone(),
            flow: txn.flow,
            dest: txn.dest,
            reliable: txn.reliable,
            started: Instant::now(),
            next_retx: Instant::now() + T1,
            interval: T1,
            provisional: false,
            owner: txn.owner.clone(),
            auth_tries: txn.auth_tries + 1,
        };
        t.bytes = req.to_bytes();
        if let TxnOwner::Call(id) = &txn.owner {
            if method == "INVITE" {
                if let Some(c) = self.calls.get_mut(id) {
                    c.invite = Some(req.clone());
                }
            }
        }
        self.tr.send(t.flow, t.dest, &t.bytes);
        self.ctx.insert(branch, t);
        true
    }

    fn on_request(&mut self, req: SipMsg, flow: Flow, from: SocketAddr) {
        let method = req.method().to_string();
        // Retransmission of a request we already answered.
        if method != "ACK" {
            let key = (req.branch().unwrap_or_default(), method.clone());
            if let Some(t) = self.stx.get(&key) {
                if let Some(last) = &t.last {
                    self.tr.send(t.flow, t.dest, last);
                }
                return;
            }
        }
        match method.as_str() {
            "ACK" => {
                let cid = req.call_id().to_string();
                let cseq = req.cseq().0;
                for t in self.stx.values_mut() {
                    if t.call_id == cid && t.cseq == cseq {
                        t.retx = None;
                    }
                }
                self.call_ack(&req);
            }
            "INVITE" => self.on_invite(req, flow, from),
            "CANCEL" => self.on_cancel(req, flow, from),
            "BYE" | "INFO" | "NOTIFY" | "REFER" | "UPDATE" | "PRACK" => {
                self.in_dialog_request(req, flow, from)
            }
            "OPTIONS" => {
                if req.to_tag().is_some() {
                    self.in_dialog_request(req, flow, from);
                } else {
                    let mut r = req.make_response(200, "OK", Some(&random_token(8)));
                    r.add("Allow", ALLOW);
                    r.add("Accept", "application/sdp");
                    r.add("Supported", "replaces");
                    r.add("User-Agent", self.user_agent.clone());
                    self.respond(&req, r, flow, from);
                }
            }
            "MESSAGE" | "SUBSCRIBE" | "PUBLISH" => {
                let mut r = req.make_response(405, "Method Not Allowed", Some(&random_token(8)));
                r.add("Allow", ALLOW);
                self.respond(&req, r, flow, from);
            }
            _ => {
                let mut r = req.make_response(501, "Not Implemented", Some(&random_token(8)));
                r.add("Allow", ALLOW);
                self.respond(&req, r, flow, from);
            }
        }
    }

    fn tick(&mut self) {
        let now = Instant::now();
        // Client transactions: retransmit / time out.
        let mut timed_out = Vec::new();
        for (b, t) in self.ctx.iter_mut() {
            if now.saturating_duration_since(t.started) > TXN_TIMEOUT {
                timed_out.push(b.clone());
                continue;
            }
            if !t.reliable && now >= t.next_retx {
                self.tr.send(t.flow, t.dest, &t.bytes);
                let cap = if t.req.method() == "INVITE" {
                    Duration::from_secs(3600)
                } else {
                    T2
                };
                t.interval = (t.interval * 2).min(cap);
                t.next_retx = now + t.interval;
            }
        }
        for b in timed_out {
            let t = self.ctx.remove(&b).unwrap();
            let fake = t.req.make_response(408, "Request Timeout", None);
            match t.owner.clone() {
                TxnOwner::Register(id) => self.on_register_response(&id, &t.req, &fake),
                TxnOwner::Call(id) => self.call_response(id, &t.req, &fake),
            }
        }
        // Server transactions: retransmit INVITE finals until ACK; expire.
        self.stx.retain(|_, t| t.expires > now);
        for t in self.stx.values_mut() {
            if let Some((at, iv)) = t.retx {
                if now >= at {
                    if let Some(b) = &t.last {
                        self.tr.send(t.flow, t.dest, b);
                    }
                    let iv2 = (iv * 2).min(T2);
                    t.retx = Some((now + iv2, iv2));
                }
            }
        }
        // Registration refresh / retries / keep-alive.
        let ids: Vec<String> = self.accs.keys().cloned().collect();
        for id in ids {
            let a = &self.accs[&id];
            if a.local_only {
                continue;
            }
            if a.refresh_at.is_some_and(|t| now >= t) && a.register_txn.is_none() {
                self.register(&id);
                continue;
            }
            if a.retry_at.is_some_and(|t| now >= t) {
                let a = self.accs.get_mut(&id).unwrap();
                a.retry_at = None;
                a.challenge = None;
                if a.ready() && a.flow == Flow::Udp
                    || matches!(a.flow, Flow::Stream(s) if self.tr.is_open(s))
                {
                    self.register(&id);
                } else {
                    let g: u64 = rand::random();
                    a.generation = g;
                    self.start_resolve(&id, g);
                }
                continue;
            }
            let a = self.accs.get_mut(&id).unwrap();
            if a.cfg.keep_alive > 0 && now >= a.keepalive_at && a.state == RegState::Registered {
                a.keepalive_at = now + Duration::from_secs(a.cfg.keep_alive as u64);
                if let Some(d) = a.dest {
                    let f = a.flow;
                    self.tr.send(f, d, b"\r\n\r\n");
                }
            }
        }
        if self.stun_server.is_some()
            && now - self.stun_at
                > Duration::from_secs(if self.public_ip.is_some() { 300 } else { 10 })
        {
            self.send_stun();
        }
        self.call_tick(now);
        self.publish_accounts();
    }

    async fn shutdown(&mut self) {
        let ids: Vec<_> = self.calls.keys().copied().collect();
        for id in ids {
            self.hangup(id);
        }
        let accs: Vec<String> = self.accs.keys().cloned().collect();
        for id in accs {
            self.unregister(&id);
        }
        // Give BYE/un-REGISTER a moment to leave (and be retransmitted once).
        for _ in 0..10 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if self.ctx.is_empty() {
                break;
            }
            self.tick();
        }
    }
}

pub(crate) fn digest(
    ch: &DigestChallenge,
    user: &str,
    pass: &str,
    method: &str,
    uri: &str,
    nc: u32,
) -> Option<String> {
    let creds = DigestCredentials::new(user, pass);
    DigestResponse::from_challenge(ch, &creds, method, uri, None, Some(nc.max(1)))
        .ok()
        .map(|r| r.to_header_value())
}

pub(crate) fn route_for_proxy(proxy: &str, proto: Proto) -> String {
    let p = proxy.trim();
    let p = p.strip_prefix("sip:").unwrap_or(p);
    let host = p.split(';').next().unwrap_or(p);
    format!("<sip:{}{};lr>", host, proto.uri_param())
}

pub(crate) fn replace_branch(via: &str, branch: &str) -> String {
    let mut parts: Vec<String> = via.split(';').map(|s| s.to_string()).collect();
    for p in parts.iter_mut().skip(1) {
        if p.trim().to_ascii_lowercase().starts_with("branch=") {
            *p = format!("branch={branch}");
        }
    }
    parts.join(";")
}

/// Where to send a response (RFC 3261 18.2.2 + RFC 3581 rport).
fn response_dest(req: &SipMsg, flow: Flow, from: SocketAddr) -> SocketAddr {
    if flow != Flow::Udp {
        return from;
    }
    let Some(via) = req.header("Via") else {
        return from;
    };
    if param(via, "rport").is_some() || via.contains(";rport") {
        return from;
    }
    // Otherwise sent-by host:port, falling back to the source.
    let sent_by = via
        .split_whitespace()
        .nth(1)
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("");
    let (h, p) = transport::split_host_port(sent_by);
    match h.parse::<IpAddr>() {
        Ok(ip) => SocketAddr::new(
            param(via, "received")
                .and_then(|r| r.parse().ok())
                .unwrap_or(ip),
            p.unwrap_or(5060),
        ),
        Err(_) => from,
    }
}
