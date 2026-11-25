//! Media: per-call RTP streams (codec, SRTP, playout buffer, DTMF) and the audio hub that
//! mixes them with the microphone and speaker.

pub mod dsp;
pub mod hub;
pub mod source;

use dsp::Resampler;
use rsiprtp::media::{G711Codec, G722Codec, OpusCodec};
use rsiprtp::rtp::{DtmfDigit, DtmfEvent, DtmfReceiver, RtpPacket};
use rsiprtp::srtp::{CryptoSuite, SrtpContext};
use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub const FRAME: usize = 960; // 20 ms at 48 kHz

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodecKind {
    Opus,
    G722,
    Pcmu,
    Pcma,
}

impl CodecKind {
    pub fn from_name(n: &str) -> Option<CodecKind> {
        match n.to_ascii_lowercase().as_str() {
            "opus" => Some(CodecKind::Opus),
            "g722" => Some(CodecKind::G722),
            "pcmu" => Some(CodecKind::Pcmu),
            "pcma" => Some(CodecKind::Pcma),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            CodecKind::Opus => "Opus",
            CodecKind::G722 => "G.722",
            CodecKind::Pcmu => "G.711 µ-law",
            CodecKind::Pcma => "G.711 A-law",
        }
    }
    pub fn rtp_clock(self) -> u32 {
        if self == CodecKind::Opus {
            48_000
        } else {
            8_000
        }
    }
    pub fn pcm_rate(self) -> u32 {
        match self {
            CodecKind::Opus => 48_000,
            CodecKind::G722 => 16_000,
            _ => 8_000,
        }
    }
    pub fn static_pt(self) -> Option<u8> {
        match self {
            CodecKind::Pcmu => Some(0),
            CodecKind::Pcma => Some(8),
            CodecKind::G722 => Some(9),
            CodecKind::Opus => None,
        }
    }
    pub fn ts_per_frame(self) -> u32 {
        self.rtp_clock() / 50
    }
}

enum Codec {
    G711(G711Codec),
    G722(Box<G722Codec>),
    Opus(Box<OpusCodec>),
}

impl Codec {
    fn new(kind: CodecKind) -> Codec {
        match kind {
            CodecKind::Pcmu => Codec::G711(G711Codec::pcmu()),
            CodecKind::Pcma => Codec::G711(G711Codec::pcma()),
            CodecKind::G722 => Codec::G722(Box::new(G722Codec::new())),
            CodecKind::Opus => match OpusCodec::new() {
                Ok(c) => Codec::Opus(Box::new(c)),
                Err(e) => {
                    tracing::error!("opus init failed: {e}; falling back to PCMU");
                    Codec::G711(G711Codec::pcmu())
                }
            },
        }
    }
    fn encode(&mut self, pcm: &[i16]) -> Vec<u8> {
        match self {
            Codec::G711(c) => c.encode(pcm),
            Codec::G722(c) => c.encode(pcm),
            Codec::Opus(c) => c.encode(pcm).unwrap_or_default(),
        }
    }
    fn decode(&mut self, data: &[u8]) -> Vec<i16> {
        match self {
            Codec::G711(c) => c.decode(data),
            Codec::G722(c) => c.decode(data),
            Codec::Opus(c) => c.decode(data).unwrap_or_default(),
        }
    }
    fn plc(&mut self) -> Option<Vec<i16>> {
        match self {
            Codec::Opus(c) => c.decode_plc().ok(),
            _ => None,
        }
    }
}

/// What the call layer negotiated for a stream.
#[derive(Clone, Debug)]
pub struct StreamConfig {
    pub remote: Option<SocketAddr>,
    pub codec: CodecKind,
    pub pt: u8,
    pub te_pt: Option<u8>,
    /// (suite, key||salt) for sending and receiving.
    pub srtp_tx: Option<(CryptoSuite, Vec<u8>)>,
    pub srtp_rx: Option<(CryptoSuite, Vec<u8>)>,
    /// We may send audio (not on hold either way).
    pub send: bool,
    /// We play what we receive (we have not put the call on hold).
    pub play: bool,
}

