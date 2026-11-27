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
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    dir.join(format!("megasip-{pid}.sock"))
}

/// Send `line` to the window with this PID. Returns true if it took it.
pub fn send_to(pid: u32, line: &str) -> bool {
    send_at(&path_for(pid), &[line.to_string()])
}

fn send_at(path: &std::path::Path, lines: &[String]) -> bool {
    let mut msg = String::new();
    for l in lines {
        msg.push_str(l);
        msg.push('\n');
    }
    match UnixStream::connect(path) {
        Ok(mut s) => match s.write_all(msg.as_bytes()) {
            Ok(()) => {
                tracing::debug!("ipc: sent {lines:?} to {}", path.display());
                true
            }
            Err(e) => {
                tracing::debug!("ipc: connected to {} but write failed: {e}", path.display());
                false
            }
        },
        Err(e) => {
            tracing::debug!("ipc: can't connect to {}: {e}", path.display());
            false
        }
    }
}

/// Send `lines` to the window with this PID over one connection, so it handles them in
/// this order (separate connections are read by separate threads). True if it took them.
pub fn send_lines_to(pid: u32, lines: &[String]) -> bool {
    send_at(&path_for(pid), lines)
}

/// What to tell the window that already has `-a`'s account: switch to `source` (the
/// `--source` argument) if given, then dial `dial`, or just come to the front.
pub fn handoff_lines(dial: Option<&str>, source: Option<&str>) -> Vec<String> {
    match (dial, source) {
        (Some(d), Some(s)) => vec![format!("dial {d}\t{s}")],
        (Some(d), None) => vec![format!("dial {d}")],
        (None, Some(s)) => vec![format!("source {s}")],
        (None, None) => vec!["show".into()],
    }
}

/// The rest of a `dial` line: the target, and the source to call with if one came along
/// (after a tab, which a number or URI can't contain).
pub fn split_dial(rest: &str) -> (&str, Option<&str>) {
    match rest.split_once('\t') {
        Some((t, s)) if !s.trim().is_empty() => (t.trim(), Some(s.trim())),
        Some((t, _)) => (t.trim(), None),
        None => (rest.trim(), None),
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
    let l = match UnixListener::bind(&p) {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("ipc: can't listen on {}: {e}", p.display());
            return;
        }
    };
    tracing::debug!("ipc: listening on {}", p.display());
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    }
    let (tx, rx) = channel();
    *RX.lock().unwrap() = Some(rx);
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            tracing::debug!("ipc: connection accepted");
            let tx = tx.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(s).lines().map_while(Result::ok).take(16) {
                    if line.len() >= 4096 {
                        tracing::debug!("ipc: dropped a {}-byte line", line.len());
                        continue;
                    }
                    tracing::debug!("ipc: received {line:?}");
                    if tx.send(line).is_err() {
                        tracing::debug!("ipc: nobody is reading requests (window not set up?)");
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
    use super::*;

    #[test]
    fn handoff_carries_the_number_and_the_source() {
        assert_eq!(
            handoff_lines(Some("3346661338"), Some("pink")),
            ["dial 3346661338\tpink"]
        );
        assert_eq!(handoff_lines(Some("3346661338"), None), ["dial 3346661338"]);
        assert_eq!(handoff_lines(None, Some("pink")), ["source pink"]);
        assert_eq!(handoff_lines(None, None), ["show"]);
    }

    #[test]
    fn dial_lines_split_back() {
        for (d, s) in [
            ("3346661338", Some("pink")),
            ("sip:bob@example.com", Some("/home/t/My Sounds/a.wav")),
            ("tel:+15551234", None),
        ] {
            let line = handoff_lines(Some(d), s).remove(0);
            let rest = line.strip_prefix("dial ").unwrap();
            assert_eq!(split_dial(rest), (d, s));
        }
        assert_eq!(split_dial(" 123 "), ("123", None));
        assert_eq!(split_dial("123\t "), ("123", None));
    }

    #[test]
    fn lines_on_one_connection_arrive_in_order() {
        let dir = std::env::temp_dir().join(format!("megasip-ipc-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("s.sock");
        let _ = std::fs::remove_file(&p);
        let l = UnixListener::bind(&p).unwrap();
        let mut lines = vec!["show".to_string()];
        lines.extend(handoff_lines(Some("123"), Some("pink")));
        assert!(send_at(&p, &lines));
        let (c, _) = l.accept().unwrap();
        let got: Vec<String> = BufReader::new(c).lines().map_while(Result::ok).collect();
        assert_eq!(got, ["show", "dial 123\tpink"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn path_is_per_pid() {
        assert!(super::path_for(42)
            .to_string_lossy()
            .ends_with("megasip-42.sock"));
    }
}
