//! SIP message parsing and serialisation.
//!
//! Messages are kept as a start line plus an ordered header list so that any header
//! (Refer-To, Replaces, Event, P-Asserted-Identity, ...) can be read, and so responses can
//! copy Via/Record-Route verbatim as RFC 3261 requires.

use std::fmt::Write as _;

#[derive(Debug, Clone)]
pub enum StartLine {
    Request { method: String, uri: String },
    Response { code: u16, reason: String },
}

#[derive(Debug, Clone)]
pub struct SipMsg {
    pub start: StartLine,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Expand RFC 3261 compact header forms.
fn canonical(name: &str) -> String {
    let n = name.trim();
    let full = match n.to_ascii_lowercase().as_str() {
        "i" => "Call-ID",
        "m" => "Contact",
        "e" => "Content-Encoding",
        "l" => "Content-Length",
        "c" => "Content-Type",
        "f" => "From",
        "s" => "Subject",
        "k" => "Supported",
        "t" => "To",
        "v" => "Via",
        "r" => "Refer-To",
        "b" => "Referred-By",
        "o" => "Event",
        "u" => "Allow-Events",
        _ => return n.to_string(),
    };
    full.to_string()
}

/// Headers whose values may be comma-joined lists that we want to split.
fn is_list_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "via" | "route" | "record-route" | "contact"
    )
}

