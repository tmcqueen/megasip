//! Small DSP helpers: integer-ratio resampling, tones, and sound-file loading.

use std::f32::consts::PI;

/// Integer-ratio FIR resampler (e.g. 8k<->48k, 16k<->48k). Keeps state across frames.
pub struct Resampler {
    up: usize,
    down: usize,
    taps: Vec<f32>,
    hist: Vec<f32>,
    phase_len: usize,
}

impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32) -> Resampler {
        let (up, down) = if out_rate >= in_rate {
            ((out_rate / in_rate.max(1)).max(1) as usize, 1)
        } else {
            (1, (in_rate / out_rate.max(1)).max(1) as usize)
        };
        let factor = up.max(down);
        // Windowed-sinc low-pass at 0.9 * Nyquist of the lower rate.
        let n = if factor == 1 { 1 } else { 24 * factor + 1 };
        let cutoff = 0.5 / factor as f32 * 0.9;
        let mid = (n as f32 - 1.0) / 2.0;
        let mut taps: Vec<f32> = (0..n)
            .map(|i| {
                let x = i as f32 - mid;
                let sinc = if x.abs() < 1e-6 {
                    2.0 * cutoff
                } else {
                    (2.0 * PI * cutoff * x).sin() / (PI * x)
                };
                let w = 0.42 - 0.5 * (2.0 * PI * i as f32 / (n as f32 - 1.0).max(1.0)).cos()
                    + 0.08 * (4.0 * PI * i as f32 / (n as f32 - 1.0).max(1.0)).cos();
                sinc * if n == 1 { 1.0 } else { w }
            })
            .collect();
        let sum: f32 = taps.iter().sum();
        for t in &mut taps {
            *t /= sum;
        }
        let phase_len = n.div_ceil(up);
        let hist_len = if up > 1 { phase_len } else { n };
        Resampler {
            up,
            down,
            taps,
            hist: vec![0.0; hist_len],
            phase_len,
        }
    }

    pub fn is_passthrough(&self) -> bool {
        self.up == 1 && self.down == 1
    }

    pub fn process(&mut self, input: &[i16]) -> Vec<i16> {
        if self.is_passthrough() {
            return input.to_vec();
        }
        let mut out = Vec::with_capacity(input.len() * self.up / self.down + 1);
        if self.up > 1 {
            let l = self.up;
            for &s in input {
                self.hist.rotate_right(1);
                self.hist[0] = s as f32;
                for p in 0..l {
                    let mut acc = 0.0f32;
                    for k in 0..self.phase_len {
                        let ti = k * l + p;
                        if ti < self.taps.len() {
                            acc += self.taps[ti] * self.hist[k];
                        }
                    }
                    out.push(clamp16(acc * l as f32));
                }
            }
        } else {
            let m = self.down;
            let n = self.taps.len();
            // hist is a sliding window, newest at the end.
            let mut buf = std::mem::take(&mut self.hist);
            buf.extend(input.iter().map(|&s| s as f32));
            let start = buf.len() - input.len();
            let mut i = start;
            // Keep decimation phase consistent: we always consume whole frames that are a
            // multiple of `m` in practice (20 ms frames).
            while i < buf.len() {
                if i + 1 >= n {
                    let mut acc = 0.0f32;
                    for k in 0..n {
                        acc += self.taps[k] * buf[i - k];
                    }
                    out.push(clamp16(acc));
                } else {
                    out.push(0);
                }
                i += m;
            }
            let keep = n.min(buf.len());
            self.hist = buf[buf.len() - keep..].to_vec();
        }
        out
    }
}

pub fn clamp16(v: f32) -> i16 {
    v.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16
}

/// Ringback (440+480 Hz, 2 s on / 4 s off) at 48 kHz.
pub struct ToneGen {
    pos: u64,
    kind: Tone,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Ringback,
    Busy,
    /// Call-waiting beep heard during a call.
    Waiting,
}

