//! SIP transports: one shared UDP socket, plus outbound TCP/TLS connections (one per
//! account) that also carry the requests the server sends back to us.

use super::msg::frame_len;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Proto {
    Udp,
    Tcp,
    Tls,
}

impl Proto {
    pub fn via_name(self) -> &'static str {
        match self {
            Proto::Udp => "UDP",
            Proto::Tcp => "TCP",
            Proto::Tls => "TLS",
        }
    }
    pub fn uri_param(self) -> &'static str {
        match self {
            Proto::Udp => "",
            Proto::Tcp => ";transport=tcp",
            Proto::Tls => ";transport=tls",
        }
    }
    pub fn default_port(self) -> u16 {
        if self == Proto::Tls {
            5061
        } else {
            5060
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flow {
    Udp,
    Stream(u64),
}

#[derive(Debug)]
pub enum TransportEvent {
    Message { data: Vec<u8>, from: SocketAddr, flow: Flow },
    Closed(u64),
}

struct StreamConn {
    tx: mpsc::UnboundedSender<Vec<u8>>,
    local: SocketAddr,
    proto: Proto,
}

pub struct Transports {
    udp: Arc<UdpSocket>,
    pub udp_local: SocketAddr,
    streams: HashMap<u64, StreamConn>,
    next_id: u64,
    events: mpsc::UnboundedSender<TransportEvent>,
    tls: Arc<tokio_rustls::rustls::ClientConfig>,
}

fn tls_config() -> Arc<tokio_rustls::rustls::ClientConfig> {
    use tokio_rustls::rustls;
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cfg = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("tls versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    Arc::new(cfg)
}

impl Transports {
    pub async fn new(
        source_port: u16,
        events: mpsc::UnboundedSender<TransportEvent>,
    ) -> std::io::Result<Transports> {
        let udp = match UdpSocket::bind(("0.0.0.0", source_port)).await {
            Ok(s) => s,
            Err(e) if source_port != 0 => {
                tracing::warn!("cannot bind SIP port {source_port}: {e}; using a random port");
                UdpSocket::bind(("0.0.0.0", 0)).await?
            }
            Err(e) => return Err(e),
        };
        let udp = Arc::new(udp);
        let udp_local = udp.local_addr()?;
        let rx_sock = udp.clone();
        let ev = events.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 65535];
            loop {
                match rx_sock.recv_from(&mut buf).await {
                    Ok((n, from)) => {
                        if n <= 4 && buf[..n].iter().all(|b| *b == b'\r' || *b == b'\n') {
                            continue; // keep-alive
                        }
                        if ev
                            .send(TransportEvent::Message { data: buf[..n].to_vec(), from, flow: Flow::Udp })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(e) => {
                        // ICMP port unreachable shows up here on Linux; keep going.
                        tracing::debug!("udp recv: {e}");
                    }
                }
            }
        });
        Ok(Transports { udp, udp_local, streams: HashMap::new(), next_id: 1, events, tls: tls_config() })
    }

    /// Take over a connected stream (from [`dial`]) and start reading SIP from it.
    pub fn attach(&mut self, stream: BoxStream, peer: SocketAddr, local: SocketAddr, proto: Proto) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let (tx, rx) = mpsc::unbounded_channel::<Vec<u8>>();
        tokio::spawn(run_stream(stream, id, peer, rx, self.events.clone()));
        self.streams.insert(id, StreamConn { tx, local, proto });
        id
    }

    pub fn tls_config(&self) -> Arc<tokio_rustls::rustls::ClientConfig> {
        self.tls.clone()
    }

    pub fn is_open(&self, id: u64) -> bool {
        self.streams.get(&id).is_some_and(|s| !s.tx.is_closed())
    }

    pub fn stream_local(&self, id: u64) -> Option<(SocketAddr, Proto)> {
        self.streams.get(&id).map(|s| (s.local, s.proto))
    }

    pub fn closed(&mut self, id: u64) {
        self.streams.remove(&id);
    }

    pub fn send(&self, flow: Flow, dest: SocketAddr, data: &[u8]) {
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("SEND {:?} -> {dest}\n{}", flow, String::from_utf8_lossy(data));
        }
        match flow {
            Flow::Udp => {
                let sock = self.udp.clone();
                let data = data.to_vec();
                // try_send_to avoids awaiting in the engine loop; fall back to a task.
                if sock.try_send_to(&data, dest).is_err() {
                    tokio::spawn(async move {
                        let _ = sock.send_to(&data, dest).await;
                    });
                }
            }
            Flow::Stream(id) => {
                if let Some(s) = self.streams.get(&id) {
                    let _ = s.tx.send(data.to_vec());
                }
            }
        }
    }
}

pub trait AsyncStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> AsyncStream for T {}
pub type BoxStream = Box<dyn AsyncStream>;

