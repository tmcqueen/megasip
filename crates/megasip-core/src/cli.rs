//! `megasip --cli`: the engine without the UI. Commands on stdin, events as JSON on stdout.
//! For testing and scripting:
//!   dial <number> [account] | answer [id] | reject [id] | hangup [id] | hangupall
//!   hold <id> on|off | dtmf <id> <digits> | transfer <id> <target> | xfer <id> <other>
//!   conf on|off | mute on|off | levels | quit

use crate::config::Settings;
use crate::engine::{self, Command, Event};
use crate::media::hub::AudioHub;
use std::io::BufRead;

/// Run with `account_id` (already locked in `db`) as the only registered account.
pub fn run(settings: Settings, account_id: String, db: rusqlite::Connection) {
    let hub = AudioHub::start();
    if let Some(src) = crate::SOURCE.get() {
        hub.set_source(Some(src.clone()));
    }
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Event>();
    let cmd = engine::spawn(settings.for_engine(Some(&account_id)), hub.clone(), tx);
    let last_incoming = std::sync::Arc::new(std::sync::Mutex::new(0u64));
    let li = last_incoming.clone();
    std::thread::spawn(move || {
        while let Some(ev) = rx.blocking_recv() {
            let v = match ev {
                Event::Accounts(a) => serde_json::json!({"event": "accounts", "accounts": a}),
                Event::Call(c) => serde_json::json!({"event": "call", "call": c}),
                Event::Incoming(c) => {
                    *li.lock().unwrap() = c.id;
                    serde_json::json!({"event": "incoming", "call": c})
                }
                Event::Ended { info, missed } => {
                    if info.account_id == account_id {
                        if let Err(e) =
                            crate::db::add_history(&db, &crate::store::history_entry(&info, missed))
                        {
                            tracing::warn!("couldn't save call history: {e}");
                        }
                    }
                    serde_json::json!({"event": "ended", "call": info, "missed": missed})
                }
                Event::Dtmf(id, d) => {
                    serde_json::json!({"event": "dtmf", "id": id, "digit": d.to_string()})
                }
                Event::Error(e) => serde_json::json!({"event": "error", "text": e}),
            };
            println!("{v}");
        }
    });
    let stdin = std::io::stdin();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let p: Vec<&str> = line.split_whitespace().collect();
        let id = |i: usize| -> u64 {
            p.get(i)
                .and_then(|v| v.parse().ok())
                .unwrap_or(*last_incoming.lock().unwrap())
        };
        let on = |i: usize| p.get(i).is_some_and(|v| *v == "on" || *v == "1");
        let c = match p.first().copied() {
            Some("dial") => Command::Dial {
                number: p.get(1).unwrap_or(&"").to_string(),
                account: p.get(2).unwrap_or(&"").to_string(),
            },
            Some("answer") => Command::Answer(id(1)),
            Some("reject") => Command::Reject(id(1)),
            Some("hangup") => Command::Hangup(id(1)),
            Some("hangupall") => Command::HangupAll,
            Some("hold") => Command::Hold(id(1), on(2)),
            Some("dtmf") => {
                for d in p.get(2).unwrap_or(&"").chars() {
                    let _ = cmd.send(Command::Dtmf(id(1), d));
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
                continue;
            }
            Some("transfer") => Command::Transfer(id(1), p.get(2).unwrap_or(&"").to_string()),
            Some("xfer") => {
                Command::AttendedTransfer(id(1), p.get(2).and_then(|v| v.parse().ok()).unwrap_or(0))
            }
            Some("conf") => Command::Conference(on(1)),
            Some("mute") => Command::Mute(on(1)),
            Some("levels") => {
                let (m, s) = hub.levels();
                println!(
                    "{}",
                    serde_json::json!({"event": "levels", "mic": m, "speaker": s, "freq": hub.speaker_freq()})
                );
                continue;
            }
            Some("sleep") => {
                std::thread::sleep(std::time::Duration::from_millis(
                    p.get(1).and_then(|v| v.parse().ok()).unwrap_or(1000),
                ));
                continue;
            }
            Some("quit") => break,
            _ => continue,
        };
        let _ = cmd.send(c);
    }
    let _ = cmd.send(Command::Shutdown);
    std::thread::sleep(std::time::Duration::from_millis(800));
    if let Ok(c) = crate::db::open() {
        crate::db::unregister_self(&c);
    }
}