impl ToneGen {
    pub fn new(kind: Tone) -> Self {
        ToneGen { pos: 0, kind }
    }
    pub fn kind(&self) -> Tone {
        self.kind
    }
    pub fn fill(&mut self, out: &mut [i32]) {
        let sr = 48_000u64;
        for o in out.iter_mut() {
            let t = self.pos as f32 / sr as f32;
            let cyc = self.pos % (sr * 6);
            let v = match self.kind {
                Tone::Ringback => {
                    if self.pos % (sr * 6) < sr * 2 {
                        ((2.0 * PI * 440.0 * t).sin() + (2.0 * PI * 480.0 * t).sin()) * 2500.0
                    } else {
                        0.0
                    }
                }
                Tone::Busy => {
                    if self.pos % sr < sr / 2 {
                        ((2.0 * PI * 480.0 * t).sin() + (2.0 * PI * 620.0 * t).sin()) * 2500.0
                    } else {
                        0.0
                    }
                }
                Tone::Waiting => {
                    if cyc < sr / 5 || (cyc > sr * 2 / 5 && cyc < sr * 3 / 5) {
                        (2.0 * PI * 440.0 * t).sin() * 2500.0
                    } else {
                        0.0
                    }
                }
            };
            *o += v as i32;
            self.pos += 1;
        }
    }
}

/// DTMF tone samples at 48 kHz for local feedback.
pub fn dtmf_tone(digit: char, ms: u32) -> Vec<i16> {
    let (lo, hi) = match digit.to_ascii_uppercase() {
        '1' => (697.0, 1209.0),
        '2' => (697.0, 1336.0),
        '3' => (697.0, 1477.0),
        'A' => (697.0, 1633.0),
        '4' => (770.0, 1209.0),
        '5' => (770.0, 1336.0),
        '6' => (770.0, 1477.0),
        'B' => (770.0, 1633.0),
        '7' => (852.0, 1209.0),
        '8' => (852.0, 1336.0),
        '9' => (852.0, 1477.0),
        'C' => (852.0, 1633.0),
        '*' => (941.0, 1209.0),
        '0' => (941.0, 1336.0),
        '#' => (941.0, 1477.0),
        'D' => (941.0, 1633.0),
        _ => return Vec::new(),
    };
    let n = 48 * ms as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / 48_000.0;
            let env = (i.min(n - i) as f32 / 240.0).min(1.0);
            (((2.0 * PI * lo * t).sin() + (2.0 * PI * hi * t).sin()) * 6000.0 * env) as i16
        })
        .collect()
}

/// Load a sound file (WAV PCM16 or Ogg Vorbis) as 48 kHz mono, at most a minute.
pub fn load_sound(path: &str) -> Option<Vec<i16>> {
    load_sound_max(path, 60).map(|(s, _)| s)
}

/// Load a WAV (PCM16), Ogg Vorbis or MP3 file as 48 kHz mono, at most `max_secs` long.
/// The flag says whether it was cut short.
pub fn load_sound_max(path: &str, max_secs: u32) -> Option<(Vec<i16>, bool)> {
    let lower = path.to_ascii_lowercase();
    let (rate, mono) = if lower.ends_with(".ogg") || lower.ends_with(".oga") {
        let f = std::fs::File::open(path).ok()?;
        let mut rdr = lewton::inside_ogg::OggStreamReader::new(f).ok()?;
        let ch = rdr.ident_hdr.audio_channels.max(1) as usize;
        let rate = rdr.ident_hdr.audio_sample_rate;
        let mut mono = Vec::new();
        while let Ok(Some(pkt)) = rdr.read_dec_packet_itl() {
            for frame in pkt.chunks(ch) {
                let s: i32 = frame.iter().map(|&v| v as i32).sum::<i32>() / ch as i32;
                mono.push(s as i16);
            }
            if mono.len() > rate as usize * max_secs as usize {
                break;
            }
        }
        (rate, mono)
    } else if lower.ends_with(".mp3") {
        decode_mp3(path, max_secs)?
    } else {
        let data = std::fs::read(path).ok()?;
        parse_wav(&data)?
    };
    if mono.is_empty() || rate == 0 {
        return None;
    }
    let limit = rate as usize * max_secs as usize;
    let cut = mono.len() > limit;
    let mono = if cut { &mono[..limit] } else { &mono[..] };
    Some((linear_resample(mono, rate, 48_000), cut))
}