/// Split a header value on commas that are not inside quotes or angle brackets.
pub fn split_list(v: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut q, mut a) = (false, 0i32);
    let mut prev_bs = false;
    for ch in v.chars() {
        match ch {
            '"' if !prev_bs => q = !q,
            '<' if !q => a += 1,
            '>' if !q => a -= 1,
            ',' if !q && a <= 0 => {
                if !cur.trim().is_empty() {
                    out.push(cur.trim().to_string());
                }
                cur.clear();
                prev_bs = false;
                continue;
            }
            _ => {}
        }
        prev_bs = ch == '\\' && !prev_bs;
        cur.push(ch);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

impl SipMsg {
    pub fn request(method: &str, uri: &str) -> Self {
        SipMsg {
            start: StartLine::Request { method: method.to_string(), uri: uri.to_string() },
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub fn response(code: u16, reason: &str) -> Self {
        SipMsg {
            start: StartLine::Response { code, reason: reason.to_string() },
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// Parse one complete message. Returns None for garbage (including keep-alive CRLFs).
    pub fn parse(data: &[u8]) -> Option<SipMsg> {
        let split = find_subslice(data, b"\r\n\r\n")
            .map(|i| (i, i + 4))
            .or_else(|| find_subslice(data, b"\n\n").map(|i| (i, i + 2)))?;
        let head = std::str::from_utf8(&data[..split.0]).ok()?;
        let mut lines: Vec<String> = Vec::new();
        for raw in head.split('\n') {
            let line = raw.trim_end_matches('\r');
            if (line.starts_with(' ') || line.starts_with('\t')) && !lines.is_empty() {
                let last = lines.last_mut().unwrap();
                last.push(' ');
                last.push_str(line.trim());
            } else if !line.is_empty() || !lines.is_empty() {
                lines.push(line.to_string());
            }
        }
        let first = lines.first()?.trim().to_string();
        let start = if let Some(rest) = first.strip_prefix("SIP/2.0 ") {
            let (code, reason) = rest.split_once(' ').unwrap_or((rest, ""));
            StartLine::Response { code: code.trim().parse().ok()?, reason: reason.to_string() }
        } else {
            let mut it = first.split_whitespace();
            let method = it.next()?.to_string();
            let uri = it.next()?.to_string();
            if it.next()? != "SIP/2.0" {
                return None;
            }
            StartLine::Request { method, uri }
        };
        let mut headers = Vec::new();
        for l in &lines[1..] {
            let Some((n, v)) = l.split_once(':') else { continue };
            let name = canonical(n);
            let value = v.trim();
            if is_list_header(&name) && !value.starts_with('*') {
                for part in split_list(value) {
                    headers.push((name.clone(), part));
                }
            } else {
                headers.push((name, value.to_string()));
            }
        }
        let mut msg = SipMsg { start, headers, body: Vec::new() };
        let body = &data[split.1..];
        let len = msg
            .header("Content-Length")
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(body.len());
        msg.body = body[..len.min(body.len())].to_vec();
        Some(msg)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut s = String::with_capacity(512 + self.body.len());
        match &self.start {
            StartLine::Request { method, uri } => {
                let _ = write!(s, "{method} {uri} SIP/2.0\r\n");
            }
            StartLine::Response { code, reason } => {
                let _ = write!(s, "SIP/2.0 {code} {reason}\r\n");
            }
        }
        for (n, v) in &self.headers {
            if n.eq_ignore_ascii_case("Content-Length") {
                continue;
            }
            let _ = write!(s, "{n}: {v}\r\n");
        }
        let _ = write!(s, "Content-Length: {}\r\n\r\n", self.body.len());
        let mut out = s.into_bytes();
        out.extend_from_slice(&self.body);
        out
    }

    pub fn is_request(&self) -> bool {
        matches!(self.start, StartLine::Request { .. })
    }

    pub fn method(&self) -> &str {
        match &self.start {
            StartLine::Request { method, .. } => method,
            StartLine::Response { .. } => "",
        }
    }

    pub fn uri(&self) -> &str {
        match &self.start {
            StartLine::Request { uri, .. } => uri,
            StartLine::Response { .. } => "",
        }
    }

    pub fn code(&self) -> u16 {
        match &self.start {
            StartLine::Response { code, .. } => *code,
            StartLine::Request { .. } => 0,
        }
    }

    pub fn reason(&self) -> &str {
        match &self.start {
            StartLine::Response { reason, .. } => reason,
            StartLine::Request { .. } => "",
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn headers_all(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .collect()
    }

    pub fn add(&mut self, name: &str, value: impl Into<String>) -> &mut Self {
        self.headers.push((name.to_string(), value.into()));
        self
    }

    pub fn set(&mut self, name: &str, value: impl Into<String>) -> &mut Self {
        self.headers.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        self.headers.push((name.to_string(), value.into()));
        self
    }

    pub fn remove(&mut self, name: &str) {
        self.headers.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
    }

    pub fn set_body(&mut self, content_type: &str, body: Vec<u8>) {
        self.remove("Content-Type");
        if !body.is_empty() {
            self.add("Content-Type", content_type);
        }
        self.body = body;
    }

    pub fn call_id(&self) -> &str {
        self.header("Call-ID").unwrap_or("")
    }

    pub fn cseq(&self) -> (u32, String) {
        let v = self.header("CSeq").unwrap_or("");
        let mut it = v.split_whitespace();
        let n = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        let m = it.next().unwrap_or("").to_ascii_uppercase();
        (n, m)
    }

    #[allow(clippy::wrong_self_convention)] // "From tag" is the SIP header, not a conversion
    pub fn from_tag(&self) -> Option<String> {
        self.header("From").and_then(|v| param(v, "tag"))
    }

    pub fn to_tag(&self) -> Option<String> {
        self.header("To").and_then(|v| param(v, "tag"))
    }

    /// Branch of the top Via.
    pub fn branch(&self) -> Option<String> {
        self.header("Via").and_then(|v| param(v, "branch"))
    }

    pub fn content_type(&self) -> String {
        self.header("Content-Type")
            .map(|v| v.split(';').next().unwrap_or("").trim().to_ascii_lowercase())
            .unwrap_or_default()
    }

    pub fn has_sdp(&self) -> bool {
        !self.body.is_empty() && self.content_type() == "application/sdp"
    }

    pub fn body_str(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// Build a response to this request, copying Via, From, To, Call-ID, CSeq and
    /// (for dialog-creating responses) Record-Route.
    pub fn make_response(&self, code: u16, reason: &str, to_tag: Option<&str>) -> SipMsg {
        let mut r = SipMsg::response(code, reason);
        for v in self.headers_all("Via") {
            r.add("Via", v);
        }
        if let Some(f) = self.header("From") {
            r.add("From", f);
        }
        if let Some(t) = self.header("To") {
            let mut t = t.to_string();
            if let Some(tag) = to_tag {
                if param(&t, "tag").is_none() && code > 100 {
                    t.push_str(";tag=");
                    t.push_str(tag);
                }
            }
            r.add("To", t);
        }
        r.add("Call-ID", self.call_id());
        if let Some(c) = self.header("CSeq") {
            r.add("CSeq", c);
        }
        if (101..300).contains(&code) {
            for rr in self.headers_all("Record-Route") {
                r.add("Record-Route", rr);
            }
        }
        r
    }
}

fn find_subslice(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

/// For a stream transport: if `buf` holds a complete message, return its length.
pub fn frame_len(buf: &[u8]) -> Option<usize> {
    let end = find_subslice(buf, b"\r\n\r\n")? + 4;
    let head = String::from_utf8_lossy(&buf[..end]);
    let mut len = 0usize;
    for line in head.split("\r\n") {
        if let Some((n, v)) = line.split_once(':') {
            let n = n.trim();
            if n.eq_ignore_ascii_case("content-length") || n == "l" {
                len = v.trim().parse().unwrap_or(0);
            }
        }
    }
    (buf.len() >= end + len).then_some(end + len)
}

/// Read a `;name=value` parameter from a header value (outside the `<...>` URI when present).
pub fn param(v: &str, name: &str) -> Option<String> {
    let tail = match v.rfind('>') {
        Some(i) => &v[i + 1..],
        None => v,
    };
    for p in tail.split(';').skip(if v.contains('>') { 0 } else { 1 }) {
        let p = p.trim();
        let (k, val) = p.split_once('=').unwrap_or((p, ""));
        if k.trim().eq_ignore_ascii_case(name) {
            return Some(val.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// A parsed name-addr such as `"Alice" <sip:alice@example.com>;tag=1`.
#[derive(Debug, Clone, Default)]
pub struct NameAddr {
    pub display: String,
    pub uri: String,
}

pub fn parse_name_addr(v: &str) -> NameAddr {
    let v = v.trim();
    if let (Some(l), Some(r)) = (v.find('<'), v.rfind('>')) {
        if l < r {
            let display = v[..l].trim().trim_matches('"').replace("\\\"", "\"");
            return NameAddr { display, uri: v[l + 1..r].trim().to_string() };
        }
    }
    let uri = v.split(';').next().unwrap_or("").trim().to_string();
    NameAddr { display: String::new(), uri }
}

/// The user part of a SIP URI (`sip:1001@host` -> `1001`), or the whole thing for tel:.
pub fn uri_user(uri: &str) -> String {
    let u = uri.trim();
    let u = u
        .strip_prefix("sips:")
        .or_else(|| u.strip_prefix("sip:"))
        .or_else(|| u.strip_prefix("tel:"))
        .unwrap_or(u);
    let u = u.split(';').next().unwrap_or(u);
    let u = u.split('?').next().unwrap_or(u);
    match u.split_once('@') {
        Some((user, _)) => user.split(':').next().unwrap_or(user).to_string(),
        None => u.to_string(),
    }
}

/// Host[:port] of a SIP URI.
pub fn uri_hostport(uri: &str) -> String {
    let u = uri.trim().trim_start_matches('<');
    let u = u
        .strip_prefix("sips:")
        .or_else(|| u.strip_prefix("sip:"))
        .unwrap_or(u);
    let u = u.split(';').next().unwrap_or(u);
    let u = u.split('?').next().unwrap_or(u);
    let u = u.trim_end_matches('>');
    match u.rsplit_once('@') {
        Some((_, h)) => h.to_string(),
        None => u.to_string(),
    }
}

/// A URI parameter such as `transport` from `sip:host;transport=tcp;lr`.
#[cfg(test)]
pub fn uri_param(uri: &str, name: &str) -> Option<String> {
    let u = uri.trim().trim_start_matches('<');
    let u = u.split('>').next().unwrap_or(u);
    let u = u.split('?').next().unwrap_or(u);
    for p in u.split(';').skip(1) {
        let (k, v) = p.split_once('=').unwrap_or((p, ""));
        if k.trim().eq_ignore_ascii_case(name) {
            return Some(v.trim().to_string());
        }
    }
    None
}

/// Percent-escape for URI headers (`?Replaces=...`).
pub fn uri_escape(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => {
                out.push(b as char)
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

pub fn uri_unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz");
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Quote a display name for a From header.
pub fn quote_display(s: &str) -> String {
    let clean: String = s.chars().filter(|c| *c != '\r' && *c != '\n').collect();
    format!("\"{}\"", clean.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn random_token(n: usize) -> String {
    use rand::Rng;
    const CH: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut r = rand::thread_rng();
    (0..n).map(|_| CH[r.gen_range(0..CH.len())] as char).collect()
}

pub fn new_branch() -> String {
    format!("z9hG4bK{}", random_token(16))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_round_trips() {
        let raw = b"INVITE sip:bob@example.com SIP/2.0\r\nVia: SIP/2.0/UDP 10.0.0.1:5060;branch=z9hG4bKabc;rport, SIP/2.0/UDP 10.0.0.2\r\nf: \"Alice\" <sip:alice@example.com>;tag=123\r\nt: <sip:bob@example.com>\r\ni: abc@host\r\nCSeq: 1 INVITE\r\nRecord-Route: <sip:p1;lr>,<sip:p2;lr>\r\nl: 4\r\n\r\nv=0\r\n";
        let m = SipMsg::parse(raw).unwrap();
        assert_eq!(m.method(), "INVITE");
        assert_eq!(m.headers_all("Via").len(), 2);
        assert_eq!(m.branch().as_deref(), Some("z9hG4bKabc"));
        assert_eq!(m.from_tag().as_deref(), Some("123"));
        assert_eq!(m.headers_all("Record-Route").len(), 2);
        assert_eq!(m.body, b"v=0\r");
        let na = parse_name_addr(m.header("From").unwrap());
        assert_eq!(na.display, "Alice");
        assert_eq!(uri_user(&na.uri), "alice");
        let r = m.make_response(200, "OK", Some("xyz"));
        let r2 = SipMsg::parse(&r.to_bytes()).unwrap();
        assert_eq!(r2.code(), 200);
        assert_eq!(r2.to_tag().as_deref(), Some("xyz"));
        assert_eq!(r2.headers_all("Record-Route").len(), 2);
    }

    #[test]
    fn params_and_uris() {
        assert_eq!(param("SIP/2.0/UDP h:5060;branch=z9;received=1.2.3.4;rport=999", "rport").as_deref(), Some("999"));
        assert_eq!(param("<sip:a@b;transport=tcp>;expires=60", "expires").as_deref(), Some("60"));
        assert_eq!(param("<sip:a@b;transport=tcp>", "transport"), None);
        assert_eq!(uri_param("<sip:a@b;transport=tcp>", "transport").as_deref(), Some("tcp"));
        assert_eq!(uri_hostport("sip:1001@pbx.example.com:5080;transport=udp"), "pbx.example.com:5080");
        assert_eq!(uri_user("tel:+15551234"), "+15551234");
        assert_eq!(uri_unescape(&uri_escape("a;to-tag=1")), "a;to-tag=1");
        assert!(frame_len(b"OPTIONS sip:a SIP/2.0\r\nContent-Length: 2\r\n\r\nab").is_some());
        assert!(frame_len(b"OPTIONS sip:a SIP/2.0\r\nContent-Length: 3\r\n\r\nab").is_none());
    }
}
