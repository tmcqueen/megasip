//! The audio hub: a 20 ms mixer thread that owns the sound devices, mixes every call
//! with the microphone (and with each other for conferences), runs echo cancellation,
//! and plays the ringtone on the ring device.

use super::dsp::{self, Tone, ToneGen};
use super::{Stream, FRAME};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ring {
    Off,
    /// Full ringtone on the ring device.
    Ringtone,
    /// Short beeps in the speaker (call waiting).
    Waiting,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioConfig {
    pub speaker: String,
    pub microphone: String,
    pub ring_device: String,
    pub ringtone: String,
    pub speaker_volume: u32,
    pub mic_volume: u32,
    pub ring_volume: u32,
    pub echo_cancellation: bool,
}

/// How many ended calls' final media figures to keep.
const MAX_ENDED: usize = 64;

struct State {
    streams: Vec<Arc<Stream>>,
    conference: bool,
    mute: bool,
    tone: Option<ToneGen>,
    local: VecDeque<i16>,
    ring: Ring,
    cfg: AudioConfig,
    cfg_version: u64,
    ringtone: Arc<Vec<i16>>,
    /// Mic level 0..1 for the UI meter.
    mic_level: f32,
    spk_level: f32,
    errors: Vec<String>,
    /// Testing: no sound devices; the "microphone" is silent unless there is a source
    /// (MEGASIP_NULL_AUDIO).
    null_audio: bool,
    /// `--source`: sent instead of the microphone.
    source: Option<super::source::Source>,
    /// Dominant frequency of what we play (zero crossings), for tests.
    spk_freq: f32,
    last_stats: Option<super::StreamStats>,
    /// Final figures of recently ended streams, newest last (for quality reports).
    ended: VecDeque<super::StreamStats>,
}

pub struct AudioHub {
    st: Arc<Mutex<State>>,
}

type Buf = Arc<Mutex<VecDeque<i16>>>;

struct RingPlayer {
    samples: Arc<Vec<i16>>,
    pos: usize,
    gain: f32,
}

struct Devices {
    speaker: Option<cpal::Stream>,
    mic: Option<cpal::Stream>,
    ring: Option<cpal::Stream>,
    spk_buf: Buf,
    mic_buf: Buf,
    ring_state: Arc<Mutex<RingPlayer>>,
    idle_since: Option<Instant>,
    cfg_version: u64,
}

fn host() -> cpal::Host {
    #[cfg(target_os = "linux")]
    {
        if let Ok(h) = cpal::host_from_id(cpal::HostId::PulseAudio) {
            return h;
        }
    }
    cpal::default_host()
}

fn find_device(h: &cpal::Host, name: &str, input: bool) -> Option<cpal::Device> {
    if !name.is_empty() {
        let devs = if input {
            h.input_devices().ok()
        } else {
            h.output_devices().ok()
        };
        if let Some(devs) = devs {
            for d in devs {
                if d.to_string() == name {
                    return Some(d);
                }
            }
        }
        tracing::warn!("audio device {name:?} not found; using the default");
    }
    if input {
        h.default_input_device()
    } else {
        h.default_output_device()
    }
}

/// Names of the playback and capture devices, for the settings page.
pub fn list_devices() -> (Vec<String>, Vec<String>) {
    // One long-lived connection for listing: opening and dropping a PulseAudio client
    // each time makes the pulseaudio crate log a spurious "Client disconnected" error.
    static LIST_HOST: std::sync::OnceLock<Mutex<cpal::Host>> = std::sync::OnceLock::new();
    let h = LIST_HOST
        .get_or_init(|| Mutex::new(host()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let outs = h
        .output_devices()
        .map(|d| d.map(|d| d.to_string()).collect())
        .unwrap_or_default();
    let ins = h
        .input_devices()
        .map(|d| d.map(|d| d.to_string()).collect())
        .unwrap_or_default();
    (outs, ins)
}

fn stream_cfg() -> cpal::StreamConfig {
    cpal::StreamConfig {
        channels: 1,
        sample_rate: 48_000,
        buffer_size: cpal::BufferSize::Fixed(480),
    }
}

fn open_output(
    dev: &cpal::Device,
    mut pull: impl FnMut(&mut [f32]) + Send + 'static,
) -> Result<cpal::Stream, String> {
    let mut cfg = stream_cfg();
    let pull = Arc::new(Mutex::new(move |o: &mut [f32]| pull(o)));
    for attempt in 0..2 {
        let p = pull.clone();
        let res = dev.build_output_stream::<f32, _, _>(
            cfg,
            move |out: &mut [f32], _| (p.lock().unwrap_or_else(|e| e.into_inner()))(out),
            |e| tracing::warn!("output stream error: {e}"),
            None,
        );
        match res {
            Ok(s) => {
                s.play().map_err(|e| e.to_string())?;
                return Ok(s);
            }
            Err(e) if attempt == 0 => {
                tracing::debug!("output open with fixed buffer failed ({e}); retrying");
                cfg.buffer_size = cpal::BufferSize::Default;
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("cannot open output".into())
}

fn open_input(dev: &cpal::Device, buf: Buf) -> Result<cpal::Stream, String> {
    let mut cfg = stream_cfg();
    for attempt in 0..2 {
        let b = buf.clone();
        let res = dev.build_input_stream::<f32, _, _>(
            cfg,
            move |data: &[f32], _| {
                let mut q = b.lock().unwrap_or_else(|e| e.into_inner());
                q.extend(data.iter().map(|&s| dsp::clamp16(s * 32767.0)));
                let max = 48 * 300;
                if q.len() > max {
                    let extra = q.len() - max;
                    q.drain(..extra);
                }
            },
            |e| tracing::warn!("input stream error: {e}"),
            None,
        );
        match res {
            Ok(s) => {
                s.play().map_err(|e| e.to_string())?;
                return Ok(s);
            }
            Err(e) if attempt == 0 => {
                tracing::debug!("input open with fixed buffer failed ({e}); retrying");
                cfg.buffer_size = cpal::BufferSize::Default;
            }
            Err(e) => {
                // WASAPI (Windows) opens capture only in the device's own format, and many
                // microphones are stereo or 44.1 kHz: open it that way and convert.
                tracing::debug!(
                    "input open as 48 kHz mono failed ({e}); trying the device's own format"
                );
                return open_input_native(dev, buf)
                    .map_err(|e2| format!("{e}; in the device's own format: {e2}"));
            }
        }
    }
    Err("cannot open input".into())
}

/// Turns captured audio in any channel count and rate into 48 kHz mono: averages the
/// channels, then resamples by linear interpolation (good enough for speech; this is only
/// the fallback when the device won't capture 48 kHz mono itself).
struct CaptureConverter {
    channels: usize,
    /// Input samples per output sample.
    step: f64,
    /// Where the next output sample falls, in input samples after `prev`.
    pos: f64,
    prev: f32,
}

impl CaptureConverter {
    fn new(channels: u16, rate: u32) -> Self {
        Self {
            channels: (channels as usize).max(1),
            step: rate.max(1) as f64 / 48_000.0,
            pos: 0.0,
            prev: 0.0,
        }
    }

    fn push(&mut self, data: &[f32], out: &mut VecDeque<i16>) {
        for frame in data.chunks(self.channels) {
            let s = frame.iter().sum::<f32>() / frame.len() as f32;
            while self.pos < 1.0 {
                let v = self.prev + (s - self.prev) * self.pos as f32;
                out.push_back(dsp::clamp16(v * 32767.0));
                self.pos += self.step;
            }
            self.pos -= 1.0;
            self.prev = s;
        }
    }
}

fn open_input_native(dev: &cpal::Device, buf: Buf) -> Result<cpal::Stream, String> {
    let sc = dev.default_input_config().map_err(|e| e.to_string())?;
    let cfg = sc.config();
    tracing::debug!(
        "input: {} ch, {} Hz, {:?}; converting to 48 kHz mono",
        cfg.channels,
        cfg.sample_rate,
        sc.sample_format()
    );
    let conv = Arc::new(Mutex::new(CaptureConverter::new(
        cfg.channels,
        cfg.sample_rate,
    )));
    let feed = move |data: &[f32]| {
        let mut q = buf.lock().unwrap_or_else(|e| e.into_inner());
        conv.lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(data, &mut q);
        let max = 48 * 300;
        if q.len() > max {
            let extra = q.len() - max;
            q.drain(..extra);
        }
    };
    let err = |e| tracing::warn!("input stream error: {e}");
    let s = match sc.sample_format() {
        cpal::SampleFormat::F32 => {
            dev.build_input_stream::<f32, _, _>(cfg, move |d: &[f32], _| feed(d), err, None)
        }
        cpal::SampleFormat::I16 => {
            let mut tmp = Vec::new();
            dev.build_input_stream::<i16, _, _>(
                cfg,
                move |d: &[i16], _| {
                    tmp.clear();
                    tmp.extend(d.iter().map(|&v| v as f32 / 32768.0));
                    feed(&tmp)
                },
                err,
                None,
            )
        }
        f => return Err(format!("unsupported sample format {f:?}")),
    }
    .map_err(|e| e.to_string())?;
    s.play().map_err(|e| e.to_string())?;
    Ok(s)
}

fn make_apm() -> sonora::AudioProcessing {
    use sonora::config::{EchoCanceller, HighPassFilter, NoiseSuppression};
    let config = sonora::Config {
        echo_canceller: Some(EchoCanceller::default()),
        noise_suppression: Some(NoiseSuppression::default()),
        high_pass_filter: Some(HighPassFilter::default()),
        ..Default::default()
    };
    sonora::AudioProcessing::builder()
        .config(config)
        .capture_config(sonora::StreamConfig::new(48_000, 1))
        .render_config(sonora::StreamConfig::new(48_000, 1))
        .build()
}

impl AudioHub {
    pub fn start() -> Arc<AudioHub> {
        let st = Arc::new(Mutex::new(State {
            streams: Vec::new(),
            conference: false,
            mute: false,
            tone: None,
            local: VecDeque::new(),
            ring: Ring::Off,
            cfg: AudioConfig {
                speaker_volume: 100,
                mic_volume: 100,
                ring_volume: 100,
                echo_cancellation: true,
                ..Default::default()
            },
            cfg_version: 0,
            ringtone: Arc::new(dsp::default_ringtone()),
            mic_level: 0.0,
            spk_level: 0.0,
            errors: Vec::new(),
            null_audio: std::env::var_os("MEGASIP_NULL_AUDIO").is_some(),
            source: None,
            spk_freq: 0.0,
            last_stats: None,
            ended: VecDeque::new(),
        }));
        let st2 = st.clone();
        std::thread::Builder::new()
            .name("megasip-audio".into())
            .spawn(move || mixer_thread(st2))
            .expect("audio thread");
        Arc::new(AudioHub { st })
    }

    pub fn configure(&self, cfg: AudioConfig) {
        let mut g = self.st.lock().unwrap_or_else(|e| e.into_inner());
        if g.cfg == cfg {
            return;
        }
        if g.cfg.ringtone != cfg.ringtone {
            g.ringtone = Arc::new(if cfg.ringtone.is_empty() {
                dsp::default_ringtone()
            } else {
                dsp::load_sound(&cfg.ringtone).unwrap_or_else(dsp::default_ringtone)
            });
        }
        let devices_changed = g.cfg.speaker != cfg.speaker
            || g.cfg.microphone != cfg.microphone
            || g.cfg.ring_device != cfg.ring_device
            || g.cfg.ringtone != cfg.ringtone;
        g.cfg = cfg;
        if devices_changed {
            g.cfg_version += 1;
        }
    }

    pub fn add_stream(&self, s: Arc<Stream>) {
        let mut g = self.st.lock().unwrap_or_else(|e| e.into_inner());
        g.streams.retain(|x| x.id != s.id);
        g.streams.push(s);
    }

    pub fn remove_stream(&self, id: u64) {
        let mut g = self.st.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = g.streams.iter().find(|x| x.id == id) {
            let mut last = s.stats();
            last.ended = true;
            g.ended.retain(|x| x.id != id);
            if g.ended.len() >= MAX_ENDED {
                g.ended.pop_front();
            }
            g.ended.push_back(last.clone());
            g.last_stats = Some(last);
        }
        g.streams.retain(|x| x.id != id);
    }

    /// Forget the last call's figures and start live calls' counters over.
    pub fn clear_stats(&self) {
        let streams = {
            let mut g = self.st.lock().unwrap_or_else(|e| e.into_inner());
            g.last_stats = None;
            g.streams.clone()
        };
        for s in streams {
            s.reset_stats();
        }
    }

    /// Live stats for every call with media, plus the last call that ended.
    pub fn stats(&self) -> (Vec<super::StreamStats>, Option<super::StreamStats>) {
        let (streams, last) = {
            let g = self.st.lock().unwrap_or_else(|e| e.into_inner());
            (g.streams.clone(), g.last_stats.clone())
        };
        (streams.iter().map(|s| s.stats()).collect(), last)
    }

    /// The final figures of call `id`'s media, if it ended recently.
    pub fn ended_stats(&self, id: u64) -> Option<super::StreamStats> {
        let g = self.st.lock().unwrap_or_else(|e| e.into_inner());
        g.ended.iter().rev().find(|x| x.id == id).cloned()
    }

    pub fn set_conference(&self, on: bool) {
        self.st.lock().unwrap_or_else(|e| e.into_inner()).conference = on;
    }

    /// Send `src` instead of the microphone (None: back to the microphone).
    pub fn set_source(&self, src: Option<super::source::Source>) {
        self.st.lock().unwrap_or_else(|e| e.into_inner()).source = src;
    }

    pub fn set_mute(&self, on: bool) {
        self.st.lock().unwrap_or_else(|e| e.into_inner()).mute = on;
    }

    pub fn set_tone(&self, tone: Option<Tone>) {
        let mut g = self.st.lock().unwrap_or_else(|e| e.into_inner());
        if g.tone.as_ref().map(|t| t.kind()) != tone {
            g.tone = tone.map(ToneGen::new);
        }
    }

    pub fn play_local_dtmf(&self, digit: char) {
        let mut g = self.st.lock().unwrap_or_else(|e| e.into_inner());
        let t = dsp::dtmf_tone(digit, 120);
        g.local.extend(t);
    }

    pub fn set_ring(&self, r: Ring) {
        self.st.lock().unwrap_or_else(|e| e.into_inner()).ring = r;
    }

    pub fn levels(&self) -> (f32, f32) {
        let g = self.st.lock().unwrap_or_else(|e| e.into_inner());
        (g.mic_level, g.spk_level)
    }

    pub fn speaker_freq(&self) -> f32 {
        self.st.lock().unwrap_or_else(|e| e.into_inner()).spk_freq
    }

    pub fn take_errors(&self) -> Vec<String> {
        std::mem::take(&mut self.st.lock().unwrap_or_else(|e| e.into_inner()).errors)
    }
}

fn level(v: &[i16]) -> f32 {
    let peak = v.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0) as f32 / 32768.0;
    // Perceptual-ish scale.
    (peak.sqrt()).min(1.0)
}

fn mixer_thread(st: Arc<Mutex<State>>) {
    let h = host();
    let mut dev = Devices {
        speaker: None,
        mic: None,
        ring: None,
        spk_buf: Arc::new(Mutex::new(VecDeque::new())),
        mic_buf: Arc::new(Mutex::new(VecDeque::new())),
        ring_state: Arc::new(Mutex::new(RingPlayer {
            samples: Arc::new(Vec::new()),
            pos: 0,
            gain: 1.0,
        })),
        idle_since: None,
        cfg_version: 0,
    };
    let mut apm: Option<sonora::AudioProcessing> = None;
    let mut next = Instant::now();
    let mut acc = vec![0i32; FRAME];
    let mut call_audio: Vec<Vec<i32>> = Vec::new();
    let null_audio = st.lock().unwrap_or_else(|e| e.into_inner()).null_audio;
    let mut player: Option<(super::source::Source, super::source::SourcePlayer)> = None;
    let mut had_streams = false;
    loop {
        next += Duration::from_millis(20);
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else if now - next > Duration::from_millis(200) {
            next = now;
        }

        let (streams, conference, mute, cfg, ring, ringtone, cfg_version, source) = {
            let g = st.lock().unwrap_or_else(|e| e.into_inner());
            (
                g.streams.clone(),
                g.conference,
                g.mute,
                g.cfg.clone(),
                g.ring,
                g.ringtone.clone(),
                g.cfg_version,
                g.source.clone(),
            )
        };
        if player.as_ref().map(|(s, _)| s) != source.as_ref() {
            player = source.map(|s| (s.clone(), super::source::SourcePlayer::new(s)));
        }
        // A source starts from the top with each call.
        if !streams.is_empty() && !had_streams {
            if let Some((_, p)) = player.as_mut() {
                p.restart();
            }
        }
        had_streams = !streams.is_empty();

        // Device changes: reopen everything.
        if cfg_version != dev.cfg_version {
            dev.speaker = None;
            dev.mic = None;
            dev.ring = None;
            dev.cfg_version = cfg_version;
        }

        // Ring device.
        let want_ring = ring == Ring::Ringtone;
        if want_ring && dev.ring.is_none() {
            {
                let mut rs = dev.ring_state.lock().unwrap_or_else(|e| e.into_inner());
                rs.samples = ringtone.clone();
                rs.pos = 0;
            }
            if let Some(d) = find_device(&h, &cfg.ring_device, false) {
                let rs = dev.ring_state.clone();
                match open_output(&d, move |out| {
                    let mut r = rs.lock().unwrap_or_else(|e| e.into_inner());
                    let n = r.samples.len();
                    for o in out.iter_mut() {
                        *o = if n == 0 {
                            0.0
                        } else {
                            let s = r.samples[r.pos % n] as f32 / 32768.0 * r.gain;
                            r.pos = (r.pos + 1) % n;
                            s
                        };
                    }
                }) {
                    Ok(s) => dev.ring = Some(s),
                    Err(e) => st
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .errors
                        .push(format!("Ring device: {e}")),
                }
            }
        } else if !want_ring && dev.ring.is_some() {
            dev.ring = None;
        }
        dev.ring_state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .gain = cfg.ring_volume as f32 / 100.0;

        let has_tone =
            st.lock().unwrap_or_else(|e| e.into_inner()).tone.is_some() || ring == Ring::Waiting;
        let has_local = !st
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .local
            .is_empty();
        let need = !streams.is_empty() || has_tone || has_local;
        let need_mic = !streams.is_empty() && player.is_none();

        if need {
            dev.idle_since = None;
        } else if dev.speaker.is_some() || dev.mic.is_some() {
            let since = *dev.idle_since.get_or_insert(now);
            if now - since > Duration::from_secs(2) {
                dev.speaker = None;
                dev.mic = None;
                apm = None;
                dev.spk_buf
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clear();
                dev.mic_buf
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clear();
            }
        }
        if !need && dev.speaker.is_none() {
            continue;
        }
        if null_audio {
            // fall through without devices
        } else if dev.speaker.is_none() {
            if let Some(d) = find_device(&h, &cfg.speaker, false) {
                let b = dev.spk_buf.clone();
                match open_output(&d, move |out| {
                    let mut q = b.lock().unwrap_or_else(|e| e.into_inner());
                    for o in out.iter_mut() {
                        *o = q.pop_front().map(|s| s as f32 / 32768.0).unwrap_or(0.0);
                    }
                }) {
                    Ok(s) => dev.speaker = Some(s),
                    Err(e) => st
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .errors
                        .push(format!("Speaker: {e}")),
                }
            }
        }
        // The microphone is only open while there is a call, so Plasma's mic indicator
        // tells the truth.
        if null_audio {
        } else if need_mic && dev.mic.is_none() {
            if let Some(d) = find_device(&h, &cfg.microphone, true) {
                dev.mic_buf
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clear();
                match open_input(&d, dev.mic_buf.clone()) {
                    Ok(s) => dev.mic = Some(s),
                    Err(e) => st
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .errors
                        .push(format!("Microphone: {e}")),
                }
            }
        } else if !need_mic && dev.mic.is_some() {
            dev.mic = None;
            dev.mic_buf
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
        }

        // 1. Received audio per call.
        call_audio.resize_with(streams.len(), || vec![0i32; FRAME]);
        for (i, s) in streams.iter().enumerate() {
            call_audio[i].iter_mut().for_each(|v| *v = 0);
            s.pull(&mut call_audio[i]);
        }

        // 2. Speaker mix.
        acc.iter_mut().for_each(|v| *v = 0);
        for (i, s) in streams.iter().enumerate() {
            if s.playing() {
                for (a, v) in acc.iter_mut().zip(&call_audio[i]) {
                    *a += *v;
                }
            }
        }
        {
            let mut g = st.lock().unwrap_or_else(|e| e.into_inner());
            if ring == Ring::Waiting {
                if !matches!(g.tone.as_ref().map(|t| t.kind()), Some(Tone::Waiting)) {
                    g.tone = Some(ToneGen::new(Tone::Waiting));
                }
            } else if matches!(g.tone.as_ref().map(|t| t.kind()), Some(Tone::Waiting)) {
                g.tone = None;
            }
            if let Some(t) = g.tone.as_mut() {
                t.fill(&mut acc);
            }
            for a in acc.iter_mut() {
                match g.local.pop_front() {
                    Some(s) => *a += s as i32,
                    None => break,
                }
            }
        }
        let sv = cfg.speaker_volume as f32 / 100.0;
        let spk: Vec<i16> = acc.iter().map(|&v| dsp::clamp16(v as f32 * sv)).collect();
        {
            let mut q = dev.spk_buf.lock().unwrap_or_else(|e| e.into_inner());
            q.extend(spk.iter().copied());
            let max = 48 * 160;
            if q.len() > max {
                let extra = q.len() - 48 * 60;
                q.drain(..extra);
            }
        }

        // 3. Microphone.
        let mut mic = vec![0i16; FRAME];
        {
            let mut q = dev.mic_buf.lock().unwrap_or_else(|e| e.into_inner());
            if q.len() > 48 * 100 {
                let extra = q.len() - 48 * 40;
                q.drain(..extra);
            }
            let n = FRAME.min(q.len());
            for (m, s) in mic.iter_mut().zip(q.drain(..n)) {
                *m = s;
            }
        }
        if let Some((_, p)) = player.as_mut() {
            p.next_frame(&mut mic);
        } else if null_audio {
            mic.fill(0);
        }
        if null_audio {
            dev.spk_buf
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
        }
        // Mic gain is for the microphone; a source keeps its calibrated level.
        let mv = cfg.mic_volume as f32 / 100.0;
        if (mv - 1.0).abs() > 0.01 && player.is_none() {
            for m in mic.iter_mut() {
                *m = dsp::clamp16(*m as f32 * mv);
            }
        }
        // A source has no room echo or background noise to remove.
        if cfg.echo_cancellation && !streams.is_empty() && player.is_none() {
            let a = apm.get_or_insert_with(make_apm);
            let mut tmp = [0i16; 480];
            for k in 0..2 {
                let r = &spk[k * 480..(k + 1) * 480];
                let _ = a.process_render_i16(r, &mut tmp);
                let c: Vec<i16> = mic[k * 480..(k + 1) * 480].to_vec();
                let _ = a.process_capture_i16(&c, &mut mic[k * 480..(k + 1) * 480]);
            }
        }
        {
            let mut g = st.lock().unwrap_or_else(|e| e.into_inner());
            g.mic_level = if mute { 0.0 } else { level(&mic) };
            g.spk_level = level(&spk);
            let zc = spk.windows(2).filter(|w| (w[0] < 0) != (w[1] < 0)).count();
            g.spk_freq = zc as f32 * 50.0 / 2.0;
        }
        if mute {
            mic.iter_mut().for_each(|m| *m = 0);
        }

        // 4. Send to every call; in a conference each call also hears the others.
        for (i, s) in streams.iter().enumerate() {
            if !s.sending() {
                s.push(&mic); // keeps timestamps moving; push() drops it
                continue;
            }
            if conference && streams.len() > 1 {
                let mut out: Vec<i32> = mic.iter().map(|&m| m as i32).collect();
                for (j, other) in streams.iter().enumerate() {
                    if j != i && other.playing() {
                        for (o, v) in out.iter_mut().zip(&call_audio[j]) {
                            *o += *v;
                        }
                    }
                }
                let frame: Vec<i16> = out.iter().map(|&v| dsp::clamp16(v as f32)).collect();
                s.push(&frame);
            } else {
                s.push(&mic);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CaptureConverter;
    use std::collections::VecDeque;

    #[test]
    fn capture_converter_downmixes_and_resamples() {
        // 48 kHz stereo: one output per frame, the average of the two channels.
        let mut c = CaptureConverter::new(2, 48_000);
        let mut out = VecDeque::new();
        c.push(&[0.5, 0.5, 0.25, -0.25, -0.5, -0.5], &mut out);
        assert_eq!(out.len(), 3);
        // 44.1 kHz mono: one second in gives (very nearly) one second out at 48 kHz.
        let mut c = CaptureConverter::new(1, 44_100);
        let mut out = VecDeque::new();
        for _ in 0..10 {
            c.push(&[0.1; 4_410], &mut out);
        }
        assert!((out.len() as i64 - 48_000).abs() <= 1, "{}", out.len());
        assert!(out.iter().skip(2).all(|&v| (v - 3277).abs() <= 1));
    }
}