fn decode_mp3(path: &str, max_secs: u32) -> Option<(u32, Vec<i16>)> {
    use symphonia::core::codecs::audio::AudioDecoderOptions;
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::formats::{FormatOptions, TrackType};
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;

    let file = Box::new(std::fs::File::open(path).ok()?);
    let mss = MediaSourceStream::new(file, Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .ok()?;
    let track = format.default_track(TrackType::Audio)?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(
            track.codec_params.as_ref()?.audio()?,
            &AudioDecoderOptions::default(),
        )
        .ok()?;
    let mut rate = 0u32;
    let mut mono: Vec<i16> = Vec::new();
    let mut buf: Vec<i16> = Vec::new();
    while let Ok(Some(packet)) = format.next_packet() {
        if packet.track_id != track_id {
            continue;
        }
        let Ok(audio) = decoder.decode(&packet) else {
            continue;
        };
        let spec = audio.spec();
        rate = spec.rate();
        let ch = spec.channels().count().max(1);
        buf.resize(audio.samples_interleaved(), 0);
        audio.copy_to_slice_interleaved(&mut buf);
        for frame in buf.chunks(ch) {
            mono.push((frame.iter().map(|&v| v as i32).sum::<i32>() / ch as i32) as i16);
        }
        if rate > 0 && mono.len() > rate as usize * max_secs as usize {
            break;
        }
    }
    Some((rate, mono))
}

fn parse_wav(d: &[u8]) -> Option<(u32, Vec<i16>)> {
    if d.len() < 44 || &d[0..4] != b"RIFF" || &d[8..12] != b"WAVE" {
        return None;
    }
    let mut i = 12;
    let (mut ch, mut rate, mut bits) = (1u16, 8000u32, 16u16);
    while i + 8 <= d.len() {
        let id = &d[i..i + 4];
        let len = u32::from_le_bytes(d[i + 4..i + 8].try_into().ok()?) as usize;
        let body = &d[i + 8..(i + 8 + len).min(d.len())];
        if id == b"fmt " && body.len() >= 16 {
            ch = u16::from_le_bytes([body[2], body[3]]).max(1);
            rate = u32::from_le_bytes(body[4..8].try_into().ok()?);
            bits = u16::from_le_bytes([body[14], body[15]]);
        } else if id == b"data" {
            let mut out = Vec::new();
            match bits {
                16 => {
                    for frame in body.chunks_exact(2 * ch as usize) {
                        let mut s = 0i32;
                        for c in 0..ch as usize {
                            s += i16::from_le_bytes([frame[2 * c], frame[2 * c + 1]]) as i32;
                        }
                        out.push((s / ch as i32) as i16);
                    }
                }
                8 => {
                    for frame in body.chunks_exact(ch as usize) {
                        let s: i32 = frame.iter().map(|&b| (b as i32 - 128) << 8).sum();
                        out.push((s / ch as i32) as i16);
                    }
                }
                _ => return None,
            }
            return Some((rate, out));
        }
        i += 8 + len + (len & 1);
    }
    None
}

pub fn linear_resample(input: &[i16], from: u32, to: u32) -> Vec<i16> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let n_out = (input.len() as u64 * to as u64 / from as u64) as usize;
    let step = from as f64 / to as f64;
    (0..n_out)
        .map(|i| {
            let x = i as f64 * step;
            let j = x as usize;
            let f = x - j as f64;
            let a = input[j.min(input.len() - 1)] as f64;
            let b = input[(j + 1).min(input.len() - 1)] as f64;
            (a + (b - a) * f) as i16
        })
        .collect()
}

