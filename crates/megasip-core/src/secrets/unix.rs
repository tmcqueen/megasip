//! Secret Service (KWallet, GNOME Keyring) over D-Bus.

use secret_service::blocking::SecretService;
use secret_service::EncryptionType;
use std::collections::HashMap;

const APP: &str = "megasip";

fn attrs(account_id: &str) -> HashMap<&str, &str> {
    HashMap::from([("application", APP), ("account", account_id)])
}

fn connect() -> Result<SecretService<'static>, String> {
    SecretService::connect(EncryptionType::Dh)
        .map_err(|e| format!("Can't reach the wallet (Secret Service): {e}"))
}

/// Read one account's password. Ok(None) when the wallet has none for it.
pub fn load(account_id: &str) -> Result<Option<String>, String> {
    let ss = connect()?;
    let found = ss
        .search_items(attrs(account_id))
        .map_err(|e| e.to_string())?;
    let item = found
        .unlocked
        .into_iter()
        .next()
        .or_else(|| found.locked.into_iter().next());
    let Some(item) = item else { return Ok(None) };
    item.ensure_unlocked()
        .map_err(|e| format!("The wallet is locked: {e}"))?;
    let secret = item.get_secret().map_err(|e| e.to_string())?;
    Ok(Some(String::from_utf8_lossy(&secret).into_owned()))
}

/// Store (or replace) one account's password; an empty password removes it.
pub fn store(account_id: &str, label: &str, password: &str) -> Result<(), String> {
    let ss = connect()?;
    if password.is_empty() {
        return remove(&ss, account_id);
    }
    let coll = ss
        .get_default_collection()
        .map_err(|e| format!("No default wallet: {e}"))?;
    coll.ensure_unlocked()
        .map_err(|e| format!("The wallet is locked: {e}"))?;
    coll.create_item(
        &format!("MegaSIP: {label}"),
        attrs(account_id),
        password.as_bytes(),
        true,
        "text/plain",
    )
    .map(|_| ())
    .map_err(|e| format!("Couldn't save the password to the wallet: {e}"))
}

fn remove(ss: &SecretService<'_>, account_id: &str) -> Result<(), String> {
    let found = ss
        .search_items(attrs(account_id))
        .map_err(|e| e.to_string())?;
    for item in found.unlocked.iter().chain(found.locked.iter()) {
        let _ = item.delete();
    }
    Ok(())
}

/// Remove wallet entries for accounts that no longer exist.
pub fn prune(keep: &[String]) -> Result<(), String> {
    let ss = connect()?;
    let found = ss
        .search_items(HashMap::from([("application", APP)]))
        .map_err(|e| e.to_string())?;
    for item in found.unlocked.iter().chain(found.locked.iter()) {
        let id = item
            .get_attributes()
            .ok()
            .and_then(|a| a.get("account").cloned())
            .unwrap_or_default();
        if !keep.contains(&id) {
            let _ = item.delete();
        }
    }
    Ok(())
}
