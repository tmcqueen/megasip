//! Windows Credential Manager: one generic credential per account, named
//! `MegaSIP:<account id>`, holding the password as UTF-8. They show up under Control Panel,
//! Credential Manager, Windows Credentials.

use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND};
use windows_sys::Win32::Security::Credentials::{
    CredDeleteW, CredEnumerateW, CredFree, CredReadW, CredWriteW, CREDENTIALW,
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
};

const PREFIX: &str = "MegaSIP:";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn target(account_id: &str) -> Vec<u16> {
    wide(&format!("{PREFIX}{account_id}"))
}

/// A NUL-terminated UTF-16 string from Windows.
///
/// # Safety
/// `p` is null or points to a NUL-terminated UTF-16 string.
unsafe fn from_wide(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
}

fn last_error(what: &str) -> String {
    // SAFETY: no arguments; reads this thread's last error.
    let code = unsafe { GetLastError() };
    format!("{what} (Windows error {code})")
}

/// Read one account's password. Ok(None) when Credential Manager has none for it.
pub fn load(account_id: &str) -> Result<Option<String>, String> {
    let name = target(account_id);
    let mut cred: *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: `name` is NUL-terminated and outlives the call; on success `cred` points to
    // a buffer Windows allocated, read here and released with CredFree.
    unsafe {
        if CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut cred) == 0 {
            if GetLastError() == ERROR_NOT_FOUND {
                return Ok(None);
            }
            return Err(last_error(
                "Couldn't read the password from Credential Manager",
            ));
        }
        let c = &*cred;
        let blob = if c.CredentialBlob.is_null() {
            &[][..]
        } else {
            std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize)
        };
        let password = String::from_utf8_lossy(blob).into_owned();
        CredFree(cred as *const _);
        Ok(Some(password))
    }
}

/// Store (or replace) one account's password; an empty password removes it.
pub fn store(account_id: &str, label: &str, password: &str) -> Result<(), String> {
    if password.is_empty() {
        delete(account_id);
        return Ok(());
    }
    let mut name = target(account_id);
    let mut comment = wide(&format!("MegaSIP: {label}"));
    let mut user = wide(account_id);
    let mut blob = password.as_bytes().to_vec();
    let cred = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: name.as_mut_ptr(),
        Comment: comment.as_mut_ptr(),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: user.as_mut_ptr(),
        ..Default::default()
    };
    // SAFETY: every pointer in `cred` points into a local buffer that outlives the call.
    if unsafe { CredWriteW(&cred, 0) } == 0 {
        return Err(last_error(
            "Couldn't save the password to Credential Manager",
        ));
    }
    Ok(())
}

fn delete(account_id: &str) {
    let name = target(account_id);
    // SAFETY: `name` is NUL-terminated and outlives the call.
    unsafe {
        CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0);
    }
}

/// The account ids that have a MegaSIP credential.
fn stored_ids() -> Result<Vec<String>, String> {
    let filter = wide(&format!("{PREFIX}*"));
    let mut count = 0u32;
    let mut list: *mut *mut CREDENTIALW = std::ptr::null_mut();
    let mut ids = Vec::new();
    // SAFETY: on success `list` holds `count` credential pointers in one buffer Windows
    // allocated; they are read here and the buffer released with CredFree.
    unsafe {
        if CredEnumerateW(filter.as_ptr(), 0, &mut count, &mut list) == 0 {
            if GetLastError() == ERROR_NOT_FOUND {
                return Ok(ids);
            }
            return Err(last_error("Couldn't list Credential Manager entries"));
        }
        for i in 0..count as usize {
            let c = &**list.add(i);
            if c.Type != CRED_TYPE_GENERIC {
                continue;
            }
            if let Some(id) = from_wide(c.TargetName).strip_prefix(PREFIX) {
                ids.push(id.to_string());
            }
        }
        CredFree(list as *const _);
    }
    Ok(ids)
}

/// Remove MegaSIP credentials of accounts that no longer exist.
pub fn prune(keep: &[String]) -> Result<(), String> {
    for id in stored_ids()? {
        if !keep.contains(&id) {
            delete(&id);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_load_replace_prune() {
        let id = format!("test-{}-{}", std::process::id(), rand::random::<u32>());
        assert_eq!(load(&id).unwrap(), None);
        store(&id, "Test", "s3cret é").unwrap();
        assert_eq!(load(&id).unwrap().as_deref(), Some("s3cret é"));
        store(&id, "Test", "other").unwrap();
        assert_eq!(load(&id).unwrap().as_deref(), Some("other"));
        assert!(stored_ids().unwrap().contains(&id));
        // Pruning keeps what it's told to keep (here every other MegaSIP entry, so a
        // developer's real accounts survive the test) and removes the rest.
        let others: Vec<String> = stored_ids()
            .unwrap()
            .into_iter()
            .filter(|x| x != &id)
            .collect();
        let mut with_ours = others.clone();
        with_ours.push(id.clone());
        prune(&with_ours).unwrap();
        assert!(load(&id).unwrap().is_some());
        prune(&others).unwrap();
        assert_eq!(load(&id).unwrap(), None);
        // Storing an empty password removes it too.
        store(&id, "Test", "x").unwrap();
        store(&id, "Test", "").unwrap();
        assert_eq!(load(&id).unwrap(), None);
    }
}
