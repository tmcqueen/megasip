//! Calls: INVITE dialogs, offer/answer, hold, transfer, DTMF, conference.

use super::msg::{self, new_branch, param, parse_name_addr, random_token, uri_user, SipMsg};
use super::sdp::{self, SrtpMode};
use super::transport::{self, Flow, Proto};
use super::{CallId, CallInfo, Engine, Event, TxnOwner, ALLOW};
use crate::media::dsp::Tone;
use crate::media::hub::Ring;
use crate::media::{new_srtp_key, CodecKind, Stream, StreamConfig};
use rsiprtp::sdp::parser::Direction;
use rsiprtp::srtp::CryptoSuite;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Calling,
    Ringing,
    Early,
    Incoming,
    Connected,
    Ended,
}

impl State {
    fn as_str(self) -> &'static str {
        match self {
            State::Calling => "calling",
            State::Ringing => "ringing",
            State::Early => "early",
            State::Incoming => "incoming",
            State::Connected => "connected",
            State::Ended => "ended",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pending {
    None,
    /// Our re-INVITE is in flight.
    Reinvite,
}

pub struct Call {
    pub id: CallId,
    pub acc: String,
    pub outgoing: bool,
    pub state: State,
    pub sip_call_id: String,
    pub local_tag: String,
    pub remote_tag: Option<String>,
    /// Header values for our side and theirs (without tags) as they appear in From/To.
    pub local_hdr: String,
    pub remote_hdr: String,
    pub remote_uri: String,
    pub remote_target: String,
    pub route_set: Vec<String>,
    pub local_cseq: u32,
    pub invite_cseq: u32,
    pub invite_branch: String,
    pub invite: Option<SipMsg>,
    pub peer: (Flow, SocketAddr),
    pub number: String,
    pub name: String,
    pub sock: Option<std::net::UdpSocket>,
    pub stream: Option<Arc<Stream>>,
    pub local_sdp: sdp::Local,
    pub remote_sdp: Option<sdp::Remote>,
    pub codec: Option<sdp::Fmt>,
    pub te_pt: Option<u8>,
    pub srtp_rx: Option<(CryptoSuite, Vec<u8>)>,
    pub hold: bool,
    pub remote_hold: bool,
    pub pending: Pending,
    pub reinvite_retry: Option<Instant>,
    pub started: SystemTime,
    pub connected_at: Option<SystemTime>,
    pub connected_instant: Option<Instant>,
    pub answer_at: Option<Instant>,
    pub forward_at: Option<Instant>,
    pub cancel_wanted: bool,
    pub provisional_seen: bool,
    pub ack: Option<Vec<u8>>,
    pub reason: String,
    pub user_rejected: bool,
    pub transfer: String,
    /// Incoming REFER: (call that asked, its NOTIFY cseq is tracked on that call).
    pub refer_from: Option<CallId>,
    /// Attended transfer partner: hang up together on success.
    pub transfer_partner: Option<CallId>,
    pub redirects: u32,
    pub extra_invite_headers: Vec<(String, String)>,
    /// Delayed offer: we sent the offer in our 200 and wait for the answer in ACK.
    pub offer_in_200: bool,
    pub replaces: Option<CallId>,
}

fn unix(t: SystemTime) -> i64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// ACK for a non-2xx final response (same branch as the INVITE).
pub fn ack_for_failure(invite: &SipMsg, resp: &SipMsg) -> SipMsg {
    let mut ack = SipMsg::request("ACK", invite.uri());
    if let Some(v) = invite.header("Via") {
        ack.add("Via", v);
    }
    ack.add("Max-Forwards", "70");
    if let Some(f) = invite.header("From") {
        ack.add("From", f);
    }
    ack.add(
        "To",
        resp.header("To").or(invite.header("To")).unwrap_or(""),
    );
    ack.add("Call-ID", invite.call_id());
    ack.add("CSeq", format!("{} ACK", invite.cseq().0));
    for r in invite.headers_all("Route") {
        ack.add("Route", r);
    }
    ack
}

/// Turn what the user typed into a request URI.
pub fn target_uri(number: &str, prefix: &str, domain: &str, proto: Proto) -> String {
    let n = number.trim();
    if n.starts_with("sip:") || n.starts_with("sips:") {
        return n.to_string();
    }
    if n.starts_with("tel:") {
        let num = n.trim_start_matches("tel:");
        return format!("sip:{}{}@{}", prefix, clean_number(num), domain);
    }
    if n.contains('@') {
        return format!("sip:{n}");
    }
    // A bare IP address (optionally :port) dials the host directly, like MicroSIP's local account.
    let host = transport::split_host_port(n).0;
    if host.parse::<std::net::IpAddr>().is_ok() {
        return format!("sip:{n}");
    }
    let looks_like_number = n
        .chars()
        .all(|c| c.is_ascii_digit() || " +-().*#/".contains(c));
    let user = if looks_like_number {
        format!("{}{}", prefix, clean_number(n))
    } else {
        n.to_string()
    };
    let _ = proto;
    format!("sip:{user}@{domain}")
}

fn clean_number(n: &str) -> String {
    n.chars()
        .filter(|c| c.is_ascii_digit() || matches!(c, '+' | '*' | '#'))
        .collect()
}

impl Engine {
    fn codecs(&self) -> Vec<CodecKind> {
        self.settings
            .codecs
            .iter()
            .filter_map(|c| CodecKind::from_name(c))
            .collect()
    }

    fn srtp_mode(&self, acc: &str) -> SrtpMode {
        SrtpMode::parse(
            self.accs
                .get(acc)
                .map(|a| a.cfg.srtp.as_str())
                .unwrap_or("off"),
        )
    }

    pub(crate) fn info(&self, c: &Call) -> CallInfo {
        CallInfo {
            id: c.id,
            account_id: c.acc.clone(),
            direction: if c.outgoing {
                "out".into()
            } else {
                "in".into()
            },
            number: c.number.clone(),
            name: c.name.clone(),
            uri: c.remote_uri.clone(),
            state: c.state.as_str().into(),
            held: c.hold,
            remote_held: c.remote_hold,
            codec: c
                .codec
                .map(|f| f.codec.label().to_string())
                .unwrap_or_default(),
            secure: c.srtp_rx.is_some(),
            started: unix(c.started),
            connected_at: c.connected_at.map(unix),
            reason: c.reason.clone(),
            transfer: c.transfer.clone(),
            sip_call_id: c.sip_call_id.clone(),
        }
    }

    fn emit_call(&self, id: CallId) {
        if let Some(c) = self.calls.get(&id) {
            self.emit(Event::Call(self.info(c)));
        }
    }

    /// Tones and ringing follow the set of calls.
    pub(crate) fn update_audio(&mut self) {
        let any_incoming = self.calls.values().any(|c| c.state == State::Incoming);
        let any_active = self
            .calls
            .values()
            .any(|c| c.state == State::Connected || c.state == State::Early);
        self.hub.set_ring(if !any_incoming {
            Ring::Off
        } else if any_active {
            Ring::Waiting
        } else {
            Ring::Ringtone
        });
        let ringback = self
            .calls
            .values()
            .any(|c| c.outgoing && c.state == State::Ringing);
        self.hub
            .set_tone(if ringback { Some(Tone::Ringback) } else { None });
    }

    fn alloc_call(
        &mut self,
        acc: &str,
        outgoing: bool,
        peer: (Flow, SocketAddr),
    ) -> Option<CallId> {
        let range = (self.settings.rtp_port_min, self.settings.rtp_port_max);
        let id = self.next_call;
        self.next_call += 1;
        let sock = match Stream::bind(id, range) {
            Ok(s) => s,
            Err(e) => {
                self.emit(Event::Error(format!("Cannot open an RTP port: {e}")));
                return None;
            }
        };
        let mut ip = self.media_ip(acc);
        let mut port = sock.local_addr().map(|a| a.port()).unwrap_or(0);
        if let Some(stun) = self.stun_for_rtp() {
            if let Some(mapped) = super::stun::binding_blocking(&sock, stun) {
                ip = mapped.ip();
                port = mapped.port();
            }
        }
        let mode = self.srtp_mode(acc);
        let crypto = if mode != SrtpMode::Off {
            let suite = CryptoSuite::AesCm128HmacSha1_80;
            Some((1, suite, new_srtp_key(suite)))
        } else {
            None
        };
        let call = Call {
            id,
            acc: acc.to_string(),
            outgoing,
            state: if outgoing {
                State::Calling
            } else {
                State::Incoming
            },
            sip_call_id: String::new(),
            local_tag: random_token(12),
            remote_tag: None,
            local_hdr: String::new(),
            remote_hdr: String::new(),
            remote_uri: String::new(),
            remote_target: String::new(),
            route_set: Vec::new(),
            local_cseq: rand::random::<u16>() as u32 + 1,
            invite_cseq: 0,
            invite_branch: String::new(),
            invite: None,
            peer,
            number: String::new(),
            name: String::new(),
            sock: Some(sock),
            stream: None,
            local_sdp: sdp::Local {
                ip,
                port,
                session_id: rand::random::<u32>() as u64,
                version: 1,
                direction: Direction::SendRecv,
                savp: mode == SrtpMode::Mandatory,
                crypto,
            },
            remote_sdp: None,
            codec: None,
            te_pt: None,
            srtp_rx: None,
            hold: false,
            remote_hold: false,
            pending: Pending::None,
            reinvite_retry: None,
            started: SystemTime::now(),
            connected_at: None,
            connected_instant: None,
            answer_at: None,
            forward_at: None,
            cancel_wanted: false,
            provisional_seen: false,
            ack: None,
            reason: String::new(),
            user_rejected: false,
            transfer: String::new(),
            refer_from: None,
            transfer_partner: None,
            redirects: 0,
            extra_invite_headers: Vec::new(),
            offer_in_200: false,
            replaces: None,
        };
        self.calls.insert(id, call);
        Some(id)
    }

    fn contact_for(&self, acc: &str, peer: SocketAddr) -> String {
        let a = self.acc(acc);
        if a.contact_addr.is_some() {
            return a.contact();
        }
        let ip = transport::local_ip_for(peer).unwrap_or(std::net::IpAddr::from([127, 0, 0, 1]));
        let user = if a.cfg.username.is_empty() {
            "megasip".to_string()
        } else {
            a.cfg.username.clone()
        };
        format!("<sip:{}@{}:{}>", user, ip, self.tr.udp_local.port())
    }

    fn via_for(&self, acc: &str, peer: (Flow, SocketAddr), branch: &str) -> String {
        let a = self.acc(acc);
        let (proto, host) = match peer.0 {
            Flow::Udp => {
                let host = match a.contact_addr {
                    Some(c) if a.proto == Proto::Udp => c.to_string(),
                    _ => format!(
                        "{}:{}",
                        transport::local_ip_for(peer.1)
                            .unwrap_or(std::net::IpAddr::from([127, 0, 0, 1])),
                        self.tr.udp_local.port()
                    ),
                };
                (Proto::Udp, host)
            }
            Flow::Stream(id) => {
                let (local, p) = self.tr.stream_local(id).unwrap_or((peer.1, Proto::Tcp));
                (p, a.contact_addr.unwrap_or(local).to_string())
            }
        };
        format!(
            "SIP/2.0/{} {};branch={}{}",
            proto.via_name(),
            host,
            branch,
            if self.settings.rport { ";rport" } else { "" }
        )
    }

    // ---------------------------------------------------------------- outgoing

    pub(crate) fn dial(&mut self, acc_id: &str, number: &str) {
        // Nothing that could break out of a header line (pasted text, links).
        let cleaned: String = number
            .chars()
            .filter(|c| !c.is_control() && *c != '<' && *c != '>' && *c != '"')
            .collect();
        let number = cleaned.trim();
        if number.is_empty() {
            return;
        }
        if self.settings.single_call_mode {
            let others: Vec<CallId> = self.calls.keys().copied().collect();
            for o in others {
                self.hangup(o);
            }
        }
        let acc_id = if self.accs.contains_key(acc_id) {
            acc_id.to_string()
        } else {
            self.settings.default_account.clone()
        };
        let mut acc_id = if self.accs.get(&acc_id).is_some_and(|a| a.ready()) {
            acc_id
        } else {
            "local".to_string()
        };
        let Some(acc) = self.accs.get(&acc_id) else {
            return;
        };
        let domain = acc.cfg.sip_domain();
        let uri = target_uri(number, &acc.cfg.dialing_prefix, &domain, acc.proto);
        // Where it goes (RFC 3261 §8.1.2, like MicroSIP/pjsip): through the account when it
        // has an outbound proxy or the address is on its own domain/server; otherwise
        // straight to the host in the address.
        let hp = msg::uri_hostport(&uri);
        let same_place = |other: &str| {
            let o = other
                .trim()
                .trim_start_matches("sip:")
                .split(';')
                .next()
                .unwrap_or("");
            let (oh, op) = transport::split_host_port(o);
            let (th, tp) = transport::split_host_port(&hp);
            !oh.is_empty()
                && th.eq_ignore_ascii_case(oh)
                && (tp.is_none()
                    || tp == op
                    || (op.is_none() && tp == Some(acc.proto.default_port())))
        };
        let ours = !acc.local_only
            && (!acc.cfg.proxy.trim().is_empty()
                || same_place(&domain)
                || same_place(&acc.cfg.server));
        let peer = if let (true, Some(dest)) = (ours, acc.dest) {
            (acc.flow, dest)
        } else {
            let (h, p) = transport::split_host_port(&hp);
            use std::net::ToSocketAddrs;
            let resolved = (h, p.unwrap_or(5060))
                .to_socket_addrs()
                .ok()
                .and_then(|mut it| it.find(|a| a.is_ipv4()));
            match resolved {
                Some(a) => {
                    // A direct UDP call can't use a TCP/TLS account's Contact.
                    if acc.proto != Proto::Udp {
                        acc_id = "local".to_string();
                    }
                    (Flow::Udp, a)
                }
                None => {
                    self.emit(Event::Error(format!("Can't reach {hp}")));
                    return;
                }
            }
        };
        let Some(id) = self.alloc_call(&acc_id, true, peer) else {
            return;
        };
        let acc = &self.accs[&acc_id];
        let local_aor = if acc.local_only {
            let ip =
                transport::local_ip_for(peer.1).unwrap_or(std::net::IpAddr::from([127, 0, 0, 1]));
            format!("sip:megasip@{ip}")
        } else {
            acc.aor()
        };
        let local_hdr = if acc.cfg.hide_cid {
            "\"Anonymous\" <sip:anonymous@anonymous.invalid>".to_string()
        } else if acc.cfg.display_name.trim().is_empty() {
            format!("<{local_aor}>")
        } else {
            format!(
                "{} <{}>",
                msg::quote_display(acc.cfg.display_name.trim()),
                local_aor
            )
        };
        let route_set = if acc.cfg.proxy.trim().is_empty() || acc.local_only {
            Vec::new()
        } else {
            vec![super::route_for_proxy(&acc.cfg.proxy, acc.proto)]
        };
        let hide = acc.cfg.hide_cid;
        let c = self.calls.get_mut(&id).unwrap();
        c.sip_call_id = format!("{}@megasip", random_token(24));
        c.local_hdr = local_hdr;
        c.remote_hdr = format!("<{uri}>");
        c.remote_uri = uri.clone();
        c.remote_target = uri.clone();
        c.route_set = route_set;
        c.number = uri_user(&uri);
        if hide {
            c.extra_invite_headers.push(("Privacy".into(), "id".into()));
            c.extra_invite_headers
                .push(("P-Preferred-Identity".into(), format!("<{local_aor}>")));
        }
        self.settings.last_number = number.to_string();
        self.send_invite(id);
        self.emit_call(id);
    }

    fn send_invite(&mut self, id: CallId) {
        let codecs = self.codecs();
        let ua = self.user_agent.clone();
        let c = self.calls.get(&id).unwrap();
        let branch = new_branch();
        let via = self.via_for(&c.acc, c.peer, &branch);
        let contact = self.contact_for(&c.acc, c.peer.1);
        let c = self.calls.get_mut(&id).unwrap();
        c.local_cseq += 1;
        c.invite_cseq = c.local_cseq;
        c.invite_branch = branch.clone();
        let mut req = SipMsg::request("INVITE", &c.remote_target.clone());
        req.add("Via", via);
        req.add("Max-Forwards", "70");
        req.add("From", format!("{};tag={}", c.local_hdr, c.local_tag));
        req.add("To", c.remote_hdr.clone());
        req.add("Call-ID", c.sip_call_id.clone());
        req.add("CSeq", format!("{} INVITE", c.invite_cseq));
        for r in &c.route_set {
            req.add("Route", r.clone());
        }
        req.add("Contact", contact);
        req.add("Allow", ALLOW);
        req.add("Supported", "replaces");
        req.add("User-Agent", ua);
        for (n, v) in c.extra_invite_headers.clone() {
            req.add(&n, v);
        }
        let body = sdp::offer(&c.local_sdp, &codecs);
        req.set_body("application/sdp", body.into_bytes());
        c.invite = Some(req.clone());
        let peer = c.peer;
        self.send_request(req, branch, peer.0, peer.1, TxnOwner::Call(id));
    }

    /// Responses to anything we sent within a call.
    pub(crate) fn call_response(&mut self, id: CallId, req: &SipMsg, resp: &SipMsg) {
        let method = req.method().to_string();
        let code = resp.code();
        if !self.calls.contains_key(&id) {
            return;
        }
        match method.as_str() {
            "INVITE" => {
                let c = &self.calls[&id];
                let is_initial = c.outgoing && c.connected_at.is_none();
                if is_initial {
                    self.initial_invite_response(id, req, resp);
                } else {
                    self.reinvite_response(id, req, resp);
                }
            }
            "BYE" => {}
            "REFER" => {
                let c = self.calls.get_mut(&id).unwrap();
                if code >= 300 {
                    c.transfer = format!("Transfer failed: {} {}", code, resp.reason());
                } else if code >= 200 {
                    c.transfer = "Transferring…".into();
                }
                self.emit_call(id);
            }
            _ => {}
        }
    }

    fn initial_invite_response(&mut self, id: CallId, req: &SipMsg, resp: &SipMsg) {
        let code = resp.code();
        // Responses to an older INVITE (before auth/redirect) are stale.
        if req.cseq().0 != self.calls[&id].invite_cseq {
            if (200..300).contains(&code) {
                self.on_stray_invite_2xx(resp);
            }
            return;
        }
        if code < 200 {
            if code == 100 {
                let c = self.calls.get_mut(&id).unwrap();
                c.provisional_seen = true;
                if c.cancel_wanted {
                    self.send_cancel(id);
                }
                return;
            }
            let has_sdp = resp.has_sdp();
            {
                let c = self.calls.get_mut(&id).unwrap();
                c.provisional_seen = true;
                if let Some(tag) = resp.to_tag() {
                    c.remote_tag = Some(tag);
                }
                c.state = if has_sdp {
                    State::Early
                } else {
                    State::Ringing
                };
            }
            if has_sdp {
                self.apply_answer(id, &resp.body_str());
            }
            if self.calls[&id].cancel_wanted {
                self.send_cancel(id);
            }
            self.update_audio();
            self.emit_call(id);
            return;
        }
        if (200..300).contains(&code) {
            let cancel = self.calls[&id].cancel_wanted || self.calls[&id].state == State::Ended;
            {
                let c = self.calls.get_mut(&id).unwrap();
                c.remote_tag = resp.to_tag();
                if let Some(ct) = resp.header("Contact") {
                    c.remote_target = parse_name_addr(ct).uri;
                }
                let mut rr: Vec<String> = resp
                    .headers_all("Record-Route")
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                if !rr.is_empty() {
                    rr.reverse();
                    c.route_set = rr;
                }
                if let Some(t) = resp.header("To") {
                    let na = parse_name_addr(t);
                    if !na.display.is_empty() && c.name.is_empty() {
                        c.name = na.display;
                    }
                }
            }
            self.send_ack(id);
            if resp.has_sdp() {
                self.apply_answer(id, &resp.body_str());
            }
            if cancel {
                self.send_bye(id);
                self.end_call(id, "Cancelled");
                return;
            }
            if self.calls[&id].stream.is_none() {
                // No usable answer: hang up.
                self.send_bye(id);
                self.end_call(id, "No compatible media");
                return;
            }
            let c = self.calls.get_mut(&id).unwrap();
            c.state = State::Connected;
            c.connected_at = Some(SystemTime::now());
            c.connected_instant = Some(Instant::now());
            if let Some(s) = &c.stream {
                self.hub.add_stream(s.clone());
            }
            self.after_connect(id);
            self.update_audio();
            self.emit_call(id);
            return;
        }
        // Failure.
        let invite = self.calls[&id]
            .invite
            .clone()
            .unwrap_or_else(|| req.clone());
        if code != 408 || resp.header("Via").is_some() {
            let ack = ack_for_failure(&invite, resp);
            let peer = self.calls[&id].peer;
            self.tr.send(peer.0, peer.1, &ack.to_bytes());
        }
        if (300..400).contains(&code) && self.calls[&id].redirects < 3 {
            if let Some(ct) = resp.header("Contact") {
                let target = parse_name_addr(ct).uri;
                let c = self.calls.get_mut(&id).unwrap();
                c.redirects += 1;
                c.remote_target = target.clone();
                c.remote_uri = target.clone();
                c.remote_hdr = format!("<{target}>");
                c.number = uri_user(&target);
                c.remote_tag = None;
                self.send_invite(id);
                self.emit_call(id);
                return;
            }
        }
        let reason = match code {
            486 | 600 => "Busy".to_string(),
            487 => "Cancelled".to_string(),
            408 => "No answer".to_string(),
            480 => "Unavailable".to_string(),
            404 => "Not found".to_string(),
            603 => "Declined".to_string(),
            _ => format!("{} {}", code, resp.reason()),
        };
        if code == 486 || code == 600 {
            // A few seconds of busy tone, like a real phone.
            self.hub.set_tone(Some(Tone::Busy));
            let hub = self.hub.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(3)).await;
                hub.set_tone(None);
            });
        }
        self.end_call(id, &reason);
    }

    fn send_ack(&mut self, id: CallId) {
        let c = &self.calls[&id];
        let branch = new_branch();
        let ack = self.in_dialog_base(c, "ACK", c.invite_cseq, &branch);
        let bytes = ack.to_bytes();
        let peer = c.peer;
        self.tr.send(peer.0, peer.1, &bytes);
        self.calls.get_mut(&id).unwrap().ack = Some(bytes);
    }

    /// Our re-INVITE (hold/resume) got an answer.
    fn reinvite_response(&mut self, id: CallId, req: &SipMsg, resp: &SipMsg) {
        let code = resp.code();
        if code < 200 {
            return;
        }
        let is_our_reinvite = self.calls[&id].pending == Pending::Reinvite;
        if (200..300).contains(&code) {
            // ACK the 2xx.
            let c = &self.calls[&id];
            let branch = new_branch();
            let ack = self.in_dialog_base(c, "ACK", req.cseq().0, &branch);
            let peer = c.peer;
            self.tr.send(peer.0, peer.1, &ack.to_bytes());
            if let Some(ct) = resp.header("Contact") {
                self.calls.get_mut(&id).unwrap().remote_target = parse_name_addr(ct).uri;
            }
            if resp.has_sdp() {
                self.apply_answer(id, &resp.body_str());
            }
        } else {
            let invite = req.clone();
            let ack = ack_for_failure(&invite, resp);
            let peer = self.calls[&id].peer;
            self.tr.send(peer.0, peer.1, &ack.to_bytes());
            if code == 491 {
                let ms = if self.calls[&id].outgoing {
                    2100 + rand::random::<u64>() % 2000
                } else {
                    rand::random::<u64>() % 2000
                };
                self.calls.get_mut(&id).unwrap().reinvite_retry =
                    Some(Instant::now() + Duration::from_millis(ms));
            } else if code == 481 || code == 408 {
                self.end_call(id, "Call lost");
                return;
            }
        }
        if is_our_reinvite {
            self.calls.get_mut(&id).unwrap().pending = Pending::None;
        }
        self.emit_call(id);
    }

    /// Apply the remote SDP answer (to our offer).
    fn apply_answer(&mut self, id: CallId, body: &str) {
        let codecs = self.codecs();
        let mode = self.srtp_mode(&self.calls[&id].acc);
        let Some(remote) = sdp::parse(body) else {
            return;
        };
        let c = self.calls.get_mut(&id).unwrap();
        let Some(fmt) = sdp::choose(&remote, &codecs, false) else {
            tracing::warn!("answer has no codec we offered");
            return;
        };
        let te = sdp::choose_te(&remote, fmt.codec);
        let ours = c.local_sdp.crypto.clone();
        let srtp_rx = match (
            &ours,
            remote
                .cryptos
                .iter()
                .find(|cr| ours.as_ref().is_some_and(|o| o.0 == cr.tag))
                .or(remote.cryptos.first()),
        ) {
            (Some(_), Some(cr)) => {
                let mut k = cr.master_key.clone();
                k.extend_from_slice(&cr.master_salt);
                Some((cr.crypto_suite, k))
            }
            _ => None,
        };
        if mode == SrtpMode::Mandatory && srtp_rx.is_none() {
            c.reason = "Remote does not support SRTP".into();
            return;
        }
        let srtp_tx = if srtp_rx.is_some() {
            ours.map(|(_, s, k)| (s, k))
        } else {
            None
        };
        c.remote_hold = matches!(remote.direction, Direction::SendOnly | Direction::Inactive);
        c.codec = Some(fmt);
        c.te_pt = te;
        c.srtp_rx = srtp_rx.clone();
        let cfg = StreamConfig {
            remote: remote.addr,
            codec: fmt.codec,
            pt: fmt.pt,
            te_pt: te,
            srtp_tx,
            srtp_rx,
            send: !c.hold && !c.remote_hold && !matches!(remote.direction, Direction::Inactive),
            play: !c.hold,
        };
        c.remote_sdp = Some(remote);
        self.install_stream(id, cfg);
    }

    fn install_stream(&mut self, id: CallId, cfg: StreamConfig) {
        let tx = self.stream_tx.clone();
        let c = self.calls.get_mut(&id).unwrap();
        if let Some(s) = &c.stream {
            s.update(cfg);
            return;
        }
        let Some(sock) = c.sock.take() else { return };
        match Stream::new(id, sock, cfg, tx) {
            Ok(s) => {
                if c.state == State::Early
                    || c.state == State::Connected
                    || c.state == State::Ringing
                {
                    self.hub.add_stream(s.clone());
                }
                c.stream = Some(s);
            }
            Err(e) => tracing::error!("stream: {e}"),
        }
    }

    fn after_connect(&mut self, id: CallId) {
        // Answering a REFER: tell the transferor it worked.
        if let Some(from) = self.calls[&id].refer_from {
            self.send_refer_notify(from, 200, "OK", true);
            self.hangup(from);
        }
        if let Some(old) = self.calls.get_mut(&id).unwrap().replaces.take() {
            self.hangup(old);
        }
        if self.conference {
            self.hub.set_conference(true);
        } else if self.settings.call_waiting {
            // Put the other active calls on hold, like MicroSIP.
            let others: Vec<CallId> = self
                .calls
                .values()
                .filter(|o| o.id != id && o.state == State::Connected && !o.hold)
                .map(|o| o.id)
                .collect();
            for o in others {
                self.set_hold(o, true);
            }
        }
    }

    fn send_cancel(&mut self, id: CallId) {
        let c = &self.calls[&id];
        let Some(inv) = &c.invite else { return };
        let mut cancel = SipMsg::request("CANCEL", inv.uri());
        if let Some(v) = inv.header("Via") {
            cancel.add("Via", v);
        }
        cancel.add("Max-Forwards", "70");
        if let Some(f) = inv.header("From") {
            cancel.add("From", f);
        }
        if let Some(t) = inv.header("To") {
            cancel.add("To", t);
        }
        cancel.add("Call-ID", inv.call_id());
        cancel.add("CSeq", format!("{} CANCEL", inv.cseq().0));
        for r in inv.headers_all("Route") {
            cancel.add("Route", r);
        }
        let branch = inv.branch().unwrap_or_default();
        let peer = c.peer;
        // CANCEL shares the INVITE's branch; key its transaction separately.
        let bytes = cancel.to_bytes();
        self.tr.send(peer.0, peer.1, &bytes);
        let _ = branch;
        self.calls.get_mut(&id).unwrap().cancel_wanted = false;
    }

    // ---------------------------------------------------------------- in-dialog helpers

    pub(crate) fn in_dialog_base(&self, c: &Call, method: &str, cseq: u32, branch: &str) -> SipMsg {
        let mut req = SipMsg::request(method, &c.remote_target);
        req.add("Via", self.via_for(&c.acc, c.peer, branch));
        req.add("Max-Forwards", "70");
        req.add("From", format!("{};tag={}", c.local_hdr, c.local_tag));
        match &c.remote_tag {
            Some(t) => req.add("To", format!("{};tag={}", c.remote_hdr, t)),
            None => req.add("To", c.remote_hdr.clone()),
        };
        req.add("Call-ID", c.sip_call_id.clone());
        req.add("CSeq", format!("{cseq} {method}"));
        for r in &c.route_set {
            req.add("Route", r.clone());
        }
        req
    }

    fn send_in_dialog(
        &mut self,
        id: CallId,
        method: &str,
        extra: Vec<(String, String)>,
        body: Option<(&str, Vec<u8>)>,
    ) {
        let ua = self.user_agent.clone();
        let Some(c) = self.calls.get_mut(&id) else {
            return;
        };
        c.local_cseq += 1;
        let cseq = c.local_cseq;
        let c = &self.calls[&id];
        let branch = new_branch();
        let mut req = self.in_dialog_base(c, method, cseq, &branch);
        if method == "INVITE" || method == "REFER" || method == "UPDATE" {
            req.add("Contact", self.contact_for(&c.acc, c.peer.1));
        }
        if method == "INVITE" {
            req.add("Allow", ALLOW);
            req.add("Supported", "replaces");
        }
        req.add("User-Agent", ua);
        for (n, v) in extra {
            req.add(&n, v);
        }
        if let Some((ct, b)) = body {
            req.set_body(ct, b);
        }
        let peer = c.peer;
        self.send_request(req, branch, peer.0, peer.1, TxnOwner::Call(id));
    }

    fn send_bye(&mut self, id: CallId) {
        self.send_in_dialog(id, "BYE", Vec::new(), None);
    }

    fn send_reinvite(&mut self, id: CallId) {
        let all_codecs = self.codecs();
        let c = self.calls.get_mut(&id).unwrap();
        if c.pending != Pending::None {
            c.reinvite_retry = Some(Instant::now() + Duration::from_millis(500));
            return;
        }
        c.pending = Pending::Reinvite;
        c.local_sdp.version += 1;
        c.local_sdp.direction = match (c.hold, c.remote_hold) {
            (true, true) => Direction::Inactive,
            (true, false) => Direction::SendOnly,
            (false, true) => Direction::RecvOnly,
            (false, false) => Direction::SendRecv,
        };
        // In a re-offer, offer just the codec in use (keeps the PBX from switching).
        let body = match c.codec {
            Some(f) => {
                let te: Vec<(u8, u32)> = c
                    .te_pt
                    .map(|p| vec![(p, f.codec.rtp_clock())])
                    .unwrap_or_default();
                sdp::build(&c.local_sdp, &[f], &te)
            }
            None => sdp::offer(&c.local_sdp, &all_codecs),
        };
        self.send_in_dialog(
            id,
            "INVITE",
            Vec::new(),
            Some(("application/sdp", body.into_bytes())),
        );
    }

    // ---------------------------------------------------------------- user actions

    pub(crate) fn hangup(&mut self, id: CallId) {
        let Some(c) = self.calls.get(&id) else { return };
        match c.state {
            State::Incoming => self.reject(id, 486, "Busy Here"),
            State::Calling | State::Ringing | State::Early if c.outgoing => {
                if c.provisional_seen {
                    self.send_cancel(id);
                    let c = self.calls.get_mut(&id).unwrap();
                    c.reason = "Cancelled".into();
                    // The 487 ends it; don't wait forever.
                    c.forward_at = Some(Instant::now() + Duration::from_secs(4));
                    c.state = State::Ended;
                    self.hub.remove_stream(id);
                    self.update_audio();
                    self.emit_call(id);
                } else {
                    self.calls.get_mut(&id).unwrap().cancel_wanted = true;
                    let c = self.calls.get_mut(&id).unwrap();
                    c.forward_at = Some(Instant::now() + Duration::from_secs(4));
                }
            }
            State::Ended => {}
            _ => {
                self.send_bye(id);
                self.end_call(id, "Hung up");
            }
        }
    }

    pub(crate) fn reject(&mut self, id: CallId, code: u16, reason: &str) {
        let Some(c) = self.calls.get_mut(&id) else {
            return;
        };
        if c.state != State::Incoming {
            return;
        }
        c.user_rejected = code == 603 || code == 486;
        let Some(inv) = c.invite.clone() else { return };
        let tag = c.local_tag.clone();
        let peer = c.peer;
        let r = inv.make_response(code, reason, Some(&tag));
        self.respond(&inv, r, peer.0, peer.1);
        self.end_call(id, if code == 603 { "Declined" } else { "Rejected" });
    }

    pub(crate) fn answer(&mut self, id: CallId) {
        let codecs = self.codecs();
        let ua = self.user_agent.clone();
        let Some(c) = self.calls.get(&id) else { return };
        if c.state != State::Incoming {
            return;
        }
        let Some(inv) = c.invite.clone() else { return };
        let contact = self.contact_for(&c.acc, c.peer.1);
        let mode = self.srtp_mode(&c.acc);
        // Put other calls on hold (MicroSIP does this when answering a waiting call).
        if !self.conference {
            let others: Vec<CallId> = self
                .calls
                .values()
                .filter(|o| o.id != id && o.state == State::Connected && !o.hold)
                .map(|o| o.id)
                .collect();
            for o in others {
                self.set_hold(o, true);
            }
        }
        let c = self.calls.get_mut(&id).unwrap();
        let body: String;
        if inv.has_sdp() {
            let Some(remote) = sdp::parse(&inv.body_str()) else {
                let r = inv.make_response(488, "Not Acceptable Here", Some(&c.local_tag));
                let peer = c.peer;
                self.respond(&inv, r, peer.0, peer.1);
                self.end_call(id, "Bad SDP");
                return;
            };
            let Some(fmt) = sdp::choose(&remote, &codecs, true) else {
                let r = inv.make_response(488, "Not Acceptable Here", Some(&c.local_tag));
                let peer = c.peer;
                self.respond(&inv, r, peer.0, peer.1);
                self.end_call(id, "No common codec");
                return;
            };
            let te = sdp::choose_te(&remote, fmt.codec);
            let their = remote
                .cryptos
                .iter()
                .find(|cr| CryptoSuite::parse(cr.crypto_suite.as_str()).is_some())
                .cloned();
            let use_srtp = their.is_some() && (mode != SrtpMode::Off || remote.savp);
            if mode == SrtpMode::Mandatory && !use_srtp {
                let r = inv.make_response(488, "Not Acceptable Here", Some(&c.local_tag));
                let peer = c.peer;
                self.respond(&inv, r, peer.0, peer.1);
                self.end_call(id, "Caller does not offer SRTP");
                return;
            }
            let (srtp_tx, srtp_rx) = if let (true, Some(cr)) = (use_srtp, their) {
                let key = new_srtp_key(cr.crypto_suite);
                c.local_sdp.crypto = Some((cr.tag, cr.crypto_suite, key.clone()));
                let mut rk = cr.master_key.clone();
                rk.extend_from_slice(&cr.master_salt);
                (Some((cr.crypto_suite, key)), Some((cr.crypto_suite, rk)))
            } else {
                c.local_sdp.crypto = None;
                (None, None)
            };
            c.local_sdp.savp = remote.savp && use_srtp;
            c.remote_hold = matches!(remote.direction, Direction::SendOnly | Direction::Inactive);
            c.local_sdp.direction = sdp::answer_direction(remote.direction, false);
            let te_list: Vec<(u8, u32)> = te
                .map(|p| vec![(p, fmt.codec.rtp_clock())])
                .unwrap_or_default();
            body = sdp::build(&c.local_sdp, &[fmt], &te_list);
            c.codec = Some(fmt);
            c.te_pt = te;
            c.srtp_rx = srtp_rx.clone();
            let cfg = StreamConfig {
                remote: remote.addr,
                codec: fmt.codec,
                pt: fmt.pt,
                te_pt: te,
                srtp_tx,
                srtp_rx,
                send: !c.remote_hold,
                play: true,
            };
            c.remote_sdp = Some(remote);
            c.state = State::Connected;
            self.install_stream(id, cfg);
        } else {
            // Delayed offer: we offer in the 200, the answer comes in the ACK.
            c.offer_in_200 = true;
            body = sdp::offer(&c.local_sdp, &codecs);
            c.state = State::Connected;
        }
        let c = self.calls.get_mut(&id).unwrap();
        c.connected_at = Some(SystemTime::now());
        c.connected_instant = Some(Instant::now());
        c.answer_at = None;
        c.forward_at = None;
        let tag = c.local_tag.clone();
        let peer = c.peer;
        let mut r = inv.make_response(200, "OK", Some(&tag));
        r.add("Contact", contact);
        r.add("Allow", ALLOW);
        r.add("Supported", "replaces");
        r.add("User-Agent", ua);
        r.set_body("application/sdp", body.into_bytes());
        self.respond(&inv, r, peer.0, peer.1);
        if let Some(s) = self.calls[&id].stream.clone() {
            self.hub.add_stream(s);
        }
        self.after_connect(id);
        self.update_audio();
        self.emit_call(id);
    }

    pub(crate) fn set_hold(&mut self, id: CallId, on: bool) {
        let Some(c) = self.calls.get_mut(&id) else {
            return;
        };
        if c.state != State::Connected || c.hold == on {
            return;
        }
        c.hold = on;
        if let Some(s) = &c.stream {
            let mut cfg = s.config();
            cfg.send = !on && !c.remote_hold;
            cfg.play = !on;
            s.update(cfg);
        }
        if !on && !self.conference && self.settings.call_waiting {
            // Resuming one call holds the others.
            let others: Vec<CallId> = self
                .calls
                .values()
                .filter(|o| o.id != id && o.state == State::Connected && !o.hold)
                .map(|o| o.id)
                .collect();
            for o in others {
                self.set_hold(o, true);
            }
        }
        self.send_reinvite(id);
        self.emit_call(id);
    }

    pub(crate) fn send_dtmf(&mut self, id: CallId, d: char) {
        if self.settings.local_dtmf {
            self.hub.play_local_dtmf(d);
        }
        let method = self.settings.dtmf_method.clone();
        let Some(c) = self.calls.get(&id) else { return };
        if c.state != State::Connected && c.state != State::Early {
            return;
        }
        let stream = c.stream.clone();
        match method.as_str() {
            "info" => {
                let body = format!("Signal={}\r\nDuration=160\r\n", d);
                self.send_in_dialog(
                    id,
                    "INFO",
                    Vec::new(),
                    Some(("application/dtmf-relay", body.into_bytes())),
                );
            }
            "inband" => {
                if let Some(s) = stream {
                    s.send_dtmf_inband(d);
                }
            }
            _ => {
                let sent = stream.as_ref().is_some_and(|s| s.send_dtmf_rfc(d, 160));
                if !sent {
                    if method == "rfc2833" {
                        if let Some(s) = stream {
                            s.send_dtmf_inband(d);
                        }
                    } else {
                        let body = format!("Signal={}\r\nDuration=160\r\n", d);
                        self.send_in_dialog(
                            id,
                            "INFO",
                            Vec::new(),
                            Some(("application/dtmf-relay", body.into_bytes())),
                        );
                    }
                }
            }
        }
    }

    pub(crate) fn blind_transfer(&mut self, id: CallId, to: &str) {
        let Some(c) = self.calls.get(&id) else { return };
        if c.state != State::Connected {
            return;
        }
        let acc = self.acc(&c.acc);
        let target = target_uri(
            to,
            &acc.cfg.dialing_prefix,
            &acc.cfg.sip_domain(),
            acc.proto,
        );
        let by = format!("<{}>", acc.aor());
        self.send_in_dialog(
            id,
            "REFER",
            vec![
                ("Refer-To".into(), format!("<{target}>")),
                ("Referred-By".into(), by),
            ],
            None,
        );
        self.calls.get_mut(&id).unwrap().transfer =
            format!("Transferring to {}", uri_user(&target));
        self.emit_call(id);
    }

    pub(crate) fn attended_transfer(&mut self, a: CallId, b: CallId) {
        let (Some(ca), Some(cb)) = (self.calls.get(&a), self.calls.get(&b)) else {
            return;
        };
        if ca.state != State::Connected || cb.state != State::Connected {
            return;
        }
        let Some(rt) = cb.remote_tag.clone() else {
            return;
        };
        let replaces = format!("{};to-tag={};from-tag={}", cb.sip_call_id, rt, cb.local_tag);
        let target = cb
            .remote_uri
            .split(';')
            .next()
            .unwrap_or(&cb.remote_uri)
            .to_string();
        let refer_to = format!("<{}?Replaces={}>", target, msg::uri_escape(&replaces));
        let by = format!("<{}>", self.acc(&ca.acc).aor());
        self.send_in_dialog(
            a,
            "REFER",
            vec![("Refer-To".into(), refer_to), ("Referred-By".into(), by)],
            None,
        );
        let c = self.calls.get_mut(&a).unwrap();
        c.transfer = "Transferring…".into();
        c.transfer_partner = Some(b);
        self.emit_call(a);
    }

    pub(crate) fn set_conference(&mut self, on: bool) {
        self.conference = on;
        self.hub.set_conference(on);
        if on {
            let held: Vec<CallId> = self
                .calls
                .values()
                .filter(|c| c.state == State::Connected && c.hold)
                .map(|c| c.id)
                .collect();
            for id in held {
                // Resume without holding the others.
                let c = self.calls.get_mut(&id).unwrap();
                c.hold = false;
                if let Some(s) = &c.stream {
                    let mut cfg = s.config();
                    cfg.send = !c.remote_hold;
                    cfg.play = true;
                    s.update(cfg);
                }
                self.send_reinvite(id);
                self.emit_call(id);
            }
        }
    }

    // ---------------------------------------------------------------- incoming

    fn account_for_request(&self, req: &SipMsg, flow: Flow, from: SocketAddr) -> String {
        // The connection it came in on.
        if let Flow::Stream(_) = flow {
            if let Some((id, _)) = self.accs.iter().find(|(_, a)| a.flow == flow) {
                return id.clone();
            }
        }
        let user = uri_user(req.uri());
        let to_user = req
            .header("To")
            .map(|t| uri_user(&parse_name_addr(t).uri))
            .unwrap_or_default();
        let mut by_server = None;
        for (id, a) in &self.accs {
            if a.local_only {
                continue;
            }
            if a.dest.is_some_and(|d| d.ip() == from.ip()) {
                if !a.cfg.username.is_empty()
                    && (a.cfg.username == user || a.cfg.username == to_user)
                {
                    return id.clone();
                }
                by_server.get_or_insert(id.clone());
            }
        }
        if let Some(id) = by_server {
            return id;
        }
        for (id, a) in &self.accs {
            if !a.local_only && !a.cfg.username.is_empty() && a.cfg.username == user {
                return id.clone();
            }
        }
        "local".into()
    }

    pub(crate) fn on_invite(&mut self, req: SipMsg, flow: Flow, from: SocketAddr) {
        // re-INVITE?
        if req.to_tag().is_some() {
            self.in_dialog_request(req, flow, from);
            return;
        }
        // Same Call-ID already being handled (forked/duplicate INVITE)?
        if self
            .calls
            .values()
            .any(|c| c.sip_call_id == req.call_id() && !c.outgoing)
        {
            return;
        }
        let trying = req.make_response(100, "Trying", None);
        self.respond(&req, trying, flow, from);

        let acc_id = self.account_for_request(&req, flow, from);
        let from_na = parse_name_addr(req.header("From").unwrap_or(""));
        let pai = req.header("P-Asserted-Identity").map(parse_name_addr);
        let Some(id) = self.alloc_call(&acc_id, false, (flow, from)) else {
            let r = req.make_response(500, "Server Internal Error", Some(&random_token(8)));
            self.respond(&req, r, flow, from);
            return;
        };
        {
            let c = self.calls.get_mut(&id).unwrap();
            c.sip_call_id = req.call_id().to_string();
            c.remote_tag = req.from_tag();
            let to_hdr = req.header("To").unwrap_or("").to_string();
            c.local_hdr = strip_tag(&to_hdr);
            c.remote_hdr = strip_tag(req.header("From").unwrap_or(""));
            c.remote_uri = from_na.uri.clone();
            c.remote_target = req
                .header("Contact")
                .map(|ct| parse_name_addr(ct).uri)
                .unwrap_or(from_na.uri.clone());
            c.route_set = req
                .headers_all("Record-Route")
                .iter()
                .map(|s| s.to_string())
                .collect();
            c.local_cseq = rand::random::<u16>() as u32 + 1;
            c.number = uri_user(&from_na.uri);
            c.name = from_na.display.clone();
            if c.name.is_empty() {
                if let Some(p) = &pai {
                    c.name = p.display.clone();
                }
            }
            if c.number.is_empty() || c.number.eq_ignore_ascii_case("anonymous") {
                if let Some(p) = &pai {
                    c.number = uri_user(&p.uri);
                }
            }
            c.invite = Some(req.clone());
        }
        // Attended transfer to us (Replaces): take over that call.
        if let Some(rep) = req.header("Replaces") {
            let callid = rep.split(';').next().unwrap_or("").trim().to_string();
            let target = self
                .calls
                .values()
                .find(|c| c.sip_call_id == callid && c.id != id)
                .map(|c| c.id);
            if let Some(old) = target {
                self.calls.get_mut(&id).unwrap().replaces = Some(old);
                self.emit(Event::Incoming(self.info(&self.calls[&id])));
                self.answer(id);
                return;
            }
            let tag = self.calls[&id].local_tag.clone();
            let r = req.make_response(481, "Call/Transaction Does Not Exist", Some(&tag));
            self.respond(&req, r, flow, from);
            self.end_call(id, "Replaces target not found");
            return;
        }
        let tag = self.calls[&id].local_tag.clone();
        let settings = self.settings.clone();
        let s = &settings;
        let busy = self
            .calls
            .values()
            .any(|c| c.id != id && c.state != State::Ended);
        let deny = match s.deny_incoming.as_str() {
            "all" => true,
            "otherdomain" => {
                let acc_domain = self
                    .accs
                    .get(&acc_id)
                    .map(|a| a.cfg.sip_domain())
                    .unwrap_or_default();
                let host = msg::uri_hostport(&from_na.uri);
                let host = transport::split_host_port(&host).0.to_string();
                acc_id == "local"
                    || (!acc_domain.is_empty() && !host.eq_ignore_ascii_case(&acc_domain))
            }
            _ => false,
        };
        let forward_target = |s: &crate::config::Settings, acc: &super::Acc| {
            target_uri(
                &s.forwarding_number,
                &acc.cfg.dialing_prefix,
                &acc.cfg.sip_domain(),
                acc.proto,
            )
        };
        let acc_snapshot = self.new_acc_snapshot(&acc_id);
        let acc = &acc_snapshot;
        if deny {
            let r = req.make_response(403, "Forbidden", Some(&tag));
            self.respond(&req, r, flow, from);
            self.end_call(id, "Denied");
            return;
        }
        if s.forwarding == "always" && !s.forwarding_number.trim().is_empty() {
            let t = forward_target(s, acc);
            self.redirect(id, &t);
            return;
        }
        if s.dnd {
            let r = req.make_response(486, "Busy Here", Some(&tag));
            self.respond(&req, r, flow, from);
            self.end_call(id, "Do not disturb");
            return;
        }
        if busy && (!s.call_waiting || s.single_call_mode) {
            if s.forwarding == "busy" && !s.forwarding_number.trim().is_empty() {
                let t = forward_target(s, acc);
                self.redirect(id, &t);
            } else {
                let r = req.make_response(486, "Busy Here", Some(&tag));
                self.respond(&req, r, flow, from);
                self.end_call(id, "Busy");
            }
            return;
        }
        let mut ringing = req.make_response(180, "Ringing", Some(&tag));
        ringing.add("Contact", self.contact_for(&acc_id, from));
        ringing.add("Allow", ALLOW);
        self.respond(&req, ringing, flow, from);
        // Auto answer: always, or when the server asks (intercom/paging headers).
        let header_asks = req
            .header("Call-Info")
            .is_some_and(|v| v.to_ascii_lowercase().contains("answer-after"))
            || req.header("Alert-Info").is_some_and(|v| {
                let v = v.to_ascii_lowercase();
                v.contains("auto answer")
                    || v.contains("autoanswer")
                    || v.contains("info=alert-autoanswer")
                    || v.contains("intercom")
            });
        let auto = match s.auto_answer.as_str() {
            "all" => !busy,
            "header" => header_asks,
            _ => false,
        };
        let delay = if header_asks {
            req.header("Call-Info")
                .and_then(|v| param(v, "answer-after"))
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(s.auto_answer_delay as u64)
        } else {
            s.auto_answer_delay as u64
        };
        let fwd_noanswer = s.forwarding == "noanswer" && !s.forwarding_number.trim().is_empty();
        let fwd_delay = s.forwarding_delay.max(1) as u64;
        let c = self.calls.get_mut(&id).unwrap();
        if auto {
            c.answer_at = Some(Instant::now() + Duration::from_secs(delay));
        }
        if fwd_noanswer {
            c.forward_at = Some(Instant::now() + Duration::from_secs(fwd_delay));
        }
        let info = self.info(&self.calls[&id]);
        self.emit(Event::Incoming(info));
        self.update_audio();
        self.emit_call(id);
    }

    fn redirect(&mut self, id: CallId, target: &str) {
        let c = &self.calls[&id];
        let Some(inv) = c.invite.clone() else { return };
        let mut r = inv.make_response(302, "Moved Temporarily", Some(&c.local_tag));
        r.add("Contact", format!("<{target}>"));
        let peer = c.peer;
        self.respond(&inv, r, peer.0, peer.1);
        self.calls.get_mut(&id).unwrap().user_rejected = true;
        self.end_call(id, &format!("Forwarded to {}", uri_user(target)));
    }

    pub(crate) fn on_cancel(&mut self, req: SipMsg, flow: Flow, from: SocketAddr) {
        let branch = req.branch().unwrap_or_default();
        let id = self
            .calls
            .values()
            .find(|c| {
                !c.outgoing
                    && c.sip_call_id == req.call_id()
                    && c.invite.as_ref().and_then(|i| i.branch()) == Some(branch.clone())
            })
            .map(|c| c.id);
        let Some(id) = id else {
            let r = req.make_response(481, "Call/Transaction Does Not Exist", None);
            self.respond(&req, r, flow, from);
            return;
        };
        let r = req.make_response(200, "OK", Some(&self.calls[&id].local_tag.clone()));
        self.respond(&req, r, flow, from);
        if self.calls[&id].state == State::Incoming {
            let c = &self.calls[&id];
            let inv = c.invite.clone().unwrap();
            let r = inv.make_response(487, "Request Terminated", Some(&c.local_tag));
            let peer = c.peer;
            self.respond(&inv, r, peer.0, peer.1);
            // "Call completed elsewhere" (RFC 3326 Reason) is not a missed call.
            let elsewhere = req.header("Reason").is_some_and(|r| {
                r.contains("cause=200") || r.to_ascii_lowercase().contains("completed elsewhere")
            });
            if elsewhere {
                self.calls.get_mut(&id).unwrap().user_rejected = true;
            }
            self.end_call(
                id,
                if elsewhere {
                    "Answered elsewhere"
                } else {
                    "Missed"
                },
            );
        }
    }

    fn find_dialog(&self, req: &SipMsg) -> Option<CallId> {
        let cid = req.call_id();
        let to_tag = req.to_tag();
        self.calls
            .values()
            .find(|c| c.sip_call_id == cid && to_tag.as_deref().is_none_or(|t| t == c.local_tag))
            .map(|c| c.id)
    }

    pub(crate) fn in_dialog_request(&mut self, req: SipMsg, flow: Flow, from: SocketAddr) {
        let method = req.method().to_string();
        let Some(id) = self.find_dialog(&req) else {
            let r = req.make_response(481, "Call/Transaction Does Not Exist", None);
            self.respond(&req, r, flow, from);
            return;
        };
        let tag = self.calls[&id].local_tag.clone();
        match method.as_str() {
            "BYE" => {
                let r = req.make_response(200, "OK", Some(&tag));
                self.respond(&req, r, flow, from);
                let reason = if self.calls[&id].state == State::Incoming {
                    "Missed"
                } else {
                    "Remote hung up"
                };
                if self.calls[&id].state == State::Incoming {
                    let c = &self.calls[&id];
                    if let Some(inv) = c.invite.clone() {
                        let r = inv.make_response(487, "Request Terminated", Some(&c.local_tag));
                        let peer = c.peer;
                        self.respond(&inv, r, peer.0, peer.1);
                    }
                }
                self.end_call(id, reason);
            }
            "INVITE" | "UPDATE" => self.on_reoffer(id, req, flow, from),
            "INFO" => {
                let r = req.make_response(200, "OK", Some(&tag));
                self.respond(&req, r, flow, from);
                if req.content_type() == "application/dtmf-relay"
                    || req.content_type() == "application/dtmf"
                {
                    let body = req.body_str();
                    let d = body
                        .lines()
                        .find_map(|l| {
                            l.trim()
                                .strip_prefix("Signal=")
                                .or_else(|| l.trim().strip_prefix("Signal ="))
                        })
                        .and_then(|v| v.trim().chars().next())
                        .or_else(|| body.trim().chars().next());
                    if let Some(d) = d {
                        self.emit(Event::Dtmf(id, d));
                    }
                }
            }
            "NOTIFY" => {
                let r = req.make_response(200, "OK", Some(&tag));
                self.respond(&req, r, flow, from);
                let ev = req.header("Event").unwrap_or("").to_ascii_lowercase();
                if ev.starts_with("refer") {
                    let body = req.body_str();
                    let code: u16 = body
                        .lines()
                        .next()
                        .and_then(|l| l.trim().strip_prefix("SIP/2.0 "))
                        .and_then(|r| r.split_whitespace().next())
                        .and_then(|c| c.parse().ok())
                        .unwrap_or(0);
                    if (200..300).contains(&code) {
                        let partner = self.calls[&id].transfer_partner;
                        self.calls.get_mut(&id).unwrap().transfer = "Transferred".into();
                        self.hangup(id);
                        if let Some(p) = partner {
                            self.hangup(p);
                        }
                    } else if code >= 300 {
                        let c = self.calls.get_mut(&id).unwrap();
                        c.transfer = format!(
                            "Transfer failed: {}",
                            body.lines()
                                .next()
                                .unwrap_or("")
                                .trim_start_matches("SIP/2.0 ")
                        );
                        c.transfer_partner = None;
                        self.emit_call(id);
                    }
                }
            }
            "REFER" => self.on_refer(id, req, flow, from),
            "OPTIONS" => {
                let mut r = req.make_response(200, "OK", Some(&tag));
                r.add("Allow", ALLOW);
                self.respond(&req, r, flow, from);
            }
            "PRACK" => {
                let r = req.make_response(200, "OK", Some(&tag));
                self.respond(&req, r, flow, from);
            }
            _ => {
                let r = req.make_response(501, "Not Implemented", Some(&tag));
                self.respond(&req, r, flow, from);
            }
        }
    }

    /// The remote re-INVITEs or UPDATEs (hold, resume, media change, session refresh).
    fn on_reoffer(&mut self, id: CallId, req: SipMsg, flow: Flow, from: SocketAddr) {
        let codecs = self.codecs();
        let ua = self.user_agent.clone();
        let tag = self.calls[&id].local_tag.clone();
        if req.method() == "INVITE" && self.calls[&id].pending == Pending::Reinvite {
            let r = req.make_response(491, "Request Pending", Some(&tag));
            self.respond(&req, r, flow, from);
            return;
        }
        let contact = self.contact_for(&self.calls[&id].acc, from);
        if let Some(ct) = req.header("Contact") {
            self.calls.get_mut(&id).unwrap().remote_target = parse_name_addr(ct).uri;
        }
        let c = self.calls.get_mut(&id).unwrap();
        let mut body = None;
        if req.has_sdp() {
            let Some(remote) = sdp::parse(&req.body_str()) else {
                let r = req.make_response(488, "Not Acceptable Here", Some(&tag));
                self.respond(&req, r, flow, from);
                return;
            };
            // Same session version: nothing changed (session refresh).
            let unchanged = c.remote_sdp.as_ref().is_some_and(|old| {
                old.session_version == remote.session_version
                    && old.direction == remote.direction
                    && old.addr == remote.addr
            });
            let fmt = c
                .codec
                .and_then(|cur| remote.fmts.iter().find(|f| f.codec == cur.codec).copied())
                .or_else(|| sdp::choose(&remote, &codecs, true));
            let Some(fmt) = fmt else {
                let r = req.make_response(488, "Not Acceptable Here", Some(&tag));
                self.respond(&req, r, flow, from);
                return;
            };
            let te = sdp::choose_te(&remote, fmt.codec);
            c.remote_hold = matches!(remote.direction, Direction::SendOnly | Direction::Inactive)
                || remote.addr.is_none();
            if !unchanged {
                c.local_sdp.version += 1;
            }
            c.local_sdp.direction = sdp::answer_direction(remote.direction, c.hold);
            // Keep our SRTP key; take theirs (it may have changed).
            let srtp_rx = if c.local_sdp.crypto.is_some() {
                remote.cryptos.first().map(|cr| {
                    let mut k = cr.master_key.clone();
                    k.extend_from_slice(&cr.master_salt);
                    (cr.crypto_suite, k)
                })
            } else {
                None
            };
            if let (Some(cr), Some((t, _, _))) =
                (remote.cryptos.first(), c.local_sdp.crypto.as_mut())
            {
                *t = cr.tag;
            }
            let te_list: Vec<(u8, u32)> = te
                .map(|p| vec![(p, fmt.codec.rtp_clock())])
                .unwrap_or_default();
            body = Some(sdp::build(&c.local_sdp, &[fmt], &te_list));
            c.codec = Some(fmt);
            c.te_pt = te;
            if srtp_rx.is_some() {
                c.srtp_rx = srtp_rx.clone();
            }
            let cfg = StreamConfig {
                remote: remote.addr,
                codec: fmt.codec,
                pt: fmt.pt,
                te_pt: te,
                srtp_tx: c.local_sdp.crypto.as_ref().map(|(_, s, k)| (*s, k.clone())),
                srtp_rx: c.srtp_rx.clone(),
                send: !c.hold && !c.remote_hold,
                play: !c.hold,
            };
            c.remote_sdp = Some(remote);
            self.install_stream(id, cfg);
        } else if req.method() == "INVITE" {
            // Offerless re-INVITE: we offer, answer comes in the ACK.
            let c = self.calls.get_mut(&id).unwrap();
            c.local_sdp.version += 1;
            c.offer_in_200 = true;
            body = Some(sdp::offer(&c.local_sdp, &codecs));
        }
        let mut r = req.make_response(200, "OK", Some(&tag));
        r.add("Contact", contact);
        r.add("Allow", ALLOW);
        r.add("User-Agent", ua);
        if let Some(se) = req.header("Session-Expires") {
            r.add("Session-Expires", se.to_string());
            if req.header("Supported").is_some_and(|s| s.contains("timer")) {
                r.add("Require", "timer");
            }
        }
        if let Some(b) = body {
            r.set_body("application/sdp", b.into_bytes());
        }
        self.respond(&req, r, flow, from);
        self.emit_call(id);
    }

    pub(crate) fn call_ack(&mut self, req: &SipMsg) {
        let Some(id) = self.find_dialog(req) else {
            return;
        };
        let c = self.calls.get_mut(&id).unwrap();
        if c.offer_in_200 && req.has_sdp() {
            c.offer_in_200 = false;
            // The ACK carries the answer to our offer.
            let body = req.body_str();
            self.apply_answer(id, &body);
            if let Some(s) = self.calls[&id].stream.clone() {
                self.hub.add_stream(s);
            }
            self.emit_call(id);
        }
    }

    pub(crate) fn on_stray_invite_2xx(&mut self, resp: &SipMsg) {
        let cid = resp.call_id();
        let id = self
            .calls
            .values()
            .find(|c| c.sip_call_id == cid && c.outgoing)
            .map(|c| c.id);
        if let Some(id) = id {
            let c = &self.calls[&id];
            if let Some(ack) = &c.ack {
                self.tr.send(c.peer.0, c.peer.1, ack);
            }
        }
    }

    // ---------------------------------------------------------------- incoming REFER

    fn on_refer(&mut self, id: CallId, req: SipMsg, flow: Flow, from: SocketAddr) {
        let tag = self.calls[&id].local_tag.clone();
        let Some(rt) = req.header("Refer-To") else {
            let r = req.make_response(400, "Missing Refer-To", Some(&tag));
            self.respond(&req, r, flow, from);
            return;
        };
        let na = parse_name_addr(rt);
        let r = req.make_response(202, "Accepted", Some(&tag));
        self.respond(&req, r, flow, from);
        self.send_refer_notify(id, 100, "Trying", false);
        // Split off ?Replaces=...
        let (target, replaces) = match na.uri.split_once('?') {
            Some((t, hdrs)) => {
                let rep = hdrs
                    .split('&')
                    .find_map(|h| {
                        h.strip_prefix("Replaces=")
                            .or_else(|| h.strip_prefix("replaces="))
                    })
                    .map(msg::uri_unescape);
                (t.to_string(), rep)
            }
            None => (na.uri.clone(), None),
        };
        let acc = self.calls[&id].acc.clone();
        self.set_hold(id, true);
        let before = self.next_call;
        self.dial(&acc, &target);
        if self.next_call > before {
            let new_id = self.next_call - 1;
            if let Some(nc) = self.calls.get_mut(&new_id) {
                nc.refer_from = Some(id);
                if let Some(r) = replaces {
                    nc.extra_invite_headers.push(("Replaces".into(), r));
                    // Re-send with the Replaces header (the first INVITE went out already).
                }
            }
            if self
                .calls
                .get(&new_id)
                .is_some_and(|c| c.extra_invite_headers.iter().any(|(n, _)| n == "Replaces"))
            {
                // Cancel the plain INVITE we just sent and resend with Replaces.
                let branch = self.calls[&new_id].invite_branch.clone();
                self.ctx.remove(&branch);
                self.send_invite(new_id);
            }
        } else {
            self.send_refer_notify(id, 503, "Service Unavailable", true);
        }
    }

    fn send_refer_notify(&mut self, id: CallId, code: u16, reason: &str, terminated: bool) {
        if !self.calls.contains_key(&id) {
            return;
        }
        let state = if terminated {
            "terminated;reason=noresource".to_string()
        } else {
            "active;expires=60".to_string()
        };
        let body = format!("SIP/2.0 {code} {reason}\r\n");
        self.send_in_dialog(
            id,
            "NOTIFY",
            vec![
                ("Event".into(), "refer".into()),
                ("Subscription-State".into(), state),
            ],
            Some(("message/sipfrag;version=2.0", body.into_bytes())),
        );
    }

    // ---------------------------------------------------------------- lifecycle

    pub(crate) fn end_call(&mut self, id: CallId, reason: &str) {
        let Some(mut c) = self.calls.remove(&id) else {
            return;
        };
        if c.reason.is_empty() || reason != "Hung up" {
            c.reason = reason.to_string();
        }
        let missed = !c.outgoing && c.connected_at.is_none() && !c.user_rejected;
        c.state = State::Ended;
        self.hub.remove_stream(id);
        // Transferee leg failed: tell the transferor.
        if let Some(from) = c.refer_from {
            if c.connected_at.is_none() {
                self.send_refer_notify(from, 503, "Service Unavailable", true);
                self.set_hold(from, false);
            }
        }
        let info = self.info(&c);
        drop(c);
        if self.calls.is_empty() {
            self.conference = false;
            self.hub.set_conference(false);
        }
        self.update_audio();
        self.emit(Event::Ended { info, missed });
    }

    pub(crate) fn call_tick(&mut self, now: Instant) {
        let ids: Vec<CallId> = self.calls.keys().copied().collect();
        for id in ids {
            let Some(c) = self.calls.get(&id) else {
                continue;
            };
            if c.answer_at.is_some_and(|t| now >= t) && c.state == State::Incoming {
                self.answer(id);
                continue;
            }
            if c.forward_at.is_some_and(|t| now >= t) {
                match c.state {
                    State::Incoming => {
                        let acc = self.acc(&c.acc);
                        let t = target_uri(
                            &self.settings.forwarding_number,
                            &acc.cfg.dialing_prefix,
                            &acc.cfg.sip_domain(),
                            acc.proto,
                        );
                        self.redirect(id, &t);
                    }
                    // Cancelled call whose 487 never came.
                    _ if c.outgoing && c.connected_at.is_none() => {
                        let reason = if c.reason.is_empty() {
                            "Cancelled".to_string()
                        } else {
                            c.reason.clone()
                        };
                        self.ctx
                            .retain(|_, t| !matches!(t.owner, TxnOwner::Call(x) if x == id));
                        self.end_call(id, &reason);
                    }
                    _ => {
                        self.calls.get_mut(&id).unwrap().forward_at = None;
                    }
                }
                continue;
            }
            if c.reinvite_retry.is_some_and(|t| now >= t) {
                let c = self.calls.get_mut(&id).unwrap();
                c.reinvite_retry = None;
                c.pending = Pending::None;
                self.send_reinvite(id);
                continue;
            }
            let ah = self.settings.auto_hangup_secs;
            if ah > 0
                && c.connected_instant
                    .is_some_and(|t| now - t > Duration::from_secs(ah as u64))
            {
                self.hangup(id);
            }
        }
    }
}

