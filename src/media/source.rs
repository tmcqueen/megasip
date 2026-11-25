//! `--source`: what a window sends instead of the microphone — a sound file (looped) or a
//! generated test signal.

use super::FRAME;
use std::sync::Arc;

/// 0 dBm0 sine peak in 16-bit linear PCM (G.711's digital milliwatt, about -3.17 dBFS).
pub const MILLI_PEAK: f32 = 22_653.0;
/// Noise level: -20 dBFS RMS.
const NOISE_RMS: f32 = 3_276.8;
const MAX_FILE_SECS: u32 = 600;

#[derive(Clone, Debug)]
pub enum Source {
    File {
        path: String,
        samples: Arc<Vec<i16>>,
    },
    Tone(f32),
    Pink,
    White,
}

impl PartialEq for Source {
    fn eq(&self, o: &Self) -> bool {
        match (self, o) {
            (Source::File { path: a, .. }, Source::File { path: b, .. }) => a == b,
            (Source::Tone(a), Source::Tone(b)) => a == b,
            (Source::Pink, Source::Pink) | (Source::White, Source::White) => true,
            _ => false,
        }
    }
}

impl Source {
    /// `milli`, `pink`, `white`, `tone:<hz>`, or a path to a .wav / .ogg / .mp3 file.
    pub fn parse(arg: &str) -> Result<Source, String> {
        match arg {
            "milli" => return Ok(Source::Tone(1004.0)),
            "pink" => return Ok(Source::Pink),
            "white" => return Ok(Source::White),
            _ => {}
        }
        if let Some(hz) = arg.strip_prefix("tone:") {
            return match hz.trim().parse::<f32>() {
                Ok(f) if (20.0..=20_000.0).contains(&f) => Ok(Source::Tone(f)),
                _ => Err(format!("tone:{hz}: the frequency must be 20 to 20000 Hz")),
            };
        }
        let lower = arg.to_ascii_lowercase();
        if ![".wav", ".ogg", ".oga", ".mp3"]
            .iter()
            .any(|e| lower.ends_with(e))
        {
            return Err(format!(
                "can't play {arg}: use a .wav, .ogg or .mp3 file, or milli, pink, white, tone:<hz>"
            ));
        }
        if !std::path::Path::new(arg).is_file() {
            return Err(format!("can't play {arg}: no such file"));
        }
        match super::dsp::load_sound_max(arg, MAX_FILE_SECS) {
            Some((samples, cut)) => {
                if cut {
                    eprintln!("megasip: {arg} is longer than 10 minutes; playing the first 10");
                }
                let path = std::fs::canonicalize(arg)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| arg.to_string());
                Ok(Source::File {
                    path,
                    samples: Arc::new(samples),
                })
            }
            None => Err(format!(
                "can't play {arg}: not a PCM WAV, Ogg Vorbis or MP3 file MegaSIP can decode"
            )),
        }
    }

    /// For the UI: "bar.wav", "1004 Hz tone", "Pink noise"…
    pub fn label(&self) -> String {
        match self {
            Source::File { path, .. } => std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone()),
            Source::Tone(f) => format!("{} Hz tone", fmt_hz(*f)),
            Source::Pink => "Pink noise".into(),
            Source::White => "White noise".into(),
        }
    }

    /// Longer form for tooltips: the full path of a file, otherwise the label.
    pub fn detail(&self) -> String {
        match self {
            Source::File { path, .. } => format!("{path} (loops)"),
            Source::Tone(f) if *f == 1004.0 => "1004 Hz test tone at 0 dBm0".into(),
            Source::Tone(f) => format!("{} Hz sine at 0 dBm0", fmt_hz(*f)),
            Source::Pink | Source::White => format!("{} at -20 dBFS", self.label()),
        }
    }
}

fn fmt_hz(f: f32) -> String {
    if f.fract() == 0.0 {
        format!("{f:.0}")
    } else {
        format!("{f}")
    }
}

/// Produces 20 ms frames from a `Source`.
pub struct SourcePlayer {
    src: Source,
    pos: u64,
    rng: u64,
    pink: [f32; 7],
}

impl SourcePlayer {
    pub fn new(src: Source) -> Self {
        SourcePlayer {
            src,
            pos: 0,
            rng: 0x9E37_79B9_7F4A_7C15,
            pink: [0.0; 7],
        }
    }

    /// Start from the beginning (a new call).
    pub fn restart(&mut self) {
        self.pos = 0;
    }

