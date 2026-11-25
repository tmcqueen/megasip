//! Settings and accounts, stored in `~/.config/megasip/settings.json`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub label: String,
    pub enabled: bool,
    pub server: String,
    pub proxy: String,
    pub username: String,
    pub domain: String,
    pub auth_id: String,
    pub password: String,
    pub display_name: String,
    pub voicemail_number: String,
    pub dialing_prefix: String,
    /// "" | "privacy" (Privacy: id) — MicroSIP's "Hide Caller ID".
    pub hide_cid: bool,
    /// "auto" | "udp" | "tcp" | "tls"
    pub transport: String,
    /// "off" | "optional" | "mandatory"
    pub srtp: String,
    pub public_addr: String,
    pub register_refresh: u32,
    pub keep_alive: u32,
    pub allow_rewrite: bool,
    pub disable_session_timer: bool,
}

impl Default for Account {
    fn default() -> Self {
        Account {
            id: String::new(),
            label: String::new(),
            enabled: true,
            server: String::new(),
            proxy: String::new(),
            username: String::new(),
            domain: String::new(),
            auth_id: String::new(),
            password: String::new(),
            display_name: String::new(),
            voicemail_number: String::new(),
            dialing_prefix: String::new(),
            hide_cid: false,
            transport: "auto".into(),
            srtp: "off".into(),
            public_addr: String::new(),
            register_refresh: 300,
            keep_alive: 15,
            allow_rewrite: true,
            disable_session_timer: false,
        }
    }
}

impl Account {
    /// Domain used in our AOR / From: explicit domain, else the server host.
    pub fn sip_domain(&self) -> String {
        if !self.domain.trim().is_empty() {
            self.domain.trim().to_string()
        } else {
            self.server.trim().to_string()
        }
    }

    pub fn title(&self) -> String {
        if !self.label.trim().is_empty() {
            self.label.trim().to_string()
        } else if !self.username.is_empty() {
            format!("{}@{}", self.username, self.sip_domain())
        } else {
            "New account".to_string()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub accounts: Vec<Account>,
    /// Account used for outgoing calls.
    pub default_account: String,

    // Audio
    pub speaker_device: String,
    pub microphone_device: String,
    pub ring_device: String,
    pub ringtone: String,
    pub ring_volume: u32,
    pub speaker_volume: u32,
    pub mic_volume: u32,
    pub echo_cancellation: bool,
    /// Enabled codecs in priority order: "opus", "g722", "pcmu", "pcma".
    pub codecs: Vec<String>,
    pub local_dtmf: bool,

    // Calls
    /// "off" | "all" | "header" (answer when the server sends Call-Info answer-after / Alert-Info auto)
    pub auto_answer: String,
    pub auto_answer_delay: u32,
    pub dnd: bool,
    /// "off" | "always" | "busy" | "noanswer"
    pub forwarding: String,
    pub forwarding_number: String,
    pub forwarding_delay: u32,
    pub deny_incoming: String,
    pub call_waiting: bool,
    pub single_call_mode: bool,
    /// "auto" | "rfc2833" | "info" | "inband"
    pub dtmf_method: String,
    pub auto_hangup_secs: u32,

    // Network
    pub source_port: u16,
    pub rtp_port_min: u16,
    pub rtp_port_max: u16,
    pub stun: String,
    pub rport: bool,
    pub dns_srv: bool,
    pub user_agent: String,

    // Application
    pub start_minimized: bool,
    pub bring_to_front: bool,
    /// Set when the wallet couldn't be read or written (shown in Preferences); not saved.
    #[serde(skip_serializing, skip_deserializing)]
    pub secret_error: String,
    /// Old single switch; migrated to the log_* fields on load.
    #[serde(skip_serializing)]
    pub enable_log: bool,
    /// "off" | "error" | "warn" | "info" | "debug" (debug = every SIP message)
    pub log_sip_level: String,
    pub log_audio_level: String,
    pub log_to_files: bool,
    pub log_to_venator: bool,
    /// host:port of the Venator app (default 127.0.0.1:8362)
    pub venator_host: String,
    pub always_on_top: bool,
    pub last_number: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            accounts: Vec::new(),
            default_account: String::new(),
            speaker_device: String::new(),
            microphone_device: String::new(),
            ring_device: String::new(),
            ringtone: String::new(),
            ring_volume: 100,
            speaker_volume: 100,
            mic_volume: 100,
            echo_cancellation: true,
            codecs: vec!["opus".into(), "g722".into(), "pcmu".into(), "pcma".into()],
            local_dtmf: true,
            auto_answer: "off".into(),
            auto_answer_delay: 0,
            dnd: false,
            forwarding: "off".into(),
            forwarding_number: String::new(),
            forwarding_delay: 20,
            deny_incoming: String::new(),
            call_waiting: true,
            single_call_mode: false,
            dtmf_method: "auto".into(),
            auto_hangup_secs: 0,
            source_port: 0,
            rtp_port_min: 10000,
            rtp_port_max: 20000,
            stun: String::new(),
            rport: true,
            dns_srv: true,
            user_agent: String::new(),
            start_minimized: false,
            bring_to_front: true,
            secret_error: String::new(),
            enable_log: false,
            log_sip_level: "info".into(),
            log_audio_level: "warn".into(),
            log_to_files: false,
            log_to_venator: false,
            venator_host: "127.0.0.1:8362".into(),
            always_on_top: false,
            last_number: String::new(),
        }
    }
}