fn strip_tag(h: &str) -> String {
    match h.rfind('>') {
        Some(i) => {
            let tail: Vec<&str> = h[i + 1..]
                .split(';')
                .filter(|p| !p.trim().to_ascii_lowercase().starts_with("tag="))
                .collect();
            format!("{}{}", &h[..=i], tail.join(";"))
        }
        None => h.split(";tag=").next().unwrap_or(h).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets() {
        assert_eq!(
            target_uri("1001", "", "pbx.example.com", Proto::Udp),
            "sip:1001@pbx.example.com"
        );
        assert_eq!(
            target_uri("+1 (555) 123-4567", "9", "pbx", Proto::Udp),
            "sip:9+15551234567@pbx"
        );
        assert_eq!(
            target_uri("bob@example.org", "", "pbx", Proto::Udp),
            "sip:bob@example.org"
        );
        assert_eq!(
            target_uri("192.168.1.20", "", "pbx", Proto::Udp),
            "sip:192.168.1.20"
        );
        assert_eq!(
            target_uri("sip:echo@x", "", "pbx", Proto::Udp),
            "sip:echo@x"
        );
        assert_eq!(target_uri("*43", "", "pbx", Proto::Udp), "sip:*43@pbx");
    }

    #[test]
    fn strips_tags() {
        assert_eq!(strip_tag("\"A\" <sip:a@b>;tag=123"), "\"A\" <sip:a@b>");
        assert_eq!(strip_tag("<sip:a@b>;foo=1;tag=1"), "<sip:a@b>;foo=1");
    }
}
