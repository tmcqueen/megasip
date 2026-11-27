//! SIP passwords live in the desktop's secret store, never in settings.json: the
//! freedesktop Secret Service on Linux (KWallet on Plasma, GNOME Keyring on GNOME) and the
//! Credential Manager on Windows.
//!
//! Call these from ordinary threads (main / Qt), not from inside the SIP engine's tokio
//! runtime: the blocking Secret Service API runs its own executor.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::{load, prune, store};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{load, prune, store};

/// Testing only: keep passwords in settings.json (throwaway test accounts must not end up
/// in the user's wallet). Never set this for real accounts.
pub fn disabled() -> bool {
    std::env::var_os("MEGASIP_NO_KEYRING").is_some()
}