pub fn config_dir() -> PathBuf {
    if let Ok(d) = std::env::var("MEGASIP_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("megasip")
}

pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("MEGASIP_DATA_DIR") {
        return PathBuf::from(d);
    }
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("megasip")
}

impl Settings {
    pub fn path() -> PathBuf {
        config_dir().join("settings.json")
    }

    pub fn load() -> Settings {
        let mut s: Settings = std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        s.normalize();
        if !crate::secrets::disabled() {
            s.load_passwords();
        }
        s
    }

    /// Fill passwords from the wallet, moving any still in settings.json (older versions
    /// stored them there) into the wallet and out of the file.
    fn load_passwords(&mut self) {
        let mut migrated = false;
        let mut failed: Option<String> = None;
        for a in &mut self.accounts {
            if !a.password.is_empty() {
                match crate::secrets::store(&a.id, &a.title(), &a.password) {
                    Ok(()) => {
                        remember_stored(&a.id, &a.password);
                        migrated = true;
                    }
                    Err(e) => failed = Some(e),
                }
                continue;
            }
            match crate::secrets::load(&a.id) {
                Ok(Some(p)) => {
                    remember_stored(&a.id, &p);
                    a.password = p;
                }
                Ok(None) => {}
                Err(e) => failed = Some(e),
            }
        }
        if migrated && failed.is_none() {
            // Rewrite settings.json without the plaintext passwords.
            if let Err(e) = self.write_file() {
                tracing::error!("couldn't rewrite settings without passwords: {e}");
            } else {
                tracing::info!("moved SIP passwords from settings.json into the wallet");
            }
        }
        if let Some(e) = failed {
            tracing::error!("{e}");
            self.secret_error = e;
        }
    }

    pub fn normalize(&mut self) {
        if self.enable_log {
            self.enable_log = false;
            self.log_to_files = true;
            self.log_sip_level = "debug".into();
        }
        for a in &mut self.accounts {
            if a.id.is_empty() {
                a.id = crate::engine::msg::random_token(10);
            }
            if a.register_refresh == 0 {
                a.register_refresh = 300;
            }
        }
        if !self.accounts.iter().any(|a| a.id == self.default_account) {
            self.default_account = self.accounts.first().map(|a| a.id.clone()).unwrap_or_default();
        }
        if self.rtp_port_min == 0 || self.rtp_port_max <= self.rtp_port_min {
            self.rtp_port_min = 10000;
            self.rtp_port_max = 20000;
        }
        self.codecs.retain(|c| matches!(c.as_str(), "opus" | "g722" | "pcmu" | "pcma"));
        if self.codecs.is_empty() {
            self.codecs = vec!["pcmu".into(), "pcma".into()];
        }
    }

    /// Save settings. Passwords go to the wallet; settings.json never contains them.
    /// If the wallet can't take a password, nothing is written and the error is returned.
    pub fn save(&self) -> Result<(), String> {
        if !crate::secrets::disabled() {
            for a in &self.accounts {
                if already_stored(&a.id, &a.password) {
                    continue;
                }
                crate::secrets::store(&a.id, &a.title(), &a.password)?;
                remember_stored(&a.id, &a.password);
            }
            let keep: Vec<String> = self.accounts.iter().map(|a| a.id.clone()).collect();
            if let Err(e) = crate::secrets::prune(&keep) {
                tracing::warn!("couldn't tidy old wallet entries: {e}");
            }
        }
        self.write_file().map_err(|e| format!("Could not save settings: {e}"))
    }

    fn write_file(&self) -> std::io::Result<()> {
        let mut on_disk = self.clone();
        if !crate::secrets::disabled() {
            for a in &mut on_disk.accounts {
                a.password.clear();
            }
        }
        let dir = config_dir();
        std::fs::create_dir_all(&dir)?;
        let tmp = dir.join("settings.json.tmp");
        let _ = std::fs::remove_file(&tmp);
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // Owner-only from the moment it exists.
            opts.mode(0o600);
        }
        {
            use std::io::Write;
            let mut f = opts.open(&tmp)?;
            f.write_all(serde_json::to_string_pretty(&on_disk).unwrap_or_default().as_bytes())?;
            f.sync_all()?;
        }
        std::fs::rename(tmp, Self::path())
    }
}

/// Passwords known to be in the wallet already (so routine saves don't rewrite them).
static STORED: std::sync::Mutex<Option<std::collections::HashMap<String, String>>> = std::sync::Mutex::new(None);

fn remember_stored(id: &str, password: &str) {
    let mut g = STORED.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(Default::default).insert(id.to_string(), password.to_string());
}

fn already_stored(id: &str, password: &str) -> bool {
    let g = STORED.lock().unwrap_or_else(|e| e.into_inner());
    g.as_ref().and_then(|m| m.get(id)).is_some_and(|p| p == password)
}