/// Connect TCP (and TLS on top). Runs off the engine loop.
pub async fn dial(
    proto: Proto,
    addr: SocketAddr,
    server_name: String,
    tls: Arc<tokio_rustls::rustls::ClientConfig>,
) -> std::io::Result<(BoxStream, SocketAddr)> {
    let tcp = tokio::time::timeout(std::time::Duration::from_secs(10), TcpStream::connect(addr))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "connect timed out"))??;
    tcp.set_nodelay(true).ok();
    let local = tcp.local_addr()?;
    if proto != Proto::Tls {
        return Ok((Box::new(tcp), local));
    }
    let name = tokio_rustls::rustls::pki_types::ServerName::try_from(server_name)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let conn = tokio_rustls::TlsConnector::from(tls);
    let stream = tokio::time::timeout(std::time::Duration::from_secs(10), conn.connect(name, tcp))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "TLS handshake timed out"))??;
    Ok((Box::new(stream), local))
}

async fn run_stream<S>(
    stream: S,
    id: u64,
    peer: SocketAddr,
    mut outgoing: mpsc::UnboundedReceiver<Vec<u8>>,
    ev: mpsc::UnboundedSender<TransportEvent>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut rd, mut wr) = tokio::io::split(stream);
    let writer = tokio::spawn(async move {
        while let Some(buf) = outgoing.recv().await {
            if wr.write_all(&buf).await.is_err() {
                break;
            }
        }
        let _ = wr.shutdown().await;
    });
    let mut buf: Vec<u8> = Vec::with_capacity(16384);
    let mut chunk = vec![0u8; 16384];
    loop {
        // Drop leading keep-alive CRLFs.
        while buf.first().is_some_and(|b| *b == b'\r' || *b == b'\n') {
            buf.remove(0);
        }
        if let Some(n) = frame_len(&buf) {
            let msg: Vec<u8> = buf.drain(..n).collect();
            if ev.send(TransportEvent::Message { data: msg, from: peer, flow: Flow::Stream(id) }).is_err() {
                break;
            }
            continue;
        }
        if buf.len() > 256 * 1024 {
            break; // garbage
        }
        match rd.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    writer.abort();
    let _ = ev.send(TransportEvent::Closed(id));
}

/// The local IP the OS would use to reach `dest` (no packets are sent).
pub fn local_ip_for(dest: SocketAddr) -> Option<std::net::IpAddr> {
    let s = std::net::UdpSocket::bind(if dest.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }).ok()?;
    s.connect(dest).ok()?;
    s.local_addr().ok().map(|a| a.ip())
}

/// Resolve a SIP server ("host", "host:port", or IP) to a socket address and transport.
/// Uses RFC 3263 SRV lookups (via rsiprtp's resolver) when no port is given.
pub async fn resolve(host_port: &str, proto: Option<Proto>, use_srv: bool) -> Option<(SocketAddr, Proto)> {
    let hp = host_port.trim();
    let (host, port) = split_host_port(hp);
    let proto_or = proto.unwrap_or(Proto::Udp);
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return Some((SocketAddr::new(ip, port.unwrap_or(proto_or.default_port())), proto_or));
    }
    if port.is_none() && use_srv {
        // hickory's futures are not Send; resolve on a small private runtime.
        let host_s = host.to_string();
        let found = tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
            rt.block_on(async move {
                let res = rsiprtp::transport::SipResolver::new().await.ok()?;
                let want = proto.map(|p| match p {
                    Proto::Udp => rsiprtp::transport::TransportProtocol::Udp,
                    Proto::Tcp => rsiprtp::transport::TransportProtocol::Tcp,
                    Proto::Tls => rsiprtp::transport::TransportProtocol::Tls,
                });
                let targets = tokio::time::timeout(std::time::Duration::from_secs(6), res.resolve(&host_s, want))
                    .await
                    .ok()?
                    .ok()?;
                for t in targets {
                    let p = match t.transport {
                        rsiprtp::transport::TransportProtocol::Tcp => Proto::Tcp,
                        rsiprtp::transport::TransportProtocol::Tls => Proto::Tls,
                        rsiprtp::transport::TransportProtocol::Udp => Proto::Udp,
                        #[allow(unreachable_patterns)]
                        _ => continue,
                    };
                    if proto.is_some_and(|w| w != p) {
                        continue;
                    }
                    if let Some(a) = t.socket_addrs().into_iter().find(|a| a.is_ipv4()) {
                        return Some((a, p));
                    }
                }
                None
            })
        })
        .await
        .ok()
        .flatten();
        if found.is_some() {
            return found;
        }
    }
    let port = port.unwrap_or(proto_or.default_port());
    let addrs = tokio::net::lookup_host((host, port)).await.ok()?;
    let v: Vec<SocketAddr> = addrs.collect();
    v.iter().find(|a| a.is_ipv4()).or(v.first()).map(|a| (*a, proto_or))
}

pub fn split_host_port(hp: &str) -> (&str, Option<u16>) {
    if let Some(rest) = hp.strip_prefix('[') {
        if let Some((h, tail)) = rest.split_once(']') {
            return (h, tail.strip_prefix(':').and_then(|p| p.parse().ok()));
        }
    }
    match hp.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => match p.parse() {
            Ok(port) => (h, Some(port)),
            Err(_) => (hp, None),
        },
        _ => (hp, None),
    }
}