pub enum StreamEvent {
    Dtmf(u64, char),
}

struct DtmfTx {
    digit: DtmfDigit,
    start_ts: u32,
    elapsed: u32,
    total: u32,
    end_sent: u8,
    first: bool,
}

struct Inner {
    cfg: StreamConfig,
    latched: bool,
    srtp_tx: Option<SrtpContext>,
    srtp_rx: Option<SrtpContext>,
    // send side
    encoder: Codec,
    down: Resampler,
    seq: u16,
    ts: u32,
    ssrc: u32,
    marker: bool,
    dtmf_queue: VecDeque<(char, u32)>,
    dtmf: Option<DtmfTx>,
    inband: VecDeque<i16>,
    // receive side
    decoder: Codec,
    up: Resampler,
    reorder: BTreeMap<u64, Vec<u8>>,
    expected: Option<u64>,
    last_seq: u16,
    ext_base: u64,
    last_ssrc: Option<u32>,
    fifo: VecDeque<i16>,
    buffering: bool,
    last_frame: Vec<i16>,
    dtmf_rx: DtmfReceiver,
    last_dtmf: Option<char>,
    rx_packets: u64,
    last_rx: Option<Instant>,
    st: Counters,
}

/// Running counters for the Debug window's call quality view.
struct Counters {
    started: Instant,
    rx_bytes: u64,
    tx_packets: u64,
    tx_bytes: u64,
    srtp_failures: u64,
    ignored: u64,
    late: u64,
    duplicates: u64,
    concealed: u64,
    underruns: u64,
    overflow_samples: u64,
    drift_skips: u64,
    /// Packets expected from sequence numbers, and media packets actually received.
    expected: u64,
    received: u64,
    highest: Option<u64>,
    /// RFC 3550 interarrival jitter, in RTP clock units.
    jitter: f64,
    last_transit: Option<f64>,
    dtmf_rx: u64,
    dtmf_tx: u64,
    played_any: bool,
}

impl Counters {
    fn new() -> Self {
        Counters {
            started: Instant::now(),
            rx_bytes: 0,
            tx_packets: 0,
            tx_bytes: 0,
            srtp_failures: 0,
            ignored: 0,
            late: 0,
            duplicates: 0,
            concealed: 0,
            underruns: 0,
            overflow_samples: 0,
            drift_skips: 0,
            expected: 0,
            received: 0,
            highest: None,
            jitter: 0.0,
            last_transit: None,
            dtmf_rx: 0,
            dtmf_tx: 0,
            played_any: false,
        }
    }
}

/// A snapshot of one call's media, for the Debug window.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamStats {
    pub id: u64,
    pub codec: String,
    pub payload_type: u8,
    pub srtp: bool,
    pub local_port: u16,
    pub remote: String,
    pub sending: bool,
    pub playing: bool,
    pub duration_s: f64,
    pub rx_packets: u64,
    pub rx_bytes: u64,
    pub tx_packets: u64,
    pub tx_bytes: u64,
    pub expected: u64,
    pub lost: u64,
    pub loss_pct: f64,
    pub late: u64,
    pub duplicates: u64,
    pub concealed: u64,
    pub underruns: u64,
    pub overflow_ms: u64,
    pub drift_skips: u64,
    pub srtp_failures: u64,
    pub ignored: u64,
    pub jitter_ms: f64,
    pub buffer_ms: f64,
    pub target_buffer_ms: f64,
    pub last_rx_ms: Option<u64>,
    pub dtmf_rx: u64,
    pub dtmf_tx: u64,
    /// Simplified ITU-T G.107 E-model; None until there's enough traffic.
    pub r_factor: Option<f64>,
    pub mos: Option<f64>,
    pub ended: bool,
}

