//! SDP offer/answer for a single audio stream.

use crate::media::CodecKind;
use rsiprtp::sdp::parser::{Direction, SessionDescription};
use rsiprtp::srtp::{CryptoSuite, SdesAttribute};
use std::fmt::Write as _;
use std::net::{IpAddr, SocketAddr};

pub const OPUS_PT: u8 = 111;
pub const TE8K_PT: u8 = 101;
pub const TE48K_PT: u8 = 102;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SrtpMode {
    Off,
    Optional,
    Mandatory,
}

impl SrtpMode {
    pub fn parse(s: &str) -> SrtpMode {
        match s {
            "mandatory" => SrtpMode::Mandatory,
            "optional" => SrtpMode::Optional,
            _ => SrtpMode::Off,
        }
    }
}

pub fn dir_str(d: Direction) -> &'static str {
    match d {
        Direction::SendRecv => "sendrecv",
        Direction::SendOnly => "sendonly",
        Direction::RecvOnly => "recvonly",
        Direction::Inactive => "inactive",
    }
}

/// Our side of the media description.
#[derive(Clone, Debug)]
pub struct Local {
    pub ip: IpAddr,
    pub port: u16,
    pub session_id: u64,
    pub version: u64,
    pub direction: Direction,
    /// Our key for SRTP (we send with it); tag is echoed from the offer in answers.
    pub crypto: Option<(u32, CryptoSuite, Vec<u8>)>,
    /// Use RTP/SAVP in the m= line.
    pub savp: bool,
}

/// One payload format in an SDP we build.
#[derive(Clone, Copy, Debug)]
pub struct Fmt {
    pub codec: CodecKind,
    pub pt: u8,
}

fn rtpmap(f: Fmt) -> String {
    match f.codec {
        CodecKind::Opus => format!(
            "a=rtpmap:{} opus/48000/2\r\na=fmtp:{} useinbandfec=1;minptime=10\r\n",
            f.pt, f.pt
        ),
        CodecKind::G722 => format!("a=rtpmap:{} G722/8000\r\n", f.pt),
        CodecKind::Pcmu => format!("a=rtpmap:{} PCMU/8000\r\n", f.pt),
        CodecKind::Pcma => format!("a=rtpmap:{} PCMA/8000\r\n", f.pt),
    }
}

/// Build an SDP body. `te` lists (pt, clock) telephone-event formats to include.
pub fn build(local: &Local, fmts: &[Fmt], te: &[(u8, u32)]) -> String {
    let ipv = if local.ip.is_ipv4() { "IP4" } else { "IP6" };
    let mut s = String::new();
    let _ = write!(
        s,
        "v=0\r\no=- {} {} IN {ipv} {}\r\ns=MegaSIP\r\nc=IN {ipv} {}\r\nt=0 0\r\n",
        local.session_id, local.version, local.ip, local.ip
    );
    let proto = if local.savp { "RTP/SAVP" } else { "RTP/AVP" };
    let mut pts: Vec<String> = fmts.iter().map(|f| f.pt.to_string()).collect();
    pts.extend(te.iter().map(|t| t.0.to_string()));
    let _ = write!(s, "m=audio {} {proto} {}\r\n", local.port, pts.join(" "));
    for f in fmts {
        s.push_str(&rtpmap(*f));
    }
    for (pt, clock) in te {
        let _ = write!(
            s,
            "a=rtpmap:{pt} telephone-event/{clock}\r\na=fmtp:{pt} 0-16\r\n"
        );
    }
    s.push_str("a=ptime:20\r\na=maxptime:150\r\n");
    if let Some((tag, suite, key)) = &local.crypto {
        let kl = suite.master_key_len();
        let attr = SdesAttribute {
            tag: *tag,
            crypto_suite: *suite,
            master_key: key[..kl].to_vec(),
            master_salt: key[kl..].to_vec(),
            lifetime: None,
            mki: None,
        };
        let _ = write!(s, "a=crypto:{}\r\n", attr.to_sdp());
    }
    let _ = write!(s, "a={}\r\n", dir_str(local.direction));
    s
}

/// An offer with every enabled codec.
pub fn offer(local: &Local, codecs: &[CodecKind]) -> String {
    let fmts: Vec<Fmt> = codecs
        .iter()
        .map(|&c| Fmt {
            codec: c,
            pt: c.static_pt().unwrap_or(OPUS_PT),
        })
        .collect();
    let mut te = vec![(TE8K_PT, 8000)];
    if codecs.contains(&CodecKind::Opus) {
        te.push((TE48K_PT, 48000));
    }
    build(local, &fmts, &te)
}

/// What the other side said.
#[derive(Clone, Debug)]
pub struct Remote {
    pub addr: Option<SocketAddr>,
    /// Formats in the remote's preference order.
    pub fmts: Vec<Fmt>,
    pub te: Vec<(u8, u32)>,
    pub direction: Direction,
    pub savp: bool,
    pub cryptos: Vec<SdesAttribute>,
    pub session_version: u64,
}

