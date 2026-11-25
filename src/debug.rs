//! In-memory capture for the Debug window: every SIP message sent or received, and recent
//! log events. Always on and bounded, so the history is there when you open the window
//! after something went wrong.

use crate::engine::msg::SipMsg;
use serde::Serialize;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_SIP: usize = 2000;
const MAX_LOGS: usize = 5000;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SipRecord {
    pub seq: u64,
    /// Unix time in milliseconds.
    pub t: i64,
    /// "out" | "in"
    pub dir: &'static str,
    pub transport: String,
    pub remote: String,
    /// Request line or status line.
    pub line: String,
    /// Method for requests, CSeq method for responses.
    pub method: String,
    /// Status code for responses, 0 for requests.
    pub code: u16,
    pub call_id: String,
    pub cseq: String,
    pub from_user: String,
    pub to_user: String,
    pub sdp: bool,
    pub text: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRecord {
    pub seq: u64,
    pub t: i64,
    pub level: String,
    /// "sip" | "audio"
    pub category: &'static str,
    pub target: String,
    pub message: String,
}

struct Store {
    sip: VecDeque<SipRecord>,
    logs: VecDeque<LogRecord>,
    sip_seq: u64,
    log_seq: u64,
}

static STORE: Mutex<Store> = Mutex::new(Store { sip: VecDeque::new(), logs: VecDeque::new(), sip_seq: 0, log_seq: 0 });

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn lock() -> std::sync::MutexGuard<'static, Store> {
    STORE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Record a SIP message. `transport` is "UDP"/"TCP"/"TLS".
pub fn sip(dir: &'static str, transport: &str, remote: SocketAddr, data: &[u8]) {
    let text = String::from_utf8_lossy(data).into_owned();
    let parsed = SipMsg::parse(data);
    let (line, method, code, call_id, cseq, from_user, to_user, sdp) = match &parsed {
        Some(m) => {
            let line = text.lines().next().unwrap_or("").trim().to_string();
            let (n, cm) = m.cseq();
            let user = |h: Option<&str>| {
                h.map(|v| crate::engine::msg::uri_user(&crate::engine::msg::parse_name_addr(v).uri))
                    .unwrap_or_default()
            };
            let method = if m.is_request() { m.method().to_string() } else { cm.clone() };
            (
                line,
                method,
                m.code(),
                m.call_id().to_string(),
                format!("{n} {cm}"),
                user(m.header("From")),
                user(m.header("To")),
                m.has_sdp(),
            )
        }
        None => (text.lines().next().unwrap_or("").to_string(), String::new(), 0, String::new(), String::new(), String::new(), String::new(), false),
    };
    let mut s = lock();
    s.sip_seq += 1;
    let rec = SipRecord {
        seq: s.sip_seq,
        t: now_ms(),
        dir,
        transport: transport.to_string(),
        remote: remote.to_string(),
        line,
        method,
        code,
        call_id,
        cseq,
        from_user,
        to_user,
        sdp,
        text,
    };
    s.sip.push_back(rec);
    while s.sip.len() > MAX_SIP {
        s.sip.pop_front();
    }
}

pub fn log(level: &str, category: &'static str, target: &str, message: String) {
    let mut s = lock();
    s.log_seq += 1;
    let rec = LogRecord { seq: s.log_seq, t: now_ms(), level: level.to_string(), category, target: target.to_string(), message };
    s.logs.push_back(rec);
    while s.logs.len() > MAX_LOGS {
        s.logs.pop_front();
    }
}

/// SIP messages newer than `since` (a seq number), as JSON.
pub fn sip_since(since: u64) -> String {
    let s = lock();
    let v: Vec<&SipRecord> = s.sip.iter().filter(|r| r.seq > since).collect();
    serde_json::to_string(&v).unwrap_or_else(|_| "[]".into())
}

pub fn logs_since(since: u64) -> String {
    let s = lock();
    let v: Vec<&LogRecord> = s.logs.iter().filter(|r| r.seq > since).collect();
    serde_json::to_string(&v).unwrap_or_else(|_| "[]".into())
}

pub fn clear() {
    let mut s = lock();
    s.sip.clear();
    s.logs.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_pages() {
        clear();
        let before = { lock().sip_seq };
        let invite = b"INVITE sip:1002@pbx SIP/2.0\r\nVia: SIP/2.0/UDP 1.2.3.4;branch=z9hG4bKx\r\nFrom: <sip:1001@pbx>;tag=a\r\nTo: <sip:1002@pbx>\r\nCall-ID: abc\r\nCSeq: 7 INVITE\r\nContent-Type: application/sdp\r\nContent-Length: 3\r\n\r\nv=0";
        sip("out", "UDP", "10.0.0.1:5060".parse().unwrap(), invite);
        let v: serde_json::Value = serde_json::from_str(&sip_since(before)).unwrap();
        let r = &v[0];
        assert_eq!(r["method"], "INVITE");
        assert_eq!(r["callId"], "abc");
        assert_eq!(r["toUser"], "1002");
        assert_eq!(r["sdp"], true);
        assert_eq!(serde_json::from_str::<serde_json::Value>(&sip_since(before + 1)).unwrap().as_array().unwrap().len(), 0);
    }
}