pub struct Stream {
    pub id: u64,
    sock: std::net::UdpSocket,
    inner: Mutex<Inner>,
    rx_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

const TARGET: usize = 48 * 60;
const HIGH: usize = 48 * 140;
const MAX: usize = 48 * 400;

fn srtp_ctx(p: &Option<(CryptoSuite, Vec<u8>)>) -> Option<SrtpContext> {
    let (suite, km) = p.as_ref()?;
    let kl = suite.master_key_len();
    if km.len() < kl + suite.master_salt_len() {
        return None;
    }
    SrtpContext::new(*suite, &km[..kl], &km[kl..kl + suite.master_salt_len()]).ok()
}

impl Stream {
    /// The stream state, even if a previous holder panicked (keep the audio thread alive).
    fn lk(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Bind an RTP socket on an even port in the range.
    pub fn bind(id: u64, range: (u16, u16)) -> std::io::Result<std::net::UdpSocket> {
        use rand::Rng;
        let (lo, hi) = (range.0.max(1024), range.1.max(range.0.max(1024) + 2));
        let mut r = rand::thread_rng();
        for _ in 0..64 {
            let p = r.gen_range(lo..hi) & !1;
            if let Ok(s) = std::net::UdpSocket::bind(("0.0.0.0", p)) {
                let _ = id;
                return Ok(s);
            }
        }
        std::net::UdpSocket::bind(("0.0.0.0", 0))
    }

    pub fn new(
        id: u64,
        sock: std::net::UdpSocket,
        cfg: StreamConfig,
        events: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    ) -> std::io::Result<Arc<Stream>> {
        use rand::Rng;
        sock.set_nonblocking(true)?;
        let rx_sock = tokio::net::UdpSocket::from_std(sock.try_clone()?)?;
        let codec = cfg.codec;
        let te = cfg.te_pt;
        let mut r = rand::thread_rng();
        let inner = Inner {
            srtp_tx: srtp_ctx(&cfg.srtp_tx),
            srtp_rx: srtp_ctx(&cfg.srtp_rx),
            latched: false,
            encoder: Codec::new(codec),
            down: Resampler::new(48_000, codec.pcm_rate()),
            seq: r.gen(),
            ts: r.gen(),
            ssrc: r.gen(),
            marker: true,
            dtmf_queue: VecDeque::new(),
            dtmf: None,
            inband: VecDeque::new(),
            decoder: Codec::new(codec),
            up: Resampler::new(codec.pcm_rate(), 48_000),
            reorder: BTreeMap::new(),
            expected: None,
            last_seq: 0,
            ext_base: 0,
            last_ssrc: None,
            fifo: VecDeque::with_capacity(MAX),
            buffering: true,
            last_frame: Vec::new(),
            dtmf_rx: DtmfReceiver::new(te.unwrap_or(101)),
            last_dtmf: None,
            rx_packets: 0,
            last_rx: None,
            st: Counters::new(),
            cfg,
        };
        let s = Arc::new(Stream { id, sock, inner: Mutex::new(inner), rx_task: Mutex::new(None) });
        let weak = Arc::downgrade(&s);
        let task = tokio::spawn(async move {
            let mut buf = vec![0u8; 2048];
            loop {
                let Ok((n, from)) = rx_sock.recv_from(&mut buf).await else {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    if weak.strong_count() == 0 {
                        break;
                    }
                    continue;
                };
                let Some(s) = weak.upgrade() else { break };
                if let Some(d) = s.on_packet(&buf[..n], from) {
                    let _ = events.send(StreamEvent::Dtmf(s.id, d));
                }
            }
        });
        *s.rx_task.lock().unwrap() = Some(task);
        Ok(s)
    }

    /// Apply a renegotiated configuration (re-INVITE, hold, codec change).
    pub fn update(&self, cfg: StreamConfig) {
        let mut g = self.lk();
        if g.cfg.codec != cfg.codec {
            g.encoder = Codec::new(cfg.codec);
            g.decoder = Codec::new(cfg.codec);
            g.down = Resampler::new(48_000, cfg.codec.pcm_rate());
            g.up = Resampler::new(cfg.codec.pcm_rate(), 48_000);
            g.reorder.clear();
            g.expected = None;
        }
        if g.cfg.srtp_tx.as_ref().map(|x| &x.1) != cfg.srtp_tx.as_ref().map(|x| &x.1) {
            g.srtp_tx = srtp_ctx(&cfg.srtp_tx);
        }
        if g.cfg.srtp_rx.as_ref().map(|x| &x.1) != cfg.srtp_rx.as_ref().map(|x| &x.1) {
            g.srtp_rx = srtp_ctx(&cfg.srtp_rx);
        }
        if g.cfg.remote != cfg.remote {
            g.latched = false;
        }
        if !g.cfg.send && cfg.send {
            g.marker = true;
        }
        if let Some(te) = cfg.te_pt {
            if g.cfg.te_pt != Some(te) {
                g.dtmf_rx = DtmfReceiver::new(te);
            }
        }
        g.cfg = cfg;
    }

    pub fn config(&self) -> StreamConfig {
        self.lk().cfg.clone()
    }

    /// Start the quality counters over (Debug window → Clear).
    pub fn reset_stats(&self) {
        let mut g = self.lk();
        g.st = Counters::new();
        g.rx_packets = 0;
    }

    pub fn stats(&self) -> StreamStats {
        let g = self.lk();
        let c = &g.st;
        let lost = c.expected.saturating_sub(c.received);
        let loss_pct = if c.expected > 0 { lost as f64 * 100.0 / c.expected as f64 } else { 0.0 };
        let jitter_ms = c.jitter * 1000.0 / g.cfg.codec.rtp_clock() as f64;
        let buffer_ms = g.fifo.len() as f64 / 48.0;
        // One-way delay we can account for: packetisation + playout buffer + device
        // buffers, plus a nominal 40 ms for the network we can't see.
        let delay = 20.0 + buffer_ms + 20.0 + 40.0 + jitter_ms * 2.0;
        let (r, mos) = estimate_mos(g.cfg.codec, delay, loss_pct);
        let enough = c.received >= 50;
        StreamStats {
            id: self.id,
            codec: g.cfg.codec.label().to_string(),
            payload_type: g.cfg.pt,
            srtp: g.srtp_rx.is_some() || g.srtp_tx.is_some(),
            local_port: self.sock.local_addr().map(|a| a.port()).unwrap_or(0),
            remote: g.cfg.remote.map(|a| a.to_string()).unwrap_or_default(),
            sending: g.cfg.send,
            playing: g.cfg.play,
            duration_s: c.started.elapsed().as_secs_f64(),
            rx_packets: g.rx_packets,
            rx_bytes: c.rx_bytes,
            tx_packets: c.tx_packets,
            tx_bytes: c.tx_bytes,
            expected: c.expected,
            lost,
            loss_pct,
            late: c.late,
            duplicates: c.duplicates,
            concealed: c.concealed,
            underruns: c.underruns,
            overflow_ms: c.overflow_samples / 48,
            drift_skips: c.drift_skips,
            srtp_failures: c.srtp_failures,
            ignored: c.ignored,
            jitter_ms,
            buffer_ms,
            target_buffer_ms: TARGET as f64 / 48.0,
            last_rx_ms: g.last_rx.map(|t| t.elapsed().as_millis() as u64),
            dtmf_rx: c.dtmf_rx,
            dtmf_tx: c.dtmf_tx,
            r_factor: enough.then_some(r),
            mos: enough.then_some(mos),
            ended: false,
        }
    }

    pub fn playing(&self) -> bool {
        self.lk().cfg.play
    }

    pub fn sending(&self) -> bool {
        let g = self.lk();
        g.cfg.send && g.cfg.remote.is_some()
    }

    /// Queue a DTMF digit as RFC 4733 events. Returns false when telephone-event
    /// was not negotiated.
    pub fn send_dtmf_rfc(&self, digit: char, ms: u32) -> bool {
        let mut g = self.lk();
        if g.cfg.te_pt.is_none() || DtmfDigit::from_char(digit).is_none() {
            return false;
        }
        g.dtmf_queue.push_back((digit, ms));
        true
    }

    /// Mix a DTMF tone into the outgoing audio.
    pub fn send_dtmf_inband(&self, digit: char) {
        let mut g = self.lk();
        g.inband.extend(dsp::dtmf_tone(digit, 120));
        g.inband.extend(std::iter::repeat_n(0i16, 48 * 60));
    }

    fn on_packet(&self, data: &[u8], from: SocketAddr) -> Option<char> {
        if data.len() < 12 || data[0] >> 6 != 2 {
            return None;
        }
        // RTCP (rtcp-mux) — ignore.
        if (200..=206).contains(&data[1]) || (72..=76).contains(&(data[1] & 0x7f)) {
            return None;
        }
        let mut g = self.lk();
        let plain;
        let data = if let Some(ctx) = g.srtp_rx.as_mut() {
            match ctx.unprotect(data) {
                Ok(b) => {
                    plain = b;
                    &plain[..]
                }
                Err(_) => {
                    g.st.srtp_failures += 1;
                    return None;
                }
            }
        } else {
            data
        };
        let Ok(pkt) = RtpPacket::parse(data) else {
            g.st.ignored += 1;
            return None;
        };
        let pt = pkt.payload_type;
        let is_te = g.cfg.te_pt == Some(pt);
        if pt != g.cfg.pt && !is_te {
            g.st.ignored += 1;
            return None; // comfort noise or an unnegotiated payload
        }
        // Symmetric RTP: send back to where the media really comes from.
        if !g.latched || g.cfg.remote != Some(from) {
            if g.cfg.remote != Some(from) {
                tracing::debug!("rtp latch {:?} -> {from}", g.cfg.remote);
            }
            g.cfg.remote = Some(from);
            g.latched = true;
        }
        g.rx_packets += 1;
        g.st.rx_bytes += data.len() as u64;
        g.last_rx = Some(Instant::now());
        if is_te {
            let d = g.dtmf_rx.process_packet(pt, &pkt.payload).map(|d| d.as_char());
            if d.is_some() && d != g.last_dtmf {
                g.last_dtmf = d;
                g.st.dtmf_rx += 1;
                return d;
            }
            if d.is_none() {
                g.last_dtmf = None;
            }
            return None;
        }
        if !g.cfg.play {
            return None;
        }
        let ssrc = pkt.ssrc;
        let seq = pkt.sequence_number;
        // Extended sequence number.
        let ext = if g.last_ssrc != Some(ssrc) || g.expected.is_none() {
            g.last_ssrc = Some(ssrc);
            g.reorder.clear();
            g.ext_base = 1 << 20;
            g.last_seq = seq;
            let e = g.ext_base + seq as u64;
            g.expected = Some(e);
            e
        } else {
            let diff = seq.wrapping_sub(g.last_seq) as i16 as i64;
            let prev = g.ext_base + g.last_seq as u64;
            let e = (prev as i64 + diff).max(0) as u64;
            if diff > 0 {
                g.last_seq = seq;
                if (seq as u64) < (prev & 0xffff) {
                    g.ext_base += 1 << 16;
                }
            }
            e
        };
        // RFC 3550 interarrival jitter.
        let clock = g.cfg.codec.rtp_clock() as f64;
        let arrival = g.st.started.elapsed().as_secs_f64() * clock;
        let transit = arrival - pkt.timestamp as f64;
        if let Some(last) = g.st.last_transit {
            let d = (transit - last).abs();
            if d < clock * 10.0 {
                g.st.jitter += (d - g.st.jitter) / 16.0;
            }
        }
        g.st.last_transit = Some(transit);

        let expected = g.expected.unwrap();
        if ext + 50 < expected || ext > expected + 1000 {
            // Discontinuity: start over.
            g.reorder.clear();
            g.expected = Some(ext);
            g.st.highest = None;
            g.st.last_transit = None;
        } else if ext < expected {
            g.st.late += 1;
            return None; // late
        }
        match g.st.highest {
            None => {
                g.st.expected += 1;
                g.st.highest = Some(ext);
            }
            Some(h) if ext > h => {
                g.st.expected += ext - h;
                g.st.highest = Some(ext);
            }
            _ => {}
        }
        if g.reorder.insert(ext, pkt.payload.to_vec()).is_some() {
            g.st.duplicates += 1;
        } else {
            g.st.received += 1;
        }
        self.drain(&mut g);
        None
    }

    fn drain(&self, g: &mut Inner) {
        loop {
            let Some(exp) = g.expected else { return };
            if let Some(payload) = g.reorder.remove(&exp) {
                let pcm = g.decoder.decode(&payload);
                let hi = g.up.process(&pcm);
                g.last_frame = hi.clone();
                push_fifo(g, &hi);
                g.expected = Some(exp + 1);
            } else if g.reorder.len() >= 3 {
                // Give up waiting for the missing packet: conceal it.
                let concealed = match g.decoder.plc() {
                    Some(pcm) => g.up.process(&pcm),
                    None => g.last_frame.iter().map(|&s| s / 2).collect(),
                };
                g.last_frame = concealed.clone();
                g.st.concealed += 1;
                push_fifo(g, &concealed);
                g.expected = Some(exp + 1);
            } else {
                return;
            }
        }
    }

    /// Pull 20 ms of received audio at 48 kHz.
    pub fn pull(&self, out: &mut [i32]) {
        let mut g = self.lk();
        if !g.cfg.play {
            return;
        }
        if g.buffering {
            if g.fifo.len() < TARGET {
                return;
            }
            g.buffering = false;
            g.st.played_any = true;
        }
        if g.fifo.len() > HIGH {
            // Running ahead (clock drift / burst): skip 2 ms.
            g.fifo.drain(..96);
            g.st.drift_skips += 1;
        }
        let n = out.len().min(g.fifo.len());
        for (o, s) in out.iter_mut().zip(g.fifo.drain(..n)) {
            *o += s as i32;
        }
        if n < out.len() {
            g.buffering = true;
            if g.st.played_any {
                g.st.underruns += 1;
            }
        }
    }

    /// Send 20 ms of 48 kHz audio (or the pending DTMF event).
    pub fn push(&self, frame: &[i16]) {
        let mut g = self.lk();
        let codec = g.cfg.codec;
        let tpf = codec.ts_per_frame();
        let Some(remote) = g.cfg.remote else {
            g.ts = g.ts.wrapping_add(tpf);
            return;
        };
        if !g.cfg.send {
            g.ts = g.ts.wrapping_add(tpf);
            return;
        }
        if g.dtmf.is_none() {
            if let Some((d, ms)) = g.dtmf_queue.pop_front() {
                if let Some(digit) = DtmfDigit::from_char(d) {
                    g.dtmf = Some(DtmfTx {
                        digit,
                        start_ts: g.ts,
                        elapsed: 0,
                        total: codec.rtp_clock() * ms / 1000,
                        end_sent: 0,
                        first: true,
                    });
                }
            }
        }
        let mut packets: Vec<Vec<u8>> = Vec::new();
        if let (Some(te), Some(mut d)) = (g.cfg.te_pt, g.dtmf.take()) {
            d.elapsed = (d.elapsed + tpf).min(d.total);
            let mut ev = DtmfEvent::new(d.digit, d.elapsed.min(u16::MAX as u32) as u16).with_volume(10);
            let ending = d.elapsed >= d.total;
            if ending {
                ev = ev.with_end();
                d.end_sent += 1;
            }
            let pkt = RtpPacket::new(te, g.seq, d.start_ts, g.ssrc)
                .with_marker(d.first)
                .with_payload(ev.encode().to_vec());
            if d.first {
                g.st.dtmf_tx += 1;
            }
            d.first = false;
            g.seq = g.seq.wrapping_add(1);
            packets.push(pkt.build().to_vec());
            if !(ending && d.end_sent >= 3) {
                g.dtmf = Some(d);
            }
        } else {
            let mut pcm: Vec<i16> = frame.to_vec();
            if !g.inband.is_empty() {
                for s in pcm.iter_mut() {
                    if let Some(t) = g.inband.pop_front() {
                        *s = s.saturating_add(t);
                    }
                }
            }
            let low = g.down.process(&pcm);
            let payload = g.encoder.encode(&low);
            if !payload.is_empty() {
                let m = g.marker;
                let pkt = RtpPacket::new(g.cfg.pt, g.seq, g.ts, g.ssrc).with_marker(m).with_payload(payload);
                g.marker = false;
                g.seq = g.seq.wrapping_add(1);
                packets.push(pkt.build().to_vec());
            }
        }
        g.ts = g.ts.wrapping_add(tpf);
        for p in packets {
            let out = match g.srtp_tx.as_mut() {
                Some(ctx) => match ctx.protect(&p) {
                    Ok(b) => b.to_vec(),
                    Err(_) => continue,
                },
                None => p,
            };
            if self.sock.send_to(&out, remote).is_ok() {
                g.st.tx_packets += 1;
                g.st.tx_bytes += out.len() as u64;
            }
        }
    }
}

fn push_fifo(g: &mut Inner, pcm: &[i16]) {
    g.fifo.extend(pcm.iter().copied());
    if g.fifo.len() > MAX {
        let extra = g.fifo.len() - TARGET;
        g.fifo.drain(..extra);
        g.st.overflow_samples += extra as u64;
    }
}

/// Simplified E-model (ITU-T G.107): R factor and MOS from one-way delay and loss.
pub fn estimate_mos(codec: CodecKind, delay_ms: f64, loss_pct: f64) -> (f64, f64) {
    let id = if delay_ms < 177.3 { 0.024 * delay_ms } else { 0.024 * delay_ms + 0.11 * (delay_ms - 177.3) };
    // Packet-loss robustness (Bpl) with our concealment; equipment impairment Ie ~ 0
    // for these codecs on the narrowband scale.
    let (ie, bpl) = match codec {
        CodecKind::Opus => (0.0, 20.0),
        CodecKind::G722 => (0.0, 10.0),
        CodecKind::Pcmu | CodecKind::Pcma => (0.0, 25.1),
    };
    let ie_eff = ie + (95.0 - ie) * loss_pct / (loss_pct + bpl);
    let r = (93.2 - id - ie_eff).clamp(0.0, 100.0);
    let mos = if r <= 0.0 { 1.0 } else { (1.0 + 0.035 * r + 7e-6 * r * (r - 60.0) * (100.0 - r)).clamp(1.0, 4.5) };
    (r, mos)
}

impl Drop for Stream {
    fn drop(&mut self) {
        if let Some(t) = self.rx_task.lock().unwrap().take() {
            t.abort();
        }
    }
}

/// Random SDES key material for a suite.
pub fn new_srtp_key(suite: CryptoSuite) -> Vec<u8> {
    use rand::RngCore;
    let mut k = vec![0u8; suite.master_key_len() + suite.master_salt_len()];
    rand::thread_rng().fill_bytes(&mut k);
    k
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mos_estimates_are_sane() {
        let (_, clean) = estimate_mos(CodecKind::Pcmu, 120.0, 0.0);
        let (_, lossy) = estimate_mos(CodecKind::Pcmu, 120.0, 5.0);
        let (_, awful) = estimate_mos(CodecKind::Pcmu, 400.0, 20.0);
        assert!(clean > 4.2 && clean <= 4.5, "{clean}");
        assert!(lossy < clean && lossy > 3.0, "{lossy}");
        assert!(awful < 2.6, "{awful}");
    }
}
