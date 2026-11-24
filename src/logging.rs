//! Logging, split by subject:
//!
//! - **SIP** (and the rest of the app) → `~/.local/share/megasip/logs/sip.log`
//! - **Audio** (PulseAudio, cpal, echo cancellation, our media code) → `.../logs/audio.log`
//! - optionally both to the [Venator](https://crates.io/crates/venator-app) log viewer
//! - stderr: SIP at `MEGASIP_LOG` (default info), audio only warnings unless
//!   `MEGASIP_AUDIO_LOG` says otherwise.
//!
//! Levels and sinks can change at runtime from Preferences; the filters read atomics,
//! so nothing has to be rebuilt.

use crate::config::{data_dir, Settings};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use tracing::{Level, Metadata, Subscriber};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Context, Filter, Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{reload, Registry};

// 0 = off, 1 = error … 5 = trace
static SIP_LEVEL: AtomicU8 = AtomicU8::new(3);
static AUDIO_LEVEL: AtomicU8 = AtomicU8::new(2);
static STDERR_SIP: AtomicU8 = AtomicU8::new(3);
static STDERR_AUDIO: AtomicU8 = AtomicU8::new(2);
static FILES_ON: AtomicBool = AtomicBool::new(false);
static VENATOR_ON: AtomicBool = AtomicBool::new(false);

static SIP_FILE: FileSink = FileSink::new();
static AUDIO_FILE: FileSink = FileSink::new();

type VenatorHandle = reload::Handle<Option<venator::Venator>, Registry>;
static VENATOR: OnceLock<VenatorHandle> = OnceLock::new();
static VENATOR_HOST: Mutex<String> = Mutex::new(String::new());

const AUDIO_TARGETS: &[&str] = &["pulseaudio", "cpal", "alsa", "sonora", "lewton", "ropus", "megasip::media"];

pub fn is_audio(target: &str) -> bool {
    AUDIO_TARGETS
        .iter()
        .any(|p| target == *p || (target.starts_with(p) && target[p.len()..].starts_with("::")))
}

fn level_num(l: &Level) -> u8 {
    match *l {
        Level::ERROR => 1,
        Level::WARN => 2,
        Level::INFO => 3,
        Level::DEBUG => 4,
        Level::TRACE => 5,
    }
}

/// "off" | "error" | "warn" | "info" | "debug" | "trace"
pub fn parse_level(s: &str) -> u8 {
    match s.trim().to_ascii_lowercase().as_str() {
        "off" | "none" | "" => 0,
        "error" => 1,
        "warn" | "warning" => 2,
        "debug" => 4,
        "trace" => 5,
        _ => 3,
    }
}

#[derive(Clone, Copy)]
enum Sink {
    SipFile,
    AudioFile,
    Stderr,
    Venator,
}

struct CatFilter(Sink);

impl CatFilter {
    fn allows(&self, meta: &Metadata<'_>) -> bool {
        // Records bridged from the `log` crate share generic callsites with target "log";
        // their real target is only known per event (see event_enabled).
        if meta.target() == "log" {
            return true;
        }
        let audio = is_audio(meta.target());
        let lvl = level_num(meta.level());
        let max = match self.0 {
            Sink::SipFile if FILES_ON.load(Ordering::Relaxed) && !audio => SIP_LEVEL.load(Ordering::Relaxed),
            Sink::AudioFile if FILES_ON.load(Ordering::Relaxed) && audio => AUDIO_LEVEL.load(Ordering::Relaxed),
            // Venator's reconnect failures repeat every 5 s while it isn't running; files only.
            Sink::Stderr if meta.target().starts_with("venator") => 0,
            Sink::Stderr if audio => STDERR_AUDIO.load(Ordering::Relaxed),
            Sink::Stderr => STDERR_SIP.load(Ordering::Relaxed),
            // Venator's own connection messages must not loop back into it.
            Sink::Venator if VENATOR_ON.load(Ordering::Relaxed) && !meta.target().starts_with("venator") => {
                if audio {
                    AUDIO_LEVEL.load(Ordering::Relaxed)
                } else {
                    SIP_LEVEL.load(Ordering::Relaxed)
                }
            }
            _ => 0,
        };
        lvl <= max
    }
}

impl<S: Subscriber> Filter<S> for CatFilter {
    fn enabled(&self, meta: &Metadata<'_>, _: &Context<'_, S>) -> bool {
        self.allows(meta)
    }

    // Checked again per event, with the real target of records bridged from the `log`
    // crate (pulseaudio and cpal log through it).
    fn event_enabled(&self, event: &tracing::Event<'_>, _: &Context<'_, S>) -> bool {
        use tracing_log::NormalizeEvent;
        match event.normalized_metadata() {
            Some(real) => self.allows(&real),
            None => self.allows(event.metadata()),
        }
    }

    fn callsite_enabled(&self, _: &'static Metadata<'static>) -> tracing::subscriber::Interest {
        // Levels change at runtime: ask every time.
        tracing::subscriber::Interest::sometimes()
    }
}

/// A log file that can be opened and closed while the program runs.
struct FileSink {
    file: Mutex<Option<File>>,
}

impl FileSink {
    const fn new() -> Self {
        FileSink { file: Mutex::new(None) }
    }

