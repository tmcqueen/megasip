//! MegaSIP: a voice-only port of MicroSIP for KDE Plasma.

mod appcpp;
mod bridge;
mod cli;
mod config;
mod db;
mod debug;
mod engine;
mod ipc;
mod logging;
mod media;
mod secrets;
mod store;

use std::sync::OnceLock;

/// The account this window activates at start (`-a`), as an account id.
pub static START_ACCOUNT: OnceLock<String> = OnceLock::new();
/// A number to dial once the window is up (and its account registered).
pub static START_DIAL: OnceLock<String> = OnceLock::new();
/// An error to show in the window once it is up.
pub static START_ERROR: OnceLock<String> = OnceLock::new();
/// `--source`: what to send instead of the microphone.
pub static SOURCE: OnceLock<media::source::Source> = OnceLock::new();

const HELP: &str = "MegaSIP — SIP softphone

Usage:
  megasip                       open a window with no account active
  megasip -a, --account <name>  open a window with that account active (its short name);
                                if another window already has it, that window comes to the front
                                and makes the call (with --source, it switches to that source)
  megasip --source <what>       send <what> instead of the microphone: a .wav, .ogg or .mp3 file,
                                milli (1004 Hz test tone), pink, white (noise) or tone:<hz>
  megasip --minimized           start in the system tray
  megasip <number|uri>          call a number, sip: / tel: / callto: link
  megasip --answer              answer the ringing call
  megasip --hangup              hang up all calls
  megasip --quit                quit every MegaSIP window
  megasip --cli -a <name>       no window: commands on stdin, events as JSON on stdout

Environment: MEGASIP_LOG=debug for SIP traces on stderr, MEGASIP_AUDIO_LOG=info for audio.";

fn fail(msg: &str) -> ! {
    eprintln!("megasip: {msg}");
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cli_mode = false;
    let mut minimized = false;
    let mut account: Option<String> = None;
    let mut source: Option<String> = None;
    let mut request: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |flag: &str| {
            it.next()
                .cloned()
                .unwrap_or_else(|| fail(&format!("{flag} needs a value")))
        };
        match a.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return;
            }
            "--cli" => cli_mode = true,
            "--minimized" | "-m" => minimized = true,
            "-a" | "--account" => account = Some(value("--account")),
            "--source" => source = Some(value("--source")),
            s if s.starts_with("--account=") => account = Some(s["--account=".len()..].to_string()),
            s if s.starts_with("--source=") => source = Some(s["--source=".len()..].to_string()),
            "--answer" => request = Some("answer".into()),
            "--hangup" => request = Some("hangup".into()),
            "--quit" => request = Some("quit".into()),
            s if !s.starts_with('-') => request = Some(format!("dial {s}")),
            s => fail(&format!("unknown option {s} (see --help)")),
        }
    }
    if let Some(s) = &source {
        match media::source::Source::parse(s) {
            Ok(src) => {
                let _ = SOURCE.set(src);
            }
            Err(e) => fail(&e),
        }
    }

    let settings = config::Settings::load();
    let db = db::open();

    if cli_mode {
        logging::init(&settings);
        let name = account.unwrap_or_else(|| fail("--cli needs -a <account>"));
        let acc = settings
            .find_account(&name)
            .cloned()
            .unwrap_or_else(|| fail(&format!("no account named \"{name}\"")));
        let mut c = db.unwrap_or_else(|e| fail(&format!("can't open the database: {e}")));
        match db::acquire(&mut c, &acc.id) {
            Ok(Ok(())) => {}
            Ok(Err(pid)) => fail(&format!("{} is already active (pid {pid})", acc.title())),
            Err(e) => fail(&format!("can't lock the account: {e}")),
        }
        logging::set_log_name(Some(&acc.short_name));
        cli::run(settings, acc.id, c);
        return;
    }

    // Hand the request to running windows where that's what it means.
    let me = std::process::id();
    let live: Vec<db::Instance> = match &db {
        Ok(c) => db::live(c)
            .unwrap_or_default()
            .into_iter()
            .filter(|i| i.pid != me)
            .collect(),
        Err(e) => {
            eprintln!("megasip: can't open the database ({e}); starting without the other windows");
            Vec::new()
        }
    };
    let dial = request
        .as_deref()
        .and_then(|r| r.strip_prefix("dial "))
        .map(str::to_string);
    if matches!(request.as_deref(), Some("answer" | "hangup" | "quit")) {
        for i in &live {
            ipc::send_to(i.pid, request.as_deref().unwrap_or_default());
        }
        return;
    }
    if let Some(name) = &account {
        match settings.find_account(name) {
            None => {
                let _ = START_ERROR.set(format!("No account named \"{name}\""));
            }
            Some(acc) => {
                if let Some(holder) = live
                    .iter()
                    .find(|i| i.account_id.as_deref() == Some(acc.id.as_str()))
                {
                    // That window places the call (and switches to --source first).
                    let src = SOURCE.get().map(|s| s.arg());
                    let lines = ipc::handoff_lines(dial.as_deref(), src.as_deref());
                    if !ipc::send_lines_to(holder.pid, &lines) {
                        fail(&format!(
                            "{} is active in another MegaSIP (pid {}) that isn't taking calls from the command line (a --cli session?)",
                            acc.title(),
                            holder.pid
                        ));
                    }
                    return;
                }
                let _ = START_ACCOUNT.set(acc.id.clone());
            }
        }
    } else if source.is_none() {
        if let Some(d) = &dial {
            let target = live
                .iter()
                .find(|i| i.account_id.as_deref() == Some(settings.default_account.as_str()))
                .or_else(|| live.iter().find(|i| i.account_id.is_some()))
                .or_else(|| live.first());
            if let Some(t) = target {
                if ipc::send_to(t.pid, &format!("dial {d}")) {
                    return;
                }
            }
        } else if let Some(idle) = live.iter().find(|i| i.account_id.is_none()) {
            if ipc::send_to(idle.pid, "show") {
                return;
            }
        }
    }
    if let Some(d) = dial {
        let _ = START_DIAL.set(d);
    }
    drop(db);

    logging::init(&settings);
    ipc::listen();
    // SIGTERM/SIGINT (logout, kill): hang up and un-register like MicroSIP does on exit.
    let _ = ctrlc::set_handler(|| {
        if !ipc::send_self("quit") {
            std::process::exit(0);
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
        std::process::exit(0);
    });
    let code =
        appcpp::ffi::run_qt_app("org.megasip.MegaSIP", minimized || settings.start_minimized);
    ipc::cleanup();
    std::process::exit(code);
}