pub fn parse(body: &str) -> Option<Remote> {
    let sd = SessionDescription::parse(body).ok()?;
    let m = sd.audio_media()?;
    let conn = m.connection.as_ref().or(sd.connection.as_ref());
    let ip: Option<IpAddr> = conn.and_then(|c| c.address.split('/').next()?.parse().ok());
    let rejected = m.port == 0;
    let addr = match ip {
        Some(ip) if !ip.is_unspecified() && !rejected => Some(SocketAddr::new(ip, m.port)),
        _ => None,
    };
    let maps = m.rtpmaps();
    let mut fmts = Vec::new();
    let mut te = Vec::new();
    for f in &m.formats {
        let Ok(pt) = f.parse::<u8>() else { continue };
        let map = maps.iter().find(|r| r.payload_type == pt);
        let (enc, clock) = match map {
            Some(r) => (r.encoding.to_ascii_lowercase(), r.clock_rate),
            None => match pt {
                0 => ("pcmu".into(), 8000),
                8 => ("pcma".into(), 8000),
                9 => ("g722".into(), 8000),
                _ => continue,
            },
        };
        match (enc.as_str(), clock) {
            ("pcmu", 8000) => fmts.push(Fmt {
                codec: CodecKind::Pcmu,
                pt,
            }),
            ("pcma", 8000) => fmts.push(Fmt {
                codec: CodecKind::Pcma,
                pt,
            }),
            ("g722", 8000) => fmts.push(Fmt {
                codec: CodecKind::G722,
                pt,
            }),
            ("opus", 48000) => fmts.push(Fmt {
                codec: CodecKind::Opus,
                pt,
            }),
            ("telephone-event", c) => te.push((pt, c)),
            _ => {}
        }
    }
    let mut direction = m.direction();
    // Old-style hold: c=0.0.0.0.
    if ip.is_some_and(|i| i.is_unspecified()) && direction == Direction::SendRecv {
        direction = Direction::SendOnly;
    }
    let cryptos = m
        .attributes
        .iter()
        .filter(|a| a.name.eq_ignore_ascii_case("crypto"))
        .filter_map(|a| SdesAttribute::parse(a.value.as_deref()?).ok())
        .collect();
    Some(Remote {
        addr,
        fmts,
        te,
        direction,
        savp: m.protocol.to_ascii_uppercase().contains("SAVP"),
        cryptos,
        session_version: sd.origin.session_version.parse().unwrap_or(0),
    })
}

/// Pick the codec: first of the remote's formats (in our priority when we answer).
pub fn choose(remote: &Remote, ours: &[CodecKind], we_are_answerer: bool) -> Option<Fmt> {
    if we_are_answerer {
        // MicroSIP's default: honour our own priority order.
        for c in ours {
            if let Some(f) = remote.fmts.iter().find(|f| f.codec == *c) {
                return Some(*f);
            }
        }
        None
    } else {
        remote
            .fmts
            .iter()
            .find(|f| ours.contains(&f.codec))
            .copied()
    }
}

/// The telephone-event format whose clock matches the codec.
pub fn choose_te(remote: &Remote, codec: CodecKind) -> Option<u8> {
    remote
        .te
        .iter()
        .find(|t| t.1 == codec.rtp_clock())
        .map(|t| t.0)
}

/// Our direction answering theirs, given whether we hold.
pub fn answer_direction(theirs: Direction, we_hold: bool) -> Direction {
    match (theirs, we_hold) {
        (Direction::Inactive, _) => Direction::Inactive,
        (Direction::SendOnly, _) => Direction::RecvOnly,
        (Direction::RecvOnly, true) => Direction::Inactive,
        (Direction::RecvOnly, false) => Direction::SendOnly,
        (Direction::SendRecv, true) => Direction::SendOnly,
        (Direction::SendRecv, false) => Direction::SendRecv,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offer_parses_back() {
        let local = Local {
            ip: "192.168.1.5".parse().unwrap(),
            port: 4000,
            session_id: 1,
            version: 1,
            direction: Direction::SendRecv,
            crypto: Some((1, CryptoSuite::AesCm128HmacSha1_80, vec![7u8; 30])),
            savp: true,
        };
        let s = offer(&local, &[CodecKind::Opus, CodecKind::Pcmu]);
        let r = parse(&s).expect("parse");
        assert_eq!(r.addr, Some("192.168.1.5:4000".parse().unwrap()));
        assert_eq!(r.fmts.len(), 2);
        assert_eq!(r.fmts[0].codec, CodecKind::Opus);
        assert_eq!(r.te.len(), 2);
        assert!(r.savp);
        assert_eq!(r.cryptos.len(), 1);
        assert_eq!(r.cryptos[0].master_key.len(), 16);
        let f = choose(&r, &[CodecKind::Pcmu, CodecKind::Opus], true).unwrap();
        assert_eq!(f.codec, CodecKind::Pcmu);
        assert_eq!(choose_te(&r, CodecKind::Pcmu), Some(TE8K_PT));
    }

    #[test]
    fn hold_detection() {
        let s = "v=0\r\no=- 1 2 IN IP4 0.0.0.0\r\ns=-\r\nc=IN IP4 0.0.0.0\r\nt=0 0\r\nm=audio 4000 RTP/AVP 0\r\n";
        let r = parse(s).unwrap();
        assert_eq!(r.direction, Direction::SendOnly);
        assert!(r.addr.is_none());
        assert_eq!(
            answer_direction(Direction::SendOnly, false),
            Direction::RecvOnly
        );
    }
}