    fn set_open(&self, path: Option<PathBuf>) {
        let mut g = self.file.lock().unwrap_or_else(|e| e.into_inner());
        match path {
            None => *g = None,
            Some(p) if g.is_none() => {
                if let Some(dir) = p.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                // Keep one previous log; start fresh when it grows past 20 MB.
                if std::fs::metadata(&p).map(|m| m.len() > 20 * 1024 * 1024).unwrap_or(false) {
                    let _ = std::fs::rename(&p, p.with_extension("log.old"));
                }
                *g = std::fs::OpenOptions::new().create(true).append(true).open(&p).ok();
            }
            Some(_) => {}
        }
    }
}

struct FileWriter(&'static FileSink);

struct FileGuard(std::sync::MutexGuard<'static, Option<File>>);

impl Write for FileGuard {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self.0.as_mut() {
            Some(f) => f.write(buf),
            None => Ok(buf.len()),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self.0.as_mut() {
            Some(f) => f.flush(),
            None => Ok(()),
        }
    }
}

impl<'a> MakeWriter<'a> for FileWriter {
    type Writer = FileGuard;
    fn make_writer(&'a self) -> FileGuard {
        FileGuard(self.0.file.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

pub fn log_dir() -> PathBuf {
    data_dir().join("logs")
}

fn timer() -> tracing_subscriber::fmt::time::ChronoLocal {
    tracing_subscriber::fmt::time::ChronoLocal::new("%Y-%m-%d %H:%M:%S%.3f".into())
}

/// Install the global subscriber. Call once, early.
pub fn init(settings: &Settings) {
    if let Ok(v) = std::env::var("MEGASIP_LOG") {
        STDERR_SIP.store(parse_level(&v), Ordering::Relaxed);
    }
    if let Ok(v) = std::env::var("MEGASIP_AUDIO_LOG") {
        STDERR_AUDIO.store(parse_level(&v), Ordering::Relaxed);
    }
    let (venator_layer, handle) = reload::Layer::new(None::<venator::Venator>);
    let _ = VENATOR.set(handle);

    let sip_file = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_timer(timer())
        .with_writer(FileWriter(&SIP_FILE))
        .with_filter(CatFilter(Sink::SipFile));
    let audio_file = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_timer(timer())
        .with_writer(FileWriter(&AUDIO_FILE))
        .with_filter(CatFilter(Sink::AudioFile));
    let stderr = tracing_subscriber::fmt::layer()
        .with_timer(timer())
        .with_writer(std::io::stderr)
        .with_filter(CatFilter(Sink::Stderr));

    // Venator goes last: when it can't connect it logs from inside the dispatch of the
    // event it was sending, which resets tracing's per-layer filter state for any layer
    // after it, so anything after it would receive events its filter had rejected.
    let layers: Vec<Box<dyn Layer<Registry> + Send + Sync>> = vec![
        sip_file.boxed(),
        audio_file.boxed(),
        stderr.boxed(),
        venator_layer.with_filter(CatFilter(Sink::Venator)).boxed(),
    ];
    let _ = tracing_subscriber::registry().with(layers).try_init();
    apply(settings);
}

/// Apply the logging part of the settings (levels, files, Venator).
pub fn apply(s: &Settings) {
    SIP_LEVEL.store(parse_level(&s.log_sip_level), Ordering::Relaxed);
    AUDIO_LEVEL.store(parse_level(&s.log_audio_level), Ordering::Relaxed);
    FILES_ON.store(s.log_to_files, Ordering::Relaxed);
    let dir = log_dir();
    SIP_FILE.set_open((s.log_to_files && parse_level(&s.log_sip_level) > 0).then(|| dir.join("sip.log")));
    AUDIO_FILE.set_open((s.log_to_files && parse_level(&s.log_audio_level) > 0).then(|| dir.join("audio.log")));

    let host = if s.venator_host.trim().is_empty() { "127.0.0.1:8362".to_string() } else { s.venator_host.trim().to_string() };
    if let Some(h) = VENATOR.get() {
        let mut cur = VENATOR_HOST.lock().unwrap_or_else(|e| e.into_inner());
        if s.log_to_venator {
            // (Re)connect only when turned on or the host changed.
            if !VENATOR_ON.load(Ordering::Relaxed) || *cur != host {
                let layer = venator::Venator::builder()
                    .with_host(host.clone())
                    .with_attribute("service", "megasip")
                    .with_attribute("version", env!("CARGO_PKG_VERSION"))
                    .build();
                let _ = h.reload(Some(layer));
                *cur = host;
            }
        } else if VENATOR_ON.load(Ordering::Relaxed) {
            let _ = h.reload(None);
            cur.clear();
        }
    }
    VENATOR_ON.store(s.log_to_venator, Ordering::Relaxed);
    tracing::callsite::rebuild_interest_cache();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories() {
        assert!(is_audio("pulseaudio::client::reactor"));
        assert!(is_audio("pulseaudio"));
        assert!(is_audio("megasip::media::hub"));
        assert!(is_audio("cpal::host::pulseaudio"));
        assert!(!is_audio("megasip::engine"));
        assert!(!is_audio("megasip::engine::transport"));
        assert!(!is_audio("pulseaudiox"));
        assert_eq!(parse_level("Debug"), 4);
        assert_eq!(parse_level("off"), 0);
    }
}