    /// Uniform in [-1, 1) (xorshift64*).
    fn uniform(&mut self) -> f32 {
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let v = self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40; // 24 bits
        v as f32 / (1u64 << 23) as f32 - 1.0
    }

    pub fn next_frame(&mut self, out: &mut [i16]) {
        debug_assert_eq!(out.len(), FRAME);
        match &self.src {
            Source::File { samples, .. } => {
                let s = samples.clone();
                let n = s.len() as u64;
                for o in out.iter_mut() {
                    *o = s[(self.pos % n) as usize];
                    self.pos += 1;
                }
            }
            Source::Tone(f) => {
                let w = 2.0 * std::f64::consts::PI * *f as f64 / 48_000.0;
                for o in out.iter_mut() {
                    *o = ((self.pos as f64 * w).sin() * MILLI_PEAK as f64) as i16;
                    self.pos += 1;
                }
            }
            Source::White => {
                // Uniform noise has RMS = peak / sqrt(3).
                let peak = NOISE_RMS * 3f32.sqrt();
                for o in out.iter_mut() {
                    *o = (self.uniform() * peak) as i16;
                }
            }
            Source::Pink => {
                for o in out.iter_mut() {
                    let w = self.uniform();
                    let b = &mut self.pink;
                    // Paul Kellet's refined pink filter.
                    b[0] = 0.99886 * b[0] + w * 0.0555179;
                    b[1] = 0.99332 * b[1] + w * 0.0750759;
                    b[2] = 0.96900 * b[2] + w * 0.153_852;
                    b[3] = 0.86650 * b[3] + w * 0.3104856;
                    b[4] = 0.55000 * b[4] + w * 0.5329522;
                    b[5] = -0.7616 * b[5] - w * 0.0168980;
                    let p = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + w * 0.5362;
                    b[6] = w * 0.115926;
                    // Scaled by the filter output RMS for uniform input (measured; see the test).
                    *o = super::dsp::clamp16(p * (NOISE_RMS / PINK_RMS));
                }
            }
        }
    }
}

/// RMS of the Kellet filter's output for uniform [-1, 1) input.
const PINK_RMS: f32 = 1.766;

#[cfg(test)]
mod tests {
    use super::*;

    fn rms_dbfs(src: Source, secs: usize) -> (f32, i16) {
        let mut p = SourcePlayer::new(src);
        let mut f = [0i16; FRAME];
        let (mut sum, mut peak) = (0f64, 0i16);
        for _ in 0..secs * 50 {
            p.next_frame(&mut f);
            for &s in &f {
                sum += (s as f64).powi(2);
                peak = peak.max(s.saturating_abs());
            }
        }
        let rms = (sum / (secs * 48_000) as f64).sqrt();
        ((20.0 * (rms / 32768.0).log10()) as f32, peak)
    }

    #[test]
    fn parse_keywords() {
        assert_eq!(Source::parse("milli").unwrap(), Source::Tone(1004.0));
        assert_eq!(Source::parse("tone:440").unwrap(), Source::Tone(440.0));
        assert_eq!(Source::parse("pink").unwrap(), Source::Pink);
        assert_eq!(Source::parse("white").unwrap(), Source::White);
        assert!(Source::parse("tone:5").is_err());
        assert!(Source::parse("/nonexistent/x.wav").is_err());
        assert!(Source::parse("notes.txt").is_err());
        assert_eq!(Source::parse("milli").unwrap().label(), "1004 Hz tone");
    }

    #[test]
    fn levels() {
        let (_, peak) = rms_dbfs(Source::Tone(1004.0), 1);
        assert!(
            (peak as f32 - MILLI_PEAK).abs() <= 50.0,
            "milli peak {peak}"
        );
        let (white, _) = rms_dbfs(Source::White, 10);
        assert!((white + 20.0).abs() < 1.0, "white {white} dBFS");
        let (pink, _) = rms_dbfs(Source::Pink, 10);
        assert!((pink + 20.0).abs() < 1.0, "pink {pink} dBFS");
    }

    #[test]
    fn file_loops() {
        let samples: Vec<i16> = (0..100).collect();
        let mut p = SourcePlayer::new(Source::File {
            path: "x.wav".into(),
            samples: Arc::new(samples),
        });
        let mut f = [0i16; FRAME];
        p.next_frame(&mut f);
        assert_eq!(&f[..3], &[0, 1, 2]);
        assert_eq!(f[100], 0);
        assert_eq!(f[959], 59);
        p.restart();
        p.next_frame(&mut f);
        assert_eq!(f[0], 0);
    }
}
