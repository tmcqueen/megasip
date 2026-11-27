//! `megasip --mcp -a <account>`: the phone as an MCP server on stdin/stdout, so an AI agent
//! can run VoIP tests: place and answer calls, send DTMF, wait for call states, pull call
//! quality reports (live or after the call), read the SIP trace, and read or change the
//! settings (account passwords excepted: they come back redacted and can't be set).
//!
//! It drives the same [`Controller`] a window does, on a thread of its own; the MCP tools
//! hand it closures over a channel. Stdout is the MCP channel, so nothing else may print
//! there (logs go to stderr and the usual files).

use crate::config::{Account, Settings};
use crate::controller::{Controller, Out, Timer};
use crate::engine::{CallInfo, Event};
use crate::media::source::Source;
use crate::media::StreamStats;
use rmcp::handler::server::{tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::{schemars, tool, tool_handler, tool_router, ServerHandler, ServiceExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// What a set password reads as.
pub const REDACTED: &str = "<redacted>";
const MAX_EVENTS: usize = 2000;
/// Ended calls kept for reports.
const MAX_ENDED: usize = 50;
/// One quality sample a second; this many per call (two hours).
const MAX_SAMPLES: usize = 7200;
/// Seconds per interval in a report's timeline.
const INTERVAL_S: f64 = 5.0;

const INSTRUCTIONS: &str = "MegaSIP is a SIP softphone for VoIP testing; this server is one registered account. \
Typical test: `status` (check the account is registered; `wait_for` until=registered if not), \
`dial` (returns a call id), `wait_for` until=connected, `send_dtmf`, `wait_for` until=dtmf or ended, \
`call_quality_report` (works during the call for a spot check and after it ends for the final report), \
`hangup`, and `sip_trace` to see the signalling. `set_source` or dial's `source` sends a test signal \
(milli = 1004 Hz, pink, white, tone:<hz>, or a .wav/.ogg/.mp3) instead of the microphone. \
`events` lists everything that happened since a sequence number. Settings can be read and changed \
with `get_settings` / `update_settings`; account passwords are never shown and can't be set here. \
Emergency numbers can't be dialled.";

// ------------------------------------------------------------------ dial policy

/// Which numbers the agent may call or transfer to.
#[derive(Debug, Clone, Default)]
pub struct DialPolicy {
    /// `--mcp-allow <prefix>`: if any, the number (or URI) must start with one of these.
    pub allow: Vec<String>,
}

/// Emergency numbers, also behind a one-digit outside-line prefix (9911). 933 (the US
/// E911 address readback line) stays allowed: calling it is a normal VoIP test.
const EMERGENCY: &[&str] = &["911", "112", "999", "000", "110", "119"];

/// The user part of a number or URI, digits and `+` only.
fn dialled_digits(target: &str) -> String {
    let t = target.trim();
    let t = t
        .strip_prefix("sips:")
        .or_else(|| t.strip_prefix("sip:"))
        .or_else(|| t.strip_prefix("tel:"))
        .or_else(|| t.strip_prefix("callto:"))
        .unwrap_or(t);
    let user = t.split(['@', ';']).next().unwrap_or("");
    user.chars().filter(|c| c.is_ascii_digit()).collect()
}

pub fn is_emergency(target: &str) -> bool {
    let d = dialled_digits(target);
    EMERGENCY
        .iter()
        .any(|e| d == *e || (*e != "000" && d.len() == e.len() + 1 && d.ends_with(e)))
}

impl DialPolicy {
    pub fn check(&self, target: &str) -> Result<(), String> {
        if target.trim().is_empty() {
            return Err("No number given".into());
        }
        if is_emergency(target) {
            return Err(format!(
                "{target} looks like an emergency number; MegaSIP won't call it over MCP"
            ));
        }
        if !self.allow.is_empty() {
            let t = crate::controller::dial_target(target);
            let digits = dialled_digits(target);
            if !self.allow.iter().any(|p| {
                t.starts_with(p.as_str()) || (!digits.is_empty() && digits.starts_with(p.as_str()))
            }) {
                return Err(format!(
                    "{target} isn't allowed: this server only calls numbers starting with {}",
                    self.allow.join(", ")
                ));
            }
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ settings

/// The settings as JSON with every set password replaced by [`REDACTED`].
pub fn redacted_settings(s: &Settings) -> Value {
    let mut v = serde_json::to_value(s).unwrap_or(Value::Null);
    if let Some(list) = v.get_mut("accounts").and_then(Value::as_array_mut) {
        for a in list {
            if let Some(p) = a.get_mut("password") {
                let set = p.as_str().is_some_and(|x| !x.is_empty());
                *p = Value::String(if set { REDACTED.into() } else { String::new() });
            }
        }
    }
    v
}

const PASSWORD_REFUSED: &str =
    "Account passwords can't be set or changed over MCP; set them in MegaSIP's Preferences";

/// RFC 7396-style merge of `patch` into `target`, refusing keys `target` doesn't have.
fn merge_known(target: &mut Value, patch: &Value, path: &str) -> Result<(), String> {
    match (target, patch) {
        (Value::Object(t), Value::Object(p)) => {
            for (k, pv) in p {
                let here = format!("{path}{k}");
                let Some(tv) = t.get_mut(k) else {
                    return Err(format!("Unknown setting \"{here}\""));
                };
                if pv.is_null() {
                    return Err(format!("\"{here}\" can't be null"));
                }
                merge_known(tv, pv, &format!("{here}."))?;
            }
            Ok(())
        }
        (t, p) => {
            *t = p.clone();
            Ok(())
        }
    }
}

/// Apply `patch` to `cur`: top-level settings merge by key; `accounts` is a list of changes,
/// each `{"id": ..., <fields>}` to edit an account, `{"id": ..., "delete": true}` to remove
/// one, or `{<fields>}` without an id to add one. Passwords can't be in it (except the
/// unchanged [`REDACTED`] placeholder, so a read-modify-write round trip works), and
/// `protected` (the account this server runs as) can't be deleted. Returns the new
/// settings and what changed.
pub fn apply_patch(
    cur: &Settings,
    patch: &Value,
    protected: Option<&str>,
) -> Result<(Settings, Vec<String>), String> {
    let Value::Object(p) = patch else {
        return Err("The patch must be a JSON object of settings to change".into());
    };
    let mut v = serde_json::to_value(cur).map_err(|e| e.to_string())?;
    let mut changed = Vec::new();
    for (k, pv) in p {
        if k == "accounts" {
            let Value::Array(items) = pv else {
                return Err("\"accounts\" must be a list of account changes".into());
            };
            for item in items {
                let what = patch_account(&mut v, item, protected)?;
                changed.push(what);
            }
            continue;
        }
        let Some(tv) = v.get_mut(k) else {
            return Err(format!("Unknown setting \"{k}\""));
        };
        if pv.is_null() {
            return Err(format!("\"{k}\" can't be null"));
        }
        merge_known(tv, pv, &format!("{k}."))?;
        changed.push(k.clone());
    }
    let s: Settings = serde_json::from_value(v).map_err(|e| format!("Invalid settings: {e}"))?;
    // Belt and braces: whatever happened above, every password is what it was.
    for a in &s.accounts {
        let before = cur.accounts.iter().find(|b| b.id == a.id);
        if a.password != before.map(|b| b.password.as_str()).unwrap_or("") {
            return Err(PASSWORD_REFUSED.into());
        }
    }
    Ok((s, changed))
}

fn patch_account(v: &mut Value, item: &Value, protected: Option<&str>) -> Result<String, String> {
    let Value::Object(fields) = item else {
        return Err("Each account change must be an object".into());
    };
    let mut fields = fields.clone();
    if let Some(pw) = fields.remove("password") {
        if pw.as_str() != Some(REDACTED) {
            return Err(PASSWORD_REFUSED.into());
        }
    }
    let list = v
        .get_mut("accounts")
        .and_then(Value::as_array_mut)
        .ok_or("settings have no account list")?;
    let id = fields
        .remove("id")
        .and_then(|x| x.as_str().map(str::to_string));
    let delete = fields
        .remove("delete")
        .is_some_and(|d| d == Value::Bool(true));
    match id {
        Some(id) => {
            let Some(pos) = list
                .iter()
                .position(|a| a["id"] == Value::String(id.clone()))
            else {
                return Err(format!(
                    "No account with id \"{id}\" (leave out \"id\" to add an account)"
                ));
            };
            if delete {
                if protected == Some(id.as_str()) {
                    return Err("Can't delete the account this MCP server is running as".into());
                }
                list.remove(pos);
                return Ok(format!("accounts[{id}] deleted"));
            }
            merge_known(&mut list[pos], &Value::Object(fields), "accounts[].")?;
            Ok(format!("accounts[{id}]"))
        }
        None => {
            if delete {
                return Err("Say which account to delete with \"id\"".into());
            }
            let a = Account {
                id: crate::engine::msg::random_token(10),
                ..Default::default()
            };
            let mut av = serde_json::to_value(&a).map_err(|e| e.to_string())?;
            merge_known(&mut av, &Value::Object(fields), "accounts[].")?;
            list.push(av);
            Ok(format!(
                "accounts[{}] added (no password: set it in Preferences)",
                a.id
            ))
        }
    }
}

// ------------------------------------------------------------------ quality

#[derive(Clone, Debug)]
struct Sample {
    /// Seconds since the call's record was made.
    t: f64,
    expected: u64,
    lost: u64,
    jitter_ms: f64,
    mos: Option<f64>,
    last_rx_ms: Option<u64>,
}

struct CallRecord {
    info: CallInfo,
    live: bool,
    missed: bool,
    created: Instant,
    ended_at: Option<i64>,
    states: BTreeSet<String>,
    dtmf_rx: String,
    dtmf_tx: String,
    samples: VecDeque<Sample>,
    last_stats: Option<StreamStats>,
    final_stats: Option<StreamStats>,
}

impl CallRecord {
    fn new(info: CallInfo) -> Self {
        let mut states = BTreeSet::new();
        states.insert(info.state.clone());
        CallRecord {
            info,
            live: true,
            missed: false,
            created: Instant::now(),
            ended_at: None,
            states,
            dtmf_rx: String::new(),
            dtmf_tx: String::new(),
            samples: VecDeque::new(),
            last_stats: None,
            final_stats: None,
        }
    }

    fn update(&mut self, info: CallInfo) {
        self.states.insert(info.state.clone());
        if info.connected_at.is_some() {
            self.states.insert("connected".into());
        }
        self.info = info;
    }

    fn sample(&mut self, s: &StreamStats) {
        if self.samples.len() >= MAX_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back(Sample {
            t: self.created.elapsed().as_secs_f64(),
            expected: s.expected,
            lost: s.lost,
            jitter_ms: s.jitter_ms,
            mos: s.mos,
            last_rx_ms: s.last_rx_ms,
        });
        self.last_stats = Some(s.clone());
    }
}

/// MOS to words, per the usual E-model bands.
pub fn rating(mos: f64) -> &'static str {
    if mos >= 4.3 {
        "excellent"
    } else if mos >= 4.0 {
        "good"
    } else if mos >= 3.6 {
        "fair"
    } else if mos >= 3.1 {
        "poor"
    } else {
        "bad"
    }
}

/// The samples in `INTERVAL_S` windows: loss in each window (not since the start), the
/// window's worst jitter and its last MOS. Counters going backwards (the stream was
/// replaced by a re-INVITE) start the window's count over.
fn intervals(samples: &VecDeque<Sample>) -> Vec<Value> {
    let mut out = Vec::new();
    let Some(first) = samples.front() else {
        return out;
    };
    let mut start_t = first.t;
    let (mut base_exp, mut base_lost) = (first.expected, first.lost);
    let mut window: Vec<&Sample> = Vec::new();
    if samples.len() == 1 {
        window.push(first);
        base_exp = 0;
        base_lost = 0;
    }
    let flush =
        |w: &[&Sample], start_t: f64, base_exp: u64, base_lost: u64, out: &mut Vec<Value>| {
            let Some(last) = w.last() else { return };
            let (de, dl) = if last.expected >= base_exp && last.lost >= base_lost {
                (last.expected - base_exp, last.lost - base_lost)
            } else {
                (last.expected, last.lost)
            };
            let loss = if de > 0 {
                dl as f64 * 100.0 / de as f64
            } else {
                0.0
            };
            let jitter = w.iter().map(|s| s.jitter_ms).fold(0.0, f64::max);
            let silent = w.iter().filter_map(|s| s.last_rx_ms).max().unwrap_or(0);
            out.push(json!({
                "fromS": round1(start_t),
                "toS": round1(last.t),
                "packetsExpected": de,
                "lossPct": round2(loss),
                "maxJitterMs": round1(jitter),
                "mos": last.mos.map(round2),
                "longestSilenceMs": silent,
            }));
        };
    for s in samples.iter().skip(1) {
        if s.expected < base_exp {
            base_exp = 0;
            base_lost = 0;
        }
        window.push(s);
        if s.t - start_t >= INTERVAL_S {
            flush(&window, start_t, base_exp, base_lost, &mut out);
            base_exp = s.expected;
            base_lost = s.lost;
            start_t = s.t;
            window.clear();
        }
    }
    flush(&window, start_t, base_exp, base_lost, &mut out);
    out
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// What stands out in a call's media, in words.
fn findings(rec: &CallRecord, st: Option<&StreamStats>, timeline: &[Value]) -> Vec<String> {
    let mut f = Vec::new();
    let connected = rec.info.connected_at.is_some();
    let Some(s) = st else {
        if connected {
            f.push("No media stream was set up for this call".into());
        } else {
            f.push("The call never connected, so there is no media to report on".into());
        }
        return f;
    };
    if connected && s.rx_packets == 0 {
        f.push(
            "No RTP was received: one-way audio (check NAT, firewall or the far end's SDP address)"
                .into(),
        );
    }
    if connected && s.tx_packets == 0 && !rec.info.held {
        f.push("No RTP was sent".into());
    }
    if rec.live {
        if let Some(ms) = s.last_rx_ms.filter(|ms| *ms > 2000) {
            if !rec.info.remote_held && !rec.info.held {
                f.push(format!(
                    "No RTP received for the last {:.1} s",
                    ms as f64 / 1000.0
                ));
            }
        }
    }
    if s.srtp_failures > 0 {
        f.push(format!(
            "{} SRTP authentication failures (key mismatch or corrupted packets)",
            s.srtp_failures
        ));
    }
    if s.loss_pct >= 5.0 {
        f.push(format!("Heavy packet loss: {:.1}%", s.loss_pct));
    } else if s.loss_pct >= 1.0 {
        f.push(format!("Noticeable packet loss: {:.1}%", s.loss_pct));
    }
    let worst = timeline
        .iter()
        .filter_map(|i| i["lossPct"].as_f64())
        .fold(0.0, f64::max);
    if worst >= 5.0 && s.loss_pct < 5.0 {
        f.push(format!(
            "Loss came in bursts: up to {worst:.1}% in a {INTERVAL_S:.0} s window"
        ));
    }
    if s.jitter_ms >= 30.0 {
        f.push(format!("High jitter: {:.1} ms", s.jitter_ms));
    }
    if s.late > 0 {
        f.push(format!("{} packets arrived too late to play", s.late));
    }
    if s.underruns > 10 {
        f.push(format!(
            "{} playout buffer underruns (gaps heard as clicks or dropouts)",
            s.underruns
        ));
    }
    if s.duplicates > 0 {
        f.push(format!("{} duplicate packets", s.duplicates));
    }
    if s.mos.is_none() && connected {
        f.push("Too few packets for a MOS estimate".into());
    }
    if f.is_empty() {
        f.push("Nothing stands out".into());
    }
    f
}

fn quality_report(rec: &CallRecord, hub: Option<&crate::media::hub::AudioHub>) -> Value {
    let st = if rec.live {
        rec.last_stats.as_ref()
    } else {
        rec.final_stats.as_ref().or(rec.last_stats.as_ref())
    };
    let timeline = intervals(&rec.samples);
    let moses: Vec<f64> = rec.samples.iter().filter_map(|s| s.mos).collect();
    let connected_s = rec.info.connected_at.map(|c| {
        let end = rec
            .ended_at
            .unwrap_or_else(|| chrono::Utc::now().timestamp());
        (end - c).max(0)
    });
    let mut v = json!({
        "kind": if rec.live { "spot" } else { "final" },
        "generatedAt": chrono::Utc::now().to_rfc3339(),
        "call": {
            "id": rec.info.id,
            "direction": rec.info.direction,
            "number": rec.info.number,
            "name": rec.info.name,
            "uri": rec.info.uri,
            "state": rec.info.state,
            "reason": rec.info.reason,
            "missed": rec.missed,
            "codec": rec.info.codec,
            "srtp": rec.info.secure,
            "held": rec.info.held,
            "remoteHeld": rec.info.remote_held,
            "started": rec.info.started,
            "connectedAt": rec.info.connected_at,
            "endedAt": rec.ended_at,
            "talkTimeS": connected_s,
            "sipCallId": rec.info.sip_call_id,
            "dtmfReceived": rec.dtmf_rx,
            "dtmfSent": rec.dtmf_tx,
        },
        "summary": st.map(|s| json!({
            "mos": s.mos.map(round2),
            "rFactor": s.r_factor.map(round1),
            "rating": s.mos.map(rating),
            "lossPct": round2(s.loss_pct),
            "jitterMs": round1(s.jitter_ms),
            "minMos": moses.iter().cloned().reduce(f64::min).map(round2),
            "avgMos": (!moses.is_empty()).then(|| round2(moses.iter().sum::<f64>() / moses.len() as f64)),
        })),
        "media": st,
        "timeline": timeline,
        "findings": findings(rec, st, &timeline),
        "method": "MOS and R-factor are a simplified ITU-T G.107 E-model from this end's receive side: codec, \
    measured loss and jitter, our own buffering, plus a nominal 40 ms for network delay (there is no RTCP round-trip \
    measurement). Treat them as a comparison between calls, not a certified score.",
    });
    if rec.live {
        if let Some(h) = hub {
            let (mic, spk) = h.levels();
            v["levels"] = json!({
                "mic": round2(mic as f64),
                "speaker": round2(spk as f64),
                "speakerFreqHz": round1(h.speaker_freq() as f64),
            });
        }
    }
    v
}

/// Hide digest responses in SIP text (a hash of the password, guessable offline).
fn redact_auth(text: &str) -> String {
    text.lines()
        .map(|l| {
            let lower = l.to_ascii_lowercase();
            if !(lower.starts_with("authorization:") || lower.starts_with("proxy-authorization:")) {
                return l.to_string();
            }
            let mut out = l.to_string();
            if let Some(i) = lower.find("response=\"") {
                let start = i + "response=\"".len();
                if let Some(len) = out[start..].find('"') {
                    out.replace_range(start..start + len, REDACTED);
                }
            }
            out
        })
        .collect::<Vec<_>>()
        .join("\r\n")
}

// ------------------------------------------------------------------ the controller thread

type Job = Box<dyn FnOnce(&mut Core) + Send>;

enum Msg {
    Engine(Box<Event>),
    Ipc(String),
    Timer(Timer),
    Do(Job),
}

pub struct Core {
    ctl: Controller,
    tx: mpsc::Sender<Msg>,
    events: VecDeque<Value>,
    next_seq: u64,
    calls: BTreeMap<u64, CallRecord>,
    policy: DialPolicy,
}

impl Core {
    fn push_event(&mut self, kind: &str, mut body: Value) {
        self.next_seq += 1;
        body["seq"] = json!(self.next_seq);
        body["type"] = json!(kind);
        body["time"] = json!(chrono::Utc::now().timestamp_millis());
        if self.events.len() >= MAX_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(body);
    }

    fn record(&mut self, info: &CallInfo) {
        match self.calls.get_mut(&info.id) {
            Some(r) => r.update(info.clone()),
            None => {
                self.calls.insert(info.id, CallRecord::new(info.clone()));
            }
        }
    }

    fn on_engine(&mut self, ev: Event) {
        match &ev {
            Event::Accounts(a) => self.push_event("accounts", json!({ "accounts": a })),
            Event::Call(c) => {
                self.record(c);
                self.push_event("call", json!({ "call": c }));
            }
            Event::Incoming(c) => {
                self.record(c);
                self.push_event("incoming", json!({ "call": c }));
            }
            Event::Ended { info, missed } => {
                self.record(info);
                let fin = self.ctl.hub().and_then(|h| h.ended_stats(info.id));
                if let Some(r) = self.calls.get_mut(&info.id) {
                    r.live = false;
                    r.missed = *missed;
                    r.ended_at = Some(chrono::Utc::now().timestamp());
                    r.states.insert("ended".into());
                    r.final_stats = fin;
                }
                self.prune_ended();
                self.push_event("ended", json!({ "call": info, "missed": missed }));
            }
            Event::Dtmf(id, d) => {
                if let Some(r) = self.calls.get_mut(id) {
                    r.dtmf_rx.push(*d);
                }
                self.push_event("dtmf", json!({ "callId": id, "digit": d.to_string() }));
            }
            // The controller turns it into Out::Error, logged in `drain`.
            Event::Error(_) => {}
        }
        self.ctl.on_event(ev);
    }

    fn prune_ended(&mut self) {
        let ended: Vec<u64> = self
            .calls
            .iter()
            .filter(|(_, r)| !r.live)
            .map(|(id, _)| *id)
            .collect();
        if ended.len() > MAX_ENDED {
            for id in &ended[..ended.len() - MAX_ENDED] {
                self.calls.remove(id);
            }
        }
    }

    fn sample(&mut self) {
        let Some(h) = self.ctl.hub().cloned() else {
            return;
        };
        let (live, _) = h.stats();
        for s in live {
            if let Some(r) = self.calls.get_mut(&s.id).filter(|r| r.live) {
                r.sample(&s);
            }
        }
    }

    fn drain(&mut self) {
        for o in self.ctl.take_outbox() {
            match o {
                Out::Error(e) => self.push_event("error", json!({ "text": e })),
                Out::Schedule { after, timer } => {
                    let tx = self.tx.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(after);
                        let _ = tx.send(Msg::Timer(timer));
                    });
                }
                _ => {}
            }
        }
    }

    fn live_ids(&self) -> Vec<u64> {
        self.calls
            .iter()
            .filter(|(_, r)| r.live)
            .map(|(id, _)| *id)
            .collect()
    }

    /// `id`, or the only live call.
    fn live_call(&self, id: Option<u64>) -> Result<u64, String> {
        match id {
            Some(id) if self.calls.get(&id).is_some_and(|r| r.live) => Ok(id),
            Some(id) => Err(format!("Call {id} isn't in progress")),
            None => match self.live_ids().as_slice() {
                [one] => Ok(*one),
                [] => Err("No call is in progress".into()),
                many => Err(format!(
                    "Several calls are up ({many:?}); say which with call_id"
                )),
            },
        }
    }

    /// `id`, or the only live call, or the most recent one.
    fn any_call(&self, id: Option<u64>) -> Result<u64, String> {
        match id {
            Some(id) if self.calls.contains_key(&id) => Ok(id),
            Some(id) => Err(format!(
                "No call {id} (only the last {MAX_ENDED} ended calls are kept)"
            )),
            None => self.live_call(None).or_else(|_| {
                self.calls
                    .iter()
                    .max_by_key(|(_, r)| r.created)
                    .map(|(id, _)| *id)
                    .ok_or_else(|| "No calls yet".to_string())
            }),
        }
    }

    fn status(&self) -> Value {
        let active = self.ctl.active_account().unwrap_or_default();
        let s = self.ctl.settings();
        let acc = s.accounts.iter().find(|a| a.id == active);
        let reg: Value = serde_json::from_str(&self.ctl.accounts_json()).unwrap_or(json!([]));
        let reg = reg
            .as_array()
            .and_then(|l| l.iter().find(|a| a["id"] == json!(active)).cloned())
            .unwrap_or(Value::Null);
        let calls: Vec<&CallInfo> = self
            .calls
            .values()
            .filter(|r| r.live)
            .map(|r| &r.info)
            .collect();
        let (src, detail) = self.ctl.source_label();
        json!({
            "account": {
                "id": active,
                "title": acc.map(|a| a.title()),
                "shortName": acc.map(|a| a.short_name.clone()),
                "registration": reg,
            },
            "calls": calls,
            "muted": self.ctl.muted(),
            "conference": self.ctl.conference(),
            "source": if src.is_empty() { "microphone".to_string() } else { format!("{src} {detail}").trim().to_string() },
            "dnd": self.ctl.dnd(),
            "autoAnswer": s.auto_answer,
            "lastEventSeq": self.next_seq,
        })
    }

    fn registered(&self) -> bool {
        let active = self.ctl.active_account().unwrap_or_default();
        let reg: Value = serde_json::from_str(&self.ctl.accounts_json()).unwrap_or(json!([]));
        reg.as_array().is_some_and(|l| {
            l.iter()
                .any(|a| a["id"] == json!(active) && a["state"] == json!("registered"))
        })
    }

    /// Has `until` happened? Some(result) when it has, or can't any more.
    fn check(
        &self,
        until: &str,
        call: Option<u64>,
        digits: Option<&str>,
    ) -> Result<Option<Value>, String> {
        match until {
            "registered" => Ok(self.registered().then(|| json!({ "matched": true, "status": self.status() }))),
            "incoming" => {
                let hit = self.calls.values().find(|r| {
                    r.live
                        && r.info.direction == "in"
                        && r.info.state == "incoming"
                        && call.is_none_or(|c| c == r.info.id)
                });
                Ok(hit.map(|r| json!({ "matched": true, "call": r.info })))
            }
            "ringing" | "connected" | "ended" | "dtmf" => {
                let id = self.any_call(call)?;
                let r = &self.calls[&id];
                let hit = match until {
                    "ringing" => ["ringing", "early", "connected"].iter().any(|s| r.states.contains(*s)),
                    "connected" => r.states.contains("connected"),
                    "ended" => !r.live,
                    _ => match digits {
                        Some(d) if !d.is_empty() => r.dtmf_rx.contains(d),
                        _ => !r.dtmf_rx.is_empty(),
                    },
                };
                let out = |matched: bool| {
                    json!({
                        "matched": matched,
                        "call": r.info,
                        "ended": !r.live,
                        "dtmfReceived": r.dtmf_rx,
                    })
                };
                if hit {
                    Ok(Some(out(true)))
                } else if !r.live {
                    // It's over and never got there.
                    Ok(Some(out(false)))
                } else {
                    Ok(None)
                }
            }
            other => Err(format!(
                "Unknown condition \"{other}\": use registered, incoming, ringing, connected, ended or dtmf"
            )),
        }
    }
}

// ------------------------------------------------------------------ MCP tools

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DialArgs {
    /// A number, extension or sip:/tel: URI.
    pub number: String,
    /// Send this instead of the microphone for this call only: milli (1004 Hz), pink, white,
    /// tone:<hz>, or the path of a .wav/.ogg/.mp3 file. Only when no other call is up.
    pub source: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CallArg {
    /// The call's id (from dial, status or events). Optional when only one call is up.
    pub call_id: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HoldArgs {
    pub call_id: Option<u64>,
    /// true to hold, false to resume.
    pub on: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OnArg {
    pub on: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DtmfArgs {
    pub call_id: Option<u64>,
    /// Digits to send, in order: 0-9 * # A-D. A comma waits one gap more.
    pub digits: String,
    /// Milliseconds between digits (default 250, 80 to 5000).
    pub gap_ms: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TransferArgs {
    pub call_id: Option<u64>,
    /// Where to send the caller: a number or URI.
    pub target: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AttendedArgs {
    /// The call being transferred.
    pub call_id: u64,
    /// The consultation call to join it to.
    pub other_call_id: u64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SourceArgs {
    /// microphone (or mic), milli (1004 Hz), pink, white, tone:<hz>, or a .wav/.ogg/.mp3 path.
    /// Takes effect at once, on calls in progress too.
    pub source: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WaitArgs {
    /// registered | incoming | ringing | connected | ended | dtmf
    pub until: String,
    /// The call to watch (default: the only live call, else the latest). Not used for
    /// registered; for incoming, waits for that call only if given.
    pub call_id: Option<u64>,
    /// For until=dtmf: wait until the received digits contain this sequence.
    pub digits: Option<String>,
    /// Give up after this many seconds (default 30, at most 300).
    pub timeout_s: Option<f64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EventsArgs {
    /// Only events after this sequence number (default 0: everything kept).
    pub since: Option<u64>,
    /// At most this many, oldest first (default 100).
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TraceArgs {
    /// Only this call's dialog (default: every SIP message kept).
    pub call_id: Option<u64>,
    /// Include each message's full text (default false: one line per message).
    pub full: Option<bool>,
    /// The last this many messages (default 100).
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LimitArg {
    /// How many, newest first (default 20).
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SettingsPatch {
    /// The settings to change, shaped like get_settings' result but with only the keys to
    /// change, e.g. {"dtmfMethod": "info", "codecs": ["pcmu"]}. "accounts" is a list of
    /// changes: {"id": "...", "transport": "tcp"} edits one, {"id": "...", "delete": true}
    /// removes one, and an object without "id" adds one. Passwords can't be set here.
    pub patch: Value,
}

#[derive(Clone)]
pub struct Phone {
    tx: mpsc::Sender<Msg>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Phone>,
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

type ToolResult = Result<String, String>;

impl Phone {
    /// Run `f` on the controller thread and wait for its answer.
    async fn with<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Core) -> R + Send + 'static,
    ) -> Result<R, String> {
        let (otx, orx) = tokio::sync::oneshot::channel();
        self.tx
            .send(Msg::Do(Box::new(move |c| {
                let _ = otx.send(f(c));
            })))
            .map_err(|_| "MegaSIP's engine has stopped".to_string())?;
        orx.await
            .map_err(|_| "MegaSIP's engine has stopped".to_string())
    }

    /// Run a call action on `id` (or the only live call); reply with the call's state.
    async fn on_call(
        &self,
        id: Option<u64>,
        f: impl FnOnce(&mut Controller, u64) + Send + 'static,
    ) -> ToolResult {
        let id = self
            .with(move |c| {
                let id = c.live_call(id)?;
                f(&mut c.ctl, id);
                Ok::<_, String>(id)
            })
            .await??;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let info = self
            .with(move |c| {
                c.calls
                    .get(&id)
                    .map(|r| json!({ "callId": id, "call": r.info, "ended": !r.live }))
            })
            .await?;
        Ok(pretty(&info.unwrap_or(json!({ "callId": id }))))
    }
}

#[tool_router]
impl Phone {
    fn new(tx: mpsc::Sender<Msg>) -> Self {
        Phone {
            tx,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "The account and its registration state, the calls in progress, mute, conference, what is sent instead of the microphone, and the latest event sequence number."
    )]
    async fn status(&self) -> ToolResult {
        Ok(pretty(&self.with(|c| c.status()).await?))
    }

    #[tool(
        description = "Place a call. Returns the new call's id and state; follow with wait_for until=connected. Emergency numbers are refused."
    )]
    async fn dial(&self, Parameters(a): Parameters<DialArgs>) -> ToolResult {
        let number = crate::controller::dial_target(&a.number);
        let before = self
            .with(move |c| {
                c.policy.check(&number)?;
                if let Some(src) = &a.source {
                    Source::parse(src)?;
                    if !c.live_ids().is_empty() {
                        return Err("A per-call source needs no other call up (use set_source to switch for every call)".to_string());
                    }
                    if !c.ctl.switch_source(src) {
                        return Err(format!("Couldn't switch to {src}"));
                    }
                }
                let acc = c.ctl.active_account().unwrap_or_default().to_string();
                let before: BTreeSet<u64> = c.calls.keys().copied().collect();
                let seq = c.next_seq;
                c.ctl.dial(&number, &acc);
                Ok((before, seq))
            })
            .await??;
        let (before, seq) = before;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let b = before.clone();
            let found = self
                .with(move |c| {
                    let call = c
                        .calls
                        .values()
                        .find(|r| r.info.direction == "out" && !b.contains(&r.info.id))
                        .map(|r| r.info.clone());
                    let errors: Vec<Value> = c
                        .events
                        .iter()
                        .filter(|e| e["seq"].as_u64().unwrap_or(0) > seq && e["type"] == "error")
                        .cloned()
                        .collect();
                    (call, errors)
                })
                .await?;
            match found {
                (Some(call), _) => {
                    return Ok(pretty(&json!({ "callId": call.id, "call": call })));
                }
                (None, errors) if !errors.is_empty() => {
                    return Err(pretty(
                        &json!({ "error": "The call wasn't placed", "events": errors }),
                    ));
                }
                _ if Instant::now() > deadline => {
                    return Err("No call appeared within 5 s; check status and events".into());
                }
                _ => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    }

    #[tool(description = "Answer a ringing incoming call (default: the one ringing).")]
    async fn answer(&self, Parameters(a): Parameters<CallArg>) -> ToolResult {
        let id = match a.call_id {
            Some(id) => Some(id),
            None => {
                self.with(|c| {
                    c.calls
                        .values()
                        .find(|r| r.live && r.info.state == "incoming")
                        .map(|r| r.info.id)
                })
                .await?
            }
        };
        let id = id.ok_or("No incoming call is ringing")?;
        self.on_call(Some(id), |ctl, id| ctl.answer(id)).await
    }

    #[tool(description = "Reject a ringing incoming call (486 Busy Here).")]
    async fn reject(&self, Parameters(a): Parameters<CallArg>) -> ToolResult {
        self.on_call(a.call_id, |ctl, id| ctl.reject(id)).await
    }

    #[tool(description = "Hang up a call (or cancel it while it rings).")]
    async fn hangup(&self, Parameters(a): Parameters<CallArg>) -> ToolResult {
        self.on_call(a.call_id, |ctl, id| ctl.hangup(id)).await
    }

    #[tool(description = "Hang up every call.")]
    async fn hangup_all(&self) -> ToolResult {
        let n = self
            .with(|c| {
                c.ctl.hangup_all();
                c.live_ids().len()
            })
            .await?;
        Ok(format!("Hanging up {n} call(s)"))
    }

    #[tool(description = "Put a call on hold (on=true) or resume it (on=false).")]
    async fn hold(&self, Parameters(a): Parameters<HoldArgs>) -> ToolResult {
        let on = a.on;
        self.on_call(a.call_id, move |ctl, id| ctl.hold(id, on))
            .await
    }

    #[tool(description = "Mute (on=true) or unmute the outgoing audio on every call.")]
    async fn mute(&self, Parameters(a): Parameters<OnArg>) -> ToolResult {
        self.with(move |c| c.ctl.set_mute(a.on)).await?;
        Ok(format!("Muted: {}", a.on))
    }

    #[tool(
        description = "Join every call in progress into a local conference (on=true), or split it again."
    )]
    async fn conference(&self, Parameters(a): Parameters<OnArg>) -> ToolResult {
        self.with(move |c| c.ctl.set_conference(a.on)).await?;
        Ok(format!("Conference: {}", a.on))
    }

    #[tool(
        description = "Send DTMF digits on a call, one at a time, using the configured DTMF method (RFC 2833, SIP INFO or in-band)."
    )]
    async fn send_dtmf(&self, Parameters(a): Parameters<DtmfArgs>) -> ToolResult {
        let digits: Vec<char> = a.digits.chars().filter(|c| !c.is_whitespace()).collect();
        if let Some(bad) = digits
            .iter()
            .find(|c| !(c.is_ascii_digit() || "*#ABCDabcd,".contains(**c)))
        {
            return Err(format!(
                "'{bad}' isn't a DTMF digit (0-9 * # A-D, comma to pause)"
            ));
        }
        let gap = Duration::from_millis(a.gap_ms.unwrap_or(250).clamp(80, 5000));
        let call = a.call_id;
        let id = self.with(move |c| c.live_call(call)).await??;
        let mut sent = String::new();
        for d in digits {
            if d != ',' {
                let d = d.to_ascii_uppercase();
                let ok = self
                    .with(move |c| {
                        if !c.calls.get(&id).is_some_and(|r| r.live) {
                            return false;
                        }
                        c.ctl.dtmf(id, d);
                        if let Some(r) = c.calls.get_mut(&id) {
                            r.dtmf_tx.push(d);
                        }
                        true
                    })
                    .await?;
                if !ok {
                    return Err(format!("Call {id} ended after sending \"{sent}\""));
                }
                sent.push(d);
            }
            tokio::time::sleep(gap).await;
        }
        Ok(format!("Sent \"{sent}\" on call {id}"))
    }

    #[tool(description = "Blind transfer: send the other party of a call to `target` (SIP REFER).")]
    async fn transfer(&self, Parameters(a): Parameters<TransferArgs>) -> ToolResult {
        let target = crate::controller::dial_target(&a.target);
        let t = target.clone();
        self.with(move |c| c.policy.check(&t)).await??;
        self.on_call(a.call_id, move |ctl, id| ctl.transfer(id, &target))
            .await
    }

    #[tool(
        description = "Attended transfer: connect call_id's party to other_call_id's party (REFER with Replaces)."
    )]
    async fn attended_transfer(&self, Parameters(a): Parameters<AttendedArgs>) -> ToolResult {
        let other = a.other_call_id;
        self.with(move |c| c.live_call(Some(other)).map(|_| ()))
            .await??;
        self.on_call(Some(a.call_id), move |ctl, id| {
            ctl.attended_transfer(id, other)
        })
        .await
    }

    #[tool(
        description = "Choose what is sent instead of the microphone, from now on and on calls in progress: microphone, milli (1004 Hz), pink, white, tone:<hz>, or a .wav/.ogg/.mp3 path."
    )]
    async fn set_source(&self, Parameters(a): Parameters<SourceArgs>) -> ToolResult {
        let src = match a.source.trim() {
            "mic" | "microphone" | "" => None,
            s => Some(Source::parse(s)?),
        };
        let label = src
            .as_ref()
            .map(|s| s.label())
            .unwrap_or_else(|| "microphone".into());
        self.with(move |c| c.ctl.set_source(src)).await?;
        Ok(format!("Now sending: {label}"))
    }

    #[tool(
        description = "Wait until something happens, or the timeout passes: until=registered (the account), incoming (a call rings in), ringing (the far end rings), connected, ended, or dtmf (digits received; give `digits` to wait for a sequence). Returns at once if it already happened, and returns matched=false if the call ended without getting there."
    )]
    async fn wait_for(&self, Parameters(a): Parameters<WaitArgs>) -> ToolResult {
        let timeout = a.timeout_s.unwrap_or(30.0).clamp(0.0, 300.0);
        let start = Instant::now();
        loop {
            let (until, call, digits) = (a.until.clone(), a.call_id, a.digits.clone());
            let hit = self
                .with(move |c| c.check(&until, call, digits.as_deref()))
                .await??;
            if let Some(mut v) = hit {
                v["waitedS"] = json!(round1(start.elapsed().as_secs_f64()));
                return Ok(pretty(&v));
            }
            if start.elapsed().as_secs_f64() >= timeout {
                let st = self.with(|c| c.status()).await?;
                return Ok(pretty(&json!({
                    "matched": false,
                    "timedOut": true,
                    "waitedS": round1(timeout),
                    "status": st,
                })));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    #[tool(
        description = "Everything that happened since a sequence number: registration changes, call state changes, incoming calls, received DTMF, ended calls and errors. Pass the returned `next` as `since` to continue."
    )]
    async fn events(&self, Parameters(a): Parameters<EventsArgs>) -> ToolResult {
        let since = a.since.unwrap_or(0);
        let limit = a.limit.unwrap_or(100).clamp(1, MAX_EVENTS);
        let v = self
            .with(move |c| {
                let list: Vec<Value> = c
                    .events
                    .iter()
                    .filter(|e| e["seq"].as_u64().unwrap_or(0) > since)
                    .take(limit)
                    .cloned()
                    .collect();
                let next = list.last().and_then(|e| e["seq"].as_u64()).unwrap_or(since);
                json!({ "next": next, "latest": c.next_seq, "events": list })
            })
            .await?;
        Ok(pretty(&v))
    }

    #[tool(
        description = "Call quality report. During a call it is a spot report of the media so far (with live mic/speaker levels and the dominant received frequency); after the call it is the final report. Includes MOS, R-factor, loss, jitter, packet counts, a timeline in 5 s windows and plain-language findings. Default call: the one in progress, else the latest."
    )]
    async fn call_quality_report(&self, Parameters(a): Parameters<CallArg>) -> ToolResult {
        let v = self
            .with(move |c| {
                c.sample();
                let id = c.any_call(a.call_id)?;
                Ok::<_, String>(quality_report(
                    &c.calls[&id],
                    c.ctl.hub().map(|h| h.as_ref()),
                ))
            })
            .await??;
        Ok(pretty(&v))
    }

    #[tool(
        description = "The SIP messages sent and received (the Debug window's capture), for one call's dialog or all of them. Digest responses are redacted."
    )]
    async fn sip_trace(&self, Parameters(a): Parameters<TraceArgs>) -> ToolResult {
        let sip_id = match a.call_id {
            Some(id) => Some(
                self.with(move |c| c.calls.get(&id).map(|r| r.info.sip_call_id.clone()))
                    .await?
                    .ok_or(format!("No call {id}"))?,
            ),
            None => None,
        };
        let all: Vec<Value> = serde_json::from_str(&crate::debug::sip_since(0)).unwrap_or_default();
        let full = a.full.unwrap_or(false);
        let mut list: Vec<Value> = all
            .into_iter()
            .filter(|m| sip_id.as_ref().is_none_or(|id| m["callId"] == json!(id)))
            .map(|mut m| {
                if full {
                    let t = m["text"].as_str().map(redact_auth).unwrap_or_default();
                    m["text"] = json!(t);
                } else if let Some(o) = m.as_object_mut() {
                    o.remove("text");
                }
                m
            })
            .collect();
        let limit = a.limit.unwrap_or(100).max(1);
        if list.len() > limit {
            list.drain(..list.len() - limit);
        }
        Ok(pretty(&json!({ "messages": list })))
    }

    #[tool(description = "This account's call history, newest first.")]
    async fn call_history(&self, Parameters(a): Parameters<LimitArg>) -> ToolResult {
        let limit = a.limit.unwrap_or(20).max(1);
        let h = self.with(|c| c.ctl.history_json()).await?;
        let list: Vec<Value> = serde_json::from_str(&h).unwrap_or_default();
        Ok(pretty(&json!(list
            .into_iter()
            .take(limit)
            .collect::<Vec<_>>())))
    }

    #[tool(
        description = "All settings and accounts, as MegaSIP stores them. Account passwords read as \"<redacted>\" when set and \"\" when not."
    )]
    async fn get_settings(&self) -> ToolResult {
        Ok(pretty(
            &self.with(|c| redacted_settings(c.ctl.settings())).await?,
        ))
    }

    #[tool(
        description = "Change settings: pass only the keys to change (see get_settings for names and values). Accounts are edited by id, deleted with {\"id\", \"delete\": true}, or added without an id. Passwords can't be set or changed here. Returns what changed and the new settings."
    )]
    async fn update_settings(&self, Parameters(a): Parameters<SettingsPatch>) -> ToolResult {
        let v = self
            .with(move |c| {
                let active = c.ctl.active_account().map(str::to_string);
                let (s, changed) = apply_patch(c.ctl.settings(), &a.patch, active.as_deref())?;
                c.ctl.apply_settings(None, s)?;
                Ok::<_, String>(
                    json!({ "changed": changed, "settings": redacted_settings(c.ctl.settings()) }),
                )
            })
            .await??;
        Ok(pretty(&v))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Phone {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("megasip", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}

// ------------------------------------------------------------------ entry point

/// Serve MCP on stdin/stdout as account `account_id` until the client goes away. Returns
/// the exit code; Err if the account couldn't be activated.
pub fn run(account_id: String, policy: DialPolicy) -> Result<i32, String> {
    let (tx, rx) = mpsc::channel::<Msg>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    crate::ipc::listen();
    if let Some(ipc) = crate::ipc::take_receiver() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in ipc {
                if tx.send(Msg::Ipc(line)).is_err() {
                    break;
                }
            }
        });
    }
    let ctl_tx = tx.clone();
    std::thread::Builder::new()
        .name("mcp-controller".into())
        .spawn(move || {
            let (mut ctl, mut ev_rx) = Controller::start();
            let etx = ctl_tx.clone();
            std::thread::spawn(move || {
                while let Some(ev) = ev_rx.blocking_recv() {
                    if etx.send(Msg::Engine(Box::new(ev))).is_err() {
                        break;
                    }
                }
            });
            ctl.activate(&account_id);
            if ctl.active_account() != Some(account_id.as_str()) {
                let errs: Vec<String> = ctl
                    .take_outbox()
                    .into_iter()
                    .filter_map(|o| match o {
                        Out::Error(e) => Some(e),
                        _ => None,
                    })
                    .collect();
                if let Ok(c) = crate::db::open() {
                    crate::db::unregister_self(&c);
                }
                let _ = ready_tx.send(Err(errs.join("; ")));
                return;
            }
            let mut core = Core {
                ctl,
                tx: ctl_tx,
                events: VecDeque::new(),
                next_seq: 0,
                calls: BTreeMap::new(),
                policy,
            };
            core.drain();
            let _ = ready_tx.send(Ok(()));
            let mut next_tick = Instant::now() + Duration::from_secs(1);
            loop {
                let wait = next_tick.saturating_duration_since(Instant::now());
                match rx.recv_timeout(wait) {
                    Ok(Msg::Engine(ev)) => core.on_engine(*ev),
                    Ok(Msg::Ipc(line)) => core.ctl.on_ipc(&line),
                    Ok(Msg::Timer(t)) => core.ctl.on_timer(t),
                    Ok(Msg::Do(job)) => job(&mut core),
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        core.sample();
                        next_tick = (next_tick + Duration::from_secs(1)).max(Instant::now());
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
                core.drain();
            }
        })
        .map_err(|e| e.to_string())?;
    ready_rx
        .recv()
        .map_err(|_| "the controller didn't start".to_string())??;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let phone = Phone::new(tx.clone());
    let code = rt.block_on(async move {
        match phone.serve(rmcp::transport::stdio()).await {
            Ok(service) => {
                let _ = service.waiting().await;
                0
            }
            Err(e) => {
                tracing::error!("mcp: couldn't start: {e}");
                1
            }
        }
    });
    // The client went away: hang up, un-register, and exit (quit doesn't return).
    let _ = tx.send(Msg::Do(Box::new(|c| c.ctl.quit())));
    std::thread::sleep(Duration::from_secs(3));
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        let mut s = Settings::default();
        s.accounts.push(Account {
            id: "a1".into(),
            label: "Main".into(),
            short_name: "main".into(),
            server: "sip.example.com".into(),
            username: "100".into(),
            password: "hunter2".into(),
            ..Default::default()
        });
        s.accounts.push(Account {
            id: "a2".into(),
            label: "Other".into(),
            short_name: "other".into(),
            server: "sip.example.com".into(),
            username: "101".into(),
            ..Default::default()
        });
        s
    }

    #[test]
    fn passwords_read_redacted() {
        let v = redacted_settings(&settings());
        assert_eq!(v["accounts"][0]["password"], json!(REDACTED));
        assert_eq!(v["accounts"][1]["password"], json!(""));
        assert!(!v.to_string().contains("hunter2"));
    }

    #[test]
    fn passwords_cannot_be_set() {
        let s = settings();
        for pw in ["new", "", "hunter2"] {
            let p = json!({"accounts": [{"id": "a1", "password": pw}]});
            assert_eq!(apply_patch(&s, &p, None).unwrap_err(), PASSWORD_REFUSED);
            let p = json!({"accounts": [{"id": "a2", "password": pw}]});
            assert!(apply_patch(&s, &p, None).is_err());
            let p = json!({"accounts": [{"label": "New", "password": pw}]});
            assert!(apply_patch(&s, &p, None).is_err());
        }
    }

    #[test]
    fn a_read_modify_write_round_trip_keeps_the_password() {
        let s = settings();
        let mut a0 = redacted_settings(&s)["accounts"][0].clone();
        a0["transport"] = json!("tcp");
        let (n, changed) = apply_patch(&s, &json!({"accounts": [a0]}), None).unwrap();
        assert_eq!(n.accounts[0].transport, "tcp");
        assert_eq!(n.accounts[0].password, "hunter2");
        assert_eq!(changed, ["accounts[a1]"]);
    }

    #[test]
    fn settings_patch_merges_and_refuses_unknown_keys() {
        let s = settings();
        let (n, _) =
            apply_patch(&s, &json!({"dtmfMethod": "info", "codecs": ["pcmu"]}), None).unwrap();
        assert_eq!(n.dtmf_method, "info");
        assert_eq!(n.codecs, ["pcmu"]);
        assert_eq!(n.accounts, s.accounts);
        assert!(apply_patch(&s, &json!({"noSuchThing": 1}), None).is_err());
        assert!(apply_patch(&s, &json!({"accounts": [{"id": "a1", "nope": 1}]}), None).is_err());
        assert!(apply_patch(&s, &json!({"accounts": [{"id": "zz", "label": "x"}]}), None).is_err());
        assert!(apply_patch(&s, &json!({"dnd": null}), None).is_err());
    }

    #[test]
    fn accounts_can_be_added_and_deleted_but_not_the_running_one() {
        let s = settings();
        let (n, _) = apply_patch(
            &s,
            &json!({"accounts": [{"label": "Lab", "shortName": "lab", "server": "10.0.0.1", "username": "200"}]}),
            Some("a1"),
        )
        .unwrap();
        assert_eq!(n.accounts.len(), 3);
        assert_eq!(n.accounts[2].password, "");
        assert!(!n.accounts[2].id.is_empty());
        let (n, _) = apply_patch(
            &s,
            &json!({"accounts": [{"id": "a2", "delete": true}]}),
            Some("a1"),
        )
        .unwrap();
        assert_eq!(n.accounts.len(), 1);
        assert!(apply_patch(
            &s,
            &json!({"accounts": [{"id": "a1", "delete": true}]}),
            Some("a1")
        )
        .is_err());
    }

    #[test]
    fn emergency_numbers_are_refused() {
        let p = DialPolicy::default();
        for n in [
            "911",
            "9911",
            "tel:911",
            "sip:911@example.com",
            "112",
            "999",
            "000",
            "1-1-2",
        ] {
            assert!(p.check(n).is_err(), "{n} should be refused");
        }
        for n in [
            "933",
            "9000",
            "18005551212",
            "sip:echo@example.com",
            "100",
            "3346661338",
        ] {
            assert!(p.check(n).is_ok(), "{n} should be allowed");
        }
    }

    #[test]
    fn allow_list_limits_targets() {
        let p = DialPolicy {
            allow: vec!["1555".into(), "sip:test".into()],
        };
        assert!(p.check("15551234567").is_ok());
        assert!(p.check("sip:test@lab").is_ok());
        assert!(p.check("18005551212").is_err());
    }

    #[test]
    fn digest_responses_are_hidden() {
        let t = "INVITE sip:1@x SIP/2.0\r\nAuthorization: Digest username=\"100\", response=\"abcdef\", nonce=\"n\"\r\nTo: <sip:1@x>";
        let r = redact_auth(t);
        assert!(!r.contains("abcdef"));
        assert!(r.contains("nonce=\"n\""));
        assert!(r.contains("To: <sip:1@x>"));
    }

    fn sample(t: f64, expected: u64, lost: u64) -> Sample {
        Sample {
            t,
            expected,
            lost,
            jitter_ms: 1.0,
            mos: Some(4.2),
            last_rx_ms: Some(20),
        }
    }

    #[test]
    fn timeline_reports_loss_per_window() {
        let mut s = VecDeque::new();
        // 50 packets/s; no loss for 5 s, then 10% loss.
        for i in 0..=10u64 {
            let lost = if i > 5 { (i - 5) * 5 } else { 0 };
            s.push_back(sample(i as f64, i * 50, lost));
        }
        let t = intervals(&s);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0]["lossPct"], json!(0.0));
        assert_eq!(t[1]["lossPct"], json!(10.0));
    }

    #[test]
    fn ratings() {
        assert_eq!(rating(4.4), "excellent");
        assert_eq!(rating(4.1), "good");
        assert_eq!(rating(3.7), "fair");
        assert_eq!(rating(3.2), "poor");
        assert_eq!(rating(2.0), "bad");
    }
}
