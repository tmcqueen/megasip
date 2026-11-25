//! MegaSIP: a voice-only port of MicroSIP for KDE Plasma.

mod appcpp;
mod bridge;
mod cli;
mod config;
mod debug;
mod engine;
mod ipc;
mod logging;
mod media;
mod store;

const HELP: &str = "MegaSIP — SIP softphone

Usage:
  megasip                 start (or show the running window)
  megasip --minimized     start in the system tray
  megasip <number|uri>    call a number, sip: / tel: / callto: link
  megasip --answer        answer the ringing call
  megasip --hangup        hang up all calls
  megasip --quit          quit the running instance

Environment: MEGASIP_LOG=debug for SIP traces on stderr, MEGASIP_AUDIO_LOG=info for audio.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--cli") {
        logging::init(&config::Settings::load());
        cli::run();
        return;
    }
    let mut minimized = false;
    let mut request: Option<String> = None;
    for a in &args {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return;
            }
            "--minimized" | "-m" => minimized = true,
            "--answer" => request = Some("answer".into()),
            "--hangup" => request = Some("hangup".into()),
            "--quit" => request = Some("quit".into()),
            s if !s.starts_with('-') => request = Some(format!("dial {s}")),
            _ => {}
        }
    }
    if ipc::send_to_running(request.as_deref().unwrap_or("show")) {
        return;
    }
    if request.as_deref() == Some("quit") {
        return;
    }

    let settings = config::Settings::load();
    logging::init(&settings);

    ipc::listen();
    // SIGTERM/SIGINT (logout, kill): hang up and un-register like MicroSIP does on exit.
    let _ = ctrlc::set_handler(|| {
        if !ipc::send_to_running("quit") {
            std::process::exit(0);
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
        std::process::exit(0);
    });
    // Queue the request (e.g. a tel: link that started us) for once the UI is up.
    if let Some(r) = request {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            ipc::send_to_running(&r);
        });
    }
    let code = appcpp::ffi::run_qt_app("org.megasip.MegaSIP", minimized || settings.start_minimized);
    ipc::cleanup();
    std::process::exit(code);
}