/// The default ringtone: Plasma's incoming-call sound, or a synthesised ring.
pub fn default_ringtone() -> Vec<i16> {
    for p in [
        "/usr/share/sounds/ocean/stereo/phone-incoming-call.oga",
        "/usr/share/sounds/freedesktop/stereo/phone-incoming-call.oga",
        "/usr/share/sounds/oxygen/stereo/phone-incoming-call.ogg",
    ] {
        if let Some(s) = load_sound(p) {
            let mut s = s;
            // Pause between rings.
            s.extend(std::iter::repeat_n(0i16, 48_000));
            return s;
        }
    }
    let mut out = Vec::new();
    for burst in 0..2 {
        for i in 0..(48_000 * 4 / 10) {
            let t = i as f32 / 48_000.0;
            let am = if (i / 1200) % 2 == 0 { 1.0 } else { 0.4 };
            out.push(
                (((2.0 * PI * 440.0 * t).sin() + (2.0 * PI * 480.0 * t).sin()) * 5000.0 * am)
                    as i16,
            );
        }
        if burst == 0 {
            out.extend(std::iter::repeat_n(0i16, 48_000 / 5));
        }
    }
    out.extend(std::iter::repeat_n(0i16, 48_000 * 2));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_mp3_and_wav() {
        let dir = std::env::temp_dir().join(format!("megasip-snd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A 1 s, 16 kHz mono PCM WAV.
        let pcm = tone(16_000, 440.0, 16_000);
        let mut w = Vec::new();
        let data_len = (pcm.len() * 2) as u32;
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + data_len).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&16_000u32.to_le_bytes());
        w.extend_from_slice(&32_000u32.to_le_bytes());
        w.extend_from_slice(&2u16.to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&data_len.to_le_bytes());
        for s in &pcm {
            w.extend_from_slice(&s.to_le_bytes());
        }
        let wav = dir.join("t.wav");
        std::fs::write(&wav, w).unwrap();
        let mp3 = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/tone.mp3");
        for p in [wav.to_str().unwrap(), mp3] {
            let (s, cut) = load_sound_max(p, 600).unwrap_or_else(|| panic!("{p} didn't load"));
            assert!(!cut);
            assert!(
                (45_600..=50_400).contains(&s.len()),
                "{p}: {} samples",
                s.len()
            );
            assert!(
                s.iter().map(|v| v.saturating_abs()).max().unwrap() > 1000,
                "{p} is silent"
            );
        }
        let (s, cut) = load_sound_max(mp3, 0)
            .map(|(s, c)| (s.len(), c))
            .unwrap_or((0, true));
        assert!(cut || s == 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn tone(rate: u32, f: f32, n: usize) -> Vec<i16> {
        (0..n)
            .map(|i| ((2.0 * PI * f * i as f32 / rate as f32).sin() * 10000.0) as i16)
            .collect()
    }

    fn rms(v: &[i16]) -> f32 {
        (v.iter().map(|&s| (s as f32).powi(2)).sum::<f32>() / v.len() as f32).sqrt()
    }

    #[test]
    fn resample_round_trip_keeps_level() {
        let mut up = Resampler::new(8000, 48000);
        let mut down = Resampler::new(48000, 8000);
        let mut out = Vec::new();
        let input = tone(8000, 1000.0, 1600);
        for frame in input.chunks(160) {
            let hi = up.process(frame);
            assert_eq!(hi.len(), 960);
            let lo = down.process(&hi);
            assert_eq!(lo.len(), 160);
            out.extend(lo);
        }
        let r_in = rms(&input[800..]);
        let r_out = rms(&out[800..]);
        assert!((r_out / r_in - 1.0).abs() < 0.1, "{r_in} vs {r_out}");
    }

    #[test]
    fn dtmf_has_samples() {
        assert_eq!(dtmf_tone('5', 100).len(), 4800);
        assert!(dtmf_tone('x', 100).is_empty());
    }
}
