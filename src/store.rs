//! Contacts, as JSON in `~/.local/share/megasip/`, and the call history record type
//! (history itself lives in `megasip.db`, see `db`).

use crate::config::data_dir;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub number: String,
    pub name: String,
    pub account_id: String,
    /// "in" | "out" | "missed"
    pub kind: String,
    pub time: i64,
    pub duration: i64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Contact {
    pub id: String,
    pub name: String,
    pub number: String,
    pub favorite: bool,
    pub notes: String,
}

fn load<T: for<'a> Deserialize<'a> + Default>(name: &str) -> T {
    std::fs::read_to_string(data_dir().join(name))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save<T: Serialize>(name: &str, v: &T) {
    let dir = data_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let tmp = dir.join(format!("{name}.tmp"));
    if std::fs::write(&tmp, serde_json::to_string_pretty(v).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(tmp, dir.join(name));
    }
}

/// The history record for a call that just ended.
pub fn history_entry(info: &crate::engine::CallInfo, missed: bool) -> HistoryEntry {
    let now = chrono::Utc::now().timestamp();
    let kind = if missed {
        "missed"
    } else if info.direction == "in" {
        "in"
    } else {
        "out"
    };
    HistoryEntry {
        id: crate::engine::msg::random_token(10),
        number: info.number.clone(),
        name: info.name.clone(),
        account_id: info.account_id.clone(),
        kind: kind.into(),
        time: info.started,
        duration: info.connected_at.map(|t| (now - t).max(0)).unwrap_or(0),
        reason: info.reason.clone(),
    }
}

pub fn load_contacts() -> Vec<Contact> {
    let mut c: Vec<Contact> = load("contacts.json");
    for x in &mut c {
        if x.id.is_empty() {
            x.id = crate::engine::msg::random_token(10);
        }
    }
    c
}

pub fn save_contacts(c: &[Contact]) {
    save("contacts.json", &c.to_vec());
}

/// Import contacts from MicroSIP's Contacts.xml or a CSV (name,number).
pub fn import_contacts(path: &str) -> Vec<Contact> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    let mut out = Vec::new();
    if text.trim_start().starts_with('<') {
        // <contact name="..." number="..." ... />
        for tag in text.split("<contact").skip(1) {
            let end = tag.find('>').unwrap_or(tag.len());
            let attrs = &tag[..end];
            let get = |k: &str| -> String {
                let pat = format!("{k}=\"");
                attrs
                    .find(&pat)
                    .and_then(|i| {
                        let rest = &attrs[i + pat.len()..];
                        rest.find('"').map(|j| xml_unescape(&rest[..j]))
                    })
                    .unwrap_or_default()
            };
            let number = get("number");
            if number.is_empty() {
                continue;
            }
            out.push(Contact {
                id: crate::engine::msg::random_token(10),
                name: get("name"),
                number,
                favorite: get("presence") == "1" || get("starred") == "1",
                notes: get("info"),
            });
        }
    } else {
        for line in text.lines() {
            let cols: Vec<&str> = line.split([',', ';', '\t']).map(|c| c.trim().trim_matches('"')).collect();
            if cols.len() >= 2 && !cols[1].is_empty() && !cols[1].eq_ignore_ascii_case("number") {
                out.push(Contact {
                    id: crate::engine::msg::random_token(10),
                    name: cols[0].to_string(),
                    number: cols[1].to_string(),
                    ..Default::default()
                });
            }
        }
    }
    out
}

pub fn export_contacts_csv(path: &str, c: &[Contact]) -> std::io::Result<()> {
    let mut s = String::from("Name,Number\n");
    for x in c {
        s.push_str(&format!("\"{}\",\"{}\"\n", x.name.replace('"', "'"), x.number.replace('"', "'")));
    }
    std::fs::write(path, s)
}

fn xml_unescape(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_microsip_xml() {
        let dir = std::env::temp_dir().join(format!("megasip-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("Contacts.xml");
        std::fs::write(
            &p,
            r#"<?xml version="1.0"?><contacts><contact name="Bob &amp; Co" number="1002" presence="1"/><contact name="X" number=""/></contacts>"#,
        )
        .unwrap();
        let c = import_contacts(p.to_str().unwrap());
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].name, "Bob & Co");
        assert!(c[0].favorite);
        let _ = std::fs::remove_dir_all(dir);
    }
}
