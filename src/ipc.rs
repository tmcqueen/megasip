//! Talking to running MegaSIP windows: each process listens on its own Unix socket,
//! `$XDG_RUNTIME_DIR/megasip-<pid>.sock`; which PIDs are running (and with which account)
//! is in `megasip.db` (see `db`).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;

static RX: Mutex<Option<Receiver<String>>> = Mutex::new(None);

pub fn path_for(pid: u32) -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
    dir.join(format!("megasip-{pid}.sock"))
}

/// Send `line` to the window with this PID. Returns true if it took it.
pub fn send_to(pid: u32, line: &str) -> bool {
    match UnixStream::connect(path_for(pid)) {
        Ok(mut s) => s.write_all(format!("{line}\n").as_bytes()).is_ok(),
        Err(_) => false,
    }
}

/// Send `line` to this process's own window (signal handler, queued start-up requests).
pub fn send_self(line: &str) -> bool {
    send_to(std::process::id(), line)
}

/// Start listening on this process's socket.
pub fn listen() {
    let p = path_for(std::process::id());
    let _ = std::fs::remove_file(&p);
    let Ok(l) = UnixListener::bind(&p) else { return };
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
                    if line.len() < 4096 && tx.send(line).is_err() {
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
    let _ = std::fs::remove_file(path_for(std::process::id()));
}

#[cfg(test)]
mod tests {
    #[test]
    fn path_is_per_pid() {
        assert!(super::path_for(42).to_string_lossy().ends_with("megasip-42.sock"));
    }
}
