//! `megasip.db`: which windows are running and which account each one has active (the
//! account lock), plus call history. Every MegaSIP process opens it; SQLite does the locking.

use crate::store::HistoryEntry;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::path::Path;

pub use rusqlite::Result;

const MAX_HISTORY: i64 = 1000;

#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    pub pid: u32,
    pub account_id: Option<String>,
    pub started_at: i64,
}

/// Open (and create) `megasip.db` in the data directory.
pub fn open() -> Result<Connection> {
    let dir = crate::config::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    open_at(&dir.join("megasip.db"))
}

pub fn open_at(path: &Path) -> Result<Connection> {
    let c = Connection::open(path)?;
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    c.pragma_update(None, "journal_mode", "WAL")?;
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS instances (
             pid INTEGER PRIMARY KEY,
             proc_start INTEGER NOT NULL,
             account_id TEXT UNIQUE,
             started_at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS call_history (
             id TEXT PRIMARY KEY,
             account_id TEXT NOT NULL,
             number TEXT NOT NULL DEFAULT '',
             name TEXT NOT NULL DEFAULT '',
             kind TEXT NOT NULL DEFAULT '',
             time INTEGER NOT NULL DEFAULT 0,
             duration INTEGER NOT NULL DEFAULT 0,
             reason TEXT NOT NULL DEFAULT '');
         CREATE INDEX IF NOT EXISTS call_history_account_time ON call_history(account_id, time);",
    )?;
    Ok(c)
}

/// The kernel's start time of a process (field 22 of /proc/<pid>/stat), which tells a live
/// process apart from a later one that reused its PID.
pub fn proc_start(pid: u32) -> Option<i64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name (field 2) can contain spaces and parentheses; fields after it follow
    // the last ')'. Field 3 is the first one there, so field 22 is index 19.
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(19)?.parse().ok()
}

