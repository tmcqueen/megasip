//! Minimal STUN (RFC 5389) binding requests, to learn our public address.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

const COOKIE: u32 = 0x2112_A442;

pub fn request(txid: &[u8; 12]) -> Vec<u8> {
    let mut v = Vec::with_capacity(20);
    v.extend_from_slice(&0x0001u16.to_be_bytes());
    v.extend_from_slice(&0u16.to_be_bytes());
    v.extend_from_slice(&COOKIE.to_be_bytes());
    v.extend_from_slice(txid);
    v
}

pub fn is_stun(d: &[u8]) -> bool {
    d.len() >= 20 && d[0] & 0xC0 == 0 && u32::from_be_bytes([d[4], d[5], d[6], d[7]]) == COOKIE
}

/// Parse a Binding Success Response, returning (transaction id, mapped address).
pub fn parse_response(d: &[u8]) -> Option<([u8; 12], SocketAddr)> {
    if !is_stun(d) || u16::from_be_bytes([d[0], d[1]]) != 0x0101 {
        return None;
    }
    let mut txid = [0u8; 12];
    txid.copy_from_slice(&d[8..20]);
    let len = u16::from_be_bytes([d[2], d[3]]) as usize;
    let body = d.get(20..20 + len)?;
    let mut i = 0;
    let mut mapped = None;
    while i + 4 <= body.len() {
        let t = u16::from_be_bytes([body[i], body[i + 1]]);
        let l = u16::from_be_bytes([body[i + 2], body[i + 3]]) as usize;
        let v = body.get(i + 4..i + 4 + l)?;
        if (t == 0x0020 || t == 0x0001) && v.len() >= 8 {
            let xor = t == 0x0020;
            let mut port = u16::from_be_bytes([v[2], v[3]]);
            if xor {
                port ^= (COOKIE >> 16) as u16;
            }
            let ip = match v[1] {
                1 => {
                    let mut b = [v[4], v[5], v[6], v[7]];
                    if xor {
                        for (k, x) in b.iter_mut().zip(COOKIE.to_be_bytes()) {
                            *k ^= x;
                        }
                    }
                    IpAddr::V4(Ipv4Addr::from(b))
                }
                2 if v.len() >= 20 => {
                    let mut b = [0u8; 16];
                    b.copy_from_slice(&v[4..20]);
                    if xor {
                        let mut key = COOKIE.to_be_bytes().to_vec();
                        key.extend_from_slice(&txid);
                        for (k, x) in b.iter_mut().zip(key) {
                            *k ^= x;
                        }
                    }
                    IpAddr::V6(Ipv6Addr::from(b))
                }
                _ => return None,
            };
            let a = SocketAddr::new(ip, port);
            if xor || mapped.is_none() {
                mapped = Some(a);
            }
        }
        i += 4 + l.div_ceil(4) * 4;
    }
    mapped.map(|m| (txid, m))
}

/// Blocking binding request on an RTP socket (used once per call when STUN is set).
pub fn binding_blocking(sock: &std::net::UdpSocket, server: SocketAddr) -> Option<SocketAddr> {
    use rand::RngCore;
    let mut txid = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut txid);
    let req = request(&txid);
    let was_nb = true;
    sock.set_nonblocking(false).ok()?;
    sock.set_read_timeout(Some(std::time::Duration::from_millis(250))).ok()?;
    let mut buf = [0u8; 512];
    let mut out = None;
    for _ in 0..2 {
        if sock.send_to(&req, server).is_err() {
            break;
        }
        if let Ok((n, _)) = sock.recv_from(&mut buf) {
            if let Some((id, a)) = parse_response(&buf[..n]) {
                if id == txid {
                    out = Some(a);
                    break;
                }
            }
        }
    }
    sock.set_read_timeout(None).ok();
    if was_nb {
        sock.set_nonblocking(true).ok();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_xor_mapped() {
        let txid = [1u8; 12];
        let mut r = vec![0x01, 0x01, 0, 12];
        r.extend_from_slice(&COOKIE.to_be_bytes());
        r.extend_from_slice(&txid);
        // XOR-MAPPED-ADDRESS 203.0.113.5:40000
        let port = 40000u16 ^ 0x2112;
        let ip = [203u8 ^ 0x21, 0 ^ 0x12, 113 ^ 0xA4, 5 ^ 0x42];
        r.extend_from_slice(&[0x00, 0x20, 0, 8, 0, 1]);
        r.extend_from_slice(&port.to_be_bytes());
        r.extend_from_slice(&ip);
        let (id, a) = parse_response(&r).unwrap();
        assert_eq!(id, txid);
        assert_eq!(a, "203.0.113.5:40000".parse().unwrap());
    }
}
