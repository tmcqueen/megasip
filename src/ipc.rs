//! Single instance: a second `megasip` (e.g. from a tel:/sip: link) hands its request
//! to the running one over a Unix socket in $XDG_RUNTIME_DIR.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;

static RX: Mutex<Option<Receiver<String>>> = Mutex::new(None);

fn path() -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
    let suffix = std::env::var("MEGASIP_INSTANCE").map(|s| format!("-{s}")).unwrap_or_default();
    dir.join(format!("megasip{suffix}.sock"))
}

/// Send `line` to a running instance. Returns true if one took it.
pub fn send_to_running(line: &str) -> bool {
    match UnixStream::connect(path()) {
        Ok(mut s) => s.write_all(format!("{line}\n").as_bytes()).is_ok(),
        Err(_) => false,
    }
}

/// Become the running instance.
pub fn listen() {
    let p = path();
    let _ = std::fs::remove_file(&p);
    let Ok(l) = UnixListener::bind(&p) else { return };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    }
    let (tx, rx) = channel();
    *RX.lock().unwrap() = Some(rx);
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(s).lines().map_while(Result::ok).take(16) {
                    if line.len() < 2048 && tx.send(line).is_err() {
                        break;
                    }
                }
            });
        }
    });
}

pub fn take_receiver() -> Option<Receiver<String>> {
    RX.lock().unwrap().take()
}

pub fn cleanup() {
    let _ = std::fs::remove_file(path());
}