fn me() -> (u32, i64) {
    let pid = std::process::id();
    (pid, proc_start(pid).unwrap_or(0))
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn alive(pid: u32, start: i64) -> bool {
    proc_start(pid) == Some(start)
}

/// Delete rows of processes that are gone (or whose PID now belongs to someone else).
fn prune(c: &Connection) -> Result<()> {
    let rows: Vec<(u32, i64)> = c
        .prepare("SELECT pid, proc_start FROM instances")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_>>()?;
    for (pid, start) in rows {
        if !alive(pid, start) {
            // Only that process's row: the PID may have been reused and re-registered since.
            c.execute("DELETE FROM instances WHERE pid = ?1 AND proc_start = ?2", params![pid, start])?;
        }
    }
    Ok(())
}

pub fn register_self(c: &Connection) -> Result<()> {
    register_pid(c, me())
}

pub fn register_pid(c: &Connection, (pid, start): (u32, i64)) -> Result<()> {
    c.execute(
        "INSERT INTO instances (pid, proc_start, account_id, started_at) VALUES (?1, ?2, NULL, ?3)
         ON CONFLICT(pid) DO UPDATE SET proc_start = ?2, account_id = NULL, started_at = ?3",
        params![pid, start, now()],
    )?;
    Ok(())
}

pub fn unregister_self(c: &Connection) {
    let _ = c.execute("DELETE FROM instances WHERE pid = ?1", [std::process::id()]);
}

/// Running MegaSIP processes, after dropping stale rows.
pub fn live(c: &Connection) -> Result<Vec<Instance>> {
    prune(c)?;
    c.prepare("SELECT pid, account_id, started_at FROM instances ORDER BY started_at, pid")?
        .query_map([], |r| Ok(Instance { pid: r.get(0)?, account_id: r.get(1)?, started_at: r.get(2)? }))?
        .collect()
}

/// Make `account` this process's active account. `Ok(Err(pid))` when another live process
/// already has it.
pub fn acquire(c: &mut Connection, account: &str) -> Result<std::result::Result<(), u32>> {
    acquire_as(c, me(), account)
}

pub fn acquire_as(c: &mut Connection, (pid, start): (u32, i64), account: &str) -> Result<std::result::Result<(), u32>> {
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    prune(&tx)?;
    let holder: Option<u32> =
        tx.query_row("SELECT pid FROM instances WHERE account_id = ?1", [account], |r| r.get(0)).optional()?;
    if let Some(h) = holder.filter(|&h| h != pid) {
        return Ok(Err(h));
    }
    tx.execute(
        "INSERT INTO instances (pid, proc_start, account_id, started_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(pid) DO UPDATE SET proc_start = ?2, account_id = ?3",
        params![pid, start, account, now()],
    )?;
    tx.commit()?;
    Ok(Ok(()))
}

/// Give up this process's account (it stays listed as an idle window).
pub fn release(c: &Connection) {
    release_pid(c, std::process::id());
}

pub fn release_pid(c: &Connection, pid: u32) {
    let _ = c.execute("UPDATE instances SET account_id = NULL WHERE pid = ?1", [pid]);
}

/// The live process that has `account` active, if any.
pub fn holder(c: &Connection, account: &str) -> Result<Option<u32>> {
    prune(c)?;
    c.query_row("SELECT pid FROM instances WHERE account_id = ?1", [account], |r| r.get(0)).optional()
}

/// One account's calls, oldest first.
pub fn history(c: &Connection, account: &str) -> Result<Vec<HistoryEntry>> {
    c.prepare(
        "SELECT id, number, name, account_id, kind, time, duration, reason FROM call_history
         WHERE account_id = ?1 ORDER BY time, rowid",
    )?
    .query_map([account], |r| {
        Ok(HistoryEntry {
            id: r.get(0)?,
            number: r.get(1)?,
            name: r.get(2)?,
            account_id: r.get(3)?,
            kind: r.get(4)?,
            time: r.get(5)?,
            duration: r.get(6)?,
            reason: r.get(7)?,
        })
    })?
    .collect()
}

/// Record a call, keeping the account's newest 1000.
pub fn add_history(c: &Connection, e: &HistoryEntry) -> Result<()> {
    c.execute(
        "INSERT OR REPLACE INTO call_history (id, account_id, number, name, kind, time, duration, reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![e.id, e.account_id, e.number, e.name, e.kind, e.time, e.duration, e.reason],
    )?;
    c.execute(
        "DELETE FROM call_history WHERE account_id = ?1 AND rowid NOT IN
           (SELECT rowid FROM call_history WHERE account_id = ?1 ORDER BY time DESC, rowid DESC LIMIT ?2)",
        params![e.account_id, MAX_HISTORY],
    )?;
    Ok(())
}

pub fn delete_history(c: &Connection, id: &str) {
    let _ = c.execute("DELETE FROM call_history WHERE id = ?1", [id]);
}

pub fn clear_history(c: &Connection, account: &str) {
    let _ = c.execute("DELETE FROM call_history WHERE account_id = ?1", [account]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> (Connection, std::path::PathBuf) {
        let p = std::env::temp_dir().join(format!("megasip-test-{}-{}.db", std::process::id(), rand::random::<u64>()));
        (open_at(&p).unwrap(), p)
    }

    fn cleanup(p: std::path::PathBuf) {
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", p.display()));
        }
    }

    #[test]
    fn acquire_is_exclusive() {
        let (mut c, p) = temp_db();
        assert_eq!(acquire_as(&mut c, me(), "a").unwrap(), Ok(()));
        let init = (1, proc_start(1).expect("pid 1 is readable"));
        assert_eq!(acquire_as(&mut c, init, "a").unwrap(), Err(std::process::id()));
        // Asking again for your own account is fine.
        assert_eq!(acquire_as(&mut c, me(), "a").unwrap(), Ok(()));
        release_pid(&c, std::process::id());
        assert_eq!(acquire_as(&mut c, init, "a").unwrap(), Ok(()));
        cleanup(p);
    }

    #[test]
    fn stale_dead_pid_is_reclaimed() {
        let (mut c, p) = temp_db();
        c.execute("INSERT INTO instances VALUES (999999, 5, 'a', 0)", []).unwrap();
        assert!(live(&c).unwrap().iter().all(|i| i.pid != 999999));
        assert_eq!(acquire(&mut c, "a").unwrap(), Ok(()));
        cleanup(p);
    }

    #[test]
    fn stale_reused_pid_is_reclaimed() {
        let (mut c, p) = temp_db();
        c.execute("INSERT INTO instances VALUES (1, 0, 'a', 0)", []).unwrap();
        assert_eq!(acquire(&mut c, "a").unwrap(), Ok(()));
        assert_eq!(holder(&c, "a").unwrap(), Some(std::process::id()));
        cleanup(p);
    }

    #[test]
    fn history_scoped_and_trimmed() {
        let (c, p) = temp_db();
        let e = |acc: &str, i: i64| HistoryEntry { id: format!("{acc}{i}"), account_id: acc.into(), time: i, ..Default::default() };
        for i in 0..1005 {
            add_history(&c, &e("a", i)).unwrap();
        }
        add_history(&c, &e("b", 1)).unwrap();
        let h = history(&c, "a").unwrap();
        assert_eq!(h.len(), 1000);
        assert_eq!(h[0].time, 5);
        assert_eq!(history(&c, "b").unwrap().len(), 1);
        clear_history(&c, "a");
        assert!(history(&c, "a").unwrap().is_empty());
        assert_eq!(history(&c, "b").unwrap().len(), 1);
        cleanup(p);
    }
}
