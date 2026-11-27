//! Talking to running MegaSIP windows: each process listens on its own endpoint, a Unix
//! socket `$XDG_RUNTIME_DIR/megasip-<pid>.sock` on Linux and a named pipe
//! `\\.\pipe\megasip-<pid>` on Windows; which PIDs are running (and with which account)
//! is in `megasip.db` (see `db`).

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;

static RX: Mutex<Option<Receiver<String>>> = Mutex::new(None);

#[cfg(unix)]
pub fn path_for(pid: u32) -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    dir.join(format!("megasip-{pid}.sock"))
}

#[cfg(windows)]
pub fn path_for(pid: u32) -> PathBuf {
    PathBuf::from(format!(r"\\.\pipe\megasip-{pid}"))
}

/// The MCP endpoint of the window with this PID (see `mcp::serve_window`).
#[cfg(unix)]
pub fn mcp_path_for(pid: u32) -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    dir.join(format!("megasip-mcp-{pid}.sock"))
}

#[cfg(windows)]
pub fn mcp_path_for(pid: u32) -> PathBuf {
    PathBuf::from(format!(r"\\.\pipe\megasip-mcp-{pid}"))
}

/// Connect to the endpoint at `path`.
#[cfg(unix)]
fn connect(path: &Path) -> std::io::Result<impl Write> {
    std::os::unix::net::UnixStream::connect(path)
}

#[cfg(windows)]
fn connect(path: &Path) -> std::io::Result<impl Write> {
    use interprocess::local_socket::{prelude::*, GenericFilePath, Stream};
    Stream::connect(path.to_fs_name::<GenericFilePath>()?)
}

/// Listen at `path` (owner-only on Unix); the connections, as they come.
#[cfg(unix)]
fn bind(path: &Path) -> std::io::Result<impl Iterator<Item = impl Read + Send> + Send> {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::remove_file(path);
    let l = std::os::unix::net::UnixListener::bind(path)?;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    Ok(std::iter::from_fn(move || Some(l.accept().map(|(s, _)| s))).flatten())
}

/// A named pipe that refuses remote clients (interprocess's default).
#[cfg(windows)]
fn bind(path: &Path) -> std::io::Result<impl Iterator<Item = impl Read + Send> + Send> {
    use interprocess::local_socket::{prelude::*, GenericFilePath, ListenerOptions};
    let l = ListenerOptions::new()
        .name(path.to_fs_name::<GenericFilePath>()?)
        .create_sync()?;
    Ok(std::iter::from_fn(move || Some(l.accept())).flatten())
}

/// Send `line` to the window with this PID. Returns true if it took it.
pub fn send_to(pid: u32, line: &str) -> bool {
    send_at(&path_for(pid), &[line.to_string()])
}

fn send_at(path: &Path, lines: &[String]) -> bool {
    let mut msg = String::new();
    for l in lines {
        msg.push_str(l);
        msg.push('\n');
    }
    match connect(path) {
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

/// Start listening on this process's endpoint.
pub fn listen() {
    let p = path_for(std::process::id());
    let incoming = match bind(&p) {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("ipc: can't listen on {}: {e}", p.display());
            return;
        }
    };
    tracing::debug!("ipc: listening on {}", p.display());
    let (tx, rx) = channel();
    *RX.lock().unwrap() = Some(rx);
    std::thread::spawn(move || {
        for s in incoming {
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

/// Remove this process's socket file (a named pipe goes away with its process).
pub fn cleanup() {
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(path_for(std::process::id()));
        let _ = std::fs::remove_file(mcp_path_for(std::process::id()));
    }
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
        #[cfg(unix)]
        let (dir, p) = {
            let dir = std::env::temp_dir().join(format!("megasip-ipc-test-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let p = dir.join("s.sock");
            (Some(dir), p)
        };
        #[cfg(windows)]
        let (dir, p): (Option<PathBuf>, _) = (
            None,
            PathBuf::from(format!(r"\\.\pipe\megasip-ipc-test-{}", std::process::id())),
        );
        let mut incoming = bind(&p).unwrap();
        let mut lines = vec!["show".to_string()];
        lines.extend(handoff_lines(Some("123"), Some("pink")));
        let sender = {
            let p = p.clone();
            let lines = lines.clone();
            std::thread::spawn(move || send_at(&p, &lines))
        };
        let c = incoming.next().unwrap();
        let got: Vec<String> = BufReader::new(c).lines().map_while(Result::ok).collect();
        assert!(sender.join().unwrap());
        assert_eq!(got, ["show", "dial 123\tpink"]);
        if let Some(d) = dir {
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn path_is_per_pid() {
        assert!(super::path_for(42)
            .to_string_lossy()
            .ends_with(if cfg!(windows) {
                "megasip-42"
            } else {
                "megasip-42.sock"
            }));
    }
}
