//! Per-account icons, so that windows with different accounts can be told apart in the
//! taskbar and the tray.
//!
//! An account can pick a glyph and a colour. The icon is a rounded badge in that colour with
//! the glyph in white, written as an SVG file under `~/.local/share/megasip/icons/`.
//!
//! Plasma's task manager (and GNOME Shell) on Wayland take a window's icon from the
//! `.desktop` file that matches the window's app_id, not from the icon the window sets. So a
//! window whose account has an icon uses its own app_id, `org.megasip.MegaSIP.acct_<id>`, and
//! a hidden `.desktop` file of that name in `~/.local/share/applications/` points at the badge.
//! Accounts without an icon keep MegaSIP's own app_id and icon.

use crate::config::Account;
use std::path::{Path, PathBuf};

/// MegaSIP's own app_id (and `.desktop` file name).
pub const DEFAULT_APP_ID: &str = "org.megasip.MegaSIP";
/// The prefix of the app_ids (and `.desktop` files) made for accounts.
const ACCOUNT_APP_ID_PREFIX: &str = "org.megasip.MegaSIP.acct_";

/// The glyphs an account can pick: key (stored in settings.json) and the name shown.
pub const GLYPHS: &[(&str, &str)] = &[
    ("phone", "Phone"),
    ("headset", "Headset"),
    ("home", "Home"),
    ("work", "Briefcase"),
    ("office", "Building"),
    ("user", "Person"),
    ("star", "Star"),
    ("globe", "Globe"),
    ("letter", "First letter of the name"),
];

/// The badge colour when an account picks a glyph but no colour.
pub const DEFAULT_COLOR: &str = "#1d99f3";

pub fn is_glyph(key: &str) -> bool {
    GLYPHS.iter().any(|(k, _)| *k == key)
}

/// `#rgb` or `#rrggbb` (any case) as lowercase `#rrggbb`.
pub fn valid_color(s: &str) -> Option<String> {
    let hex = s.trim().strip_prefix('#')?;
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let full = match hex.len() {
        3 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => hex.to_string(),
        _ => return None,
    };
    Some(format!("#{}", full.to_ascii_lowercase()))
}

fn rgb(color: &str) -> (u8, u8, u8) {
    let c = valid_color(color).unwrap_or_else(|| DEFAULT_COLOR.to_string());
    let p = |i: usize| u8::from_str_radix(&c[i..i + 2], 16).unwrap_or(0);
    (p(1), p(3), p(5))
}

/// The letter the "letter" glyph shows: the first letter or digit of `name`, upper-cased.
pub fn initial(name: &str) -> char {
    name.chars()
        .find(|c| c.is_alphanumeric())
        .and_then(|c| c.to_uppercase().next())
        .unwrap_or('?')
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> String {
    format!("M{x} {y}h{w}v{h}h{}z", -w)
}

fn glyph_svg(glyph: &str, letter: char) -> String {
    match glyph {
        "headset" => concat!(
            r##"<path d="M17 38v-6a15 15 0 0 1 30 0v6" fill="none" stroke="#fff" stroke-width="4" stroke-linecap="round"/>"##,
            r##"<rect x="13" y="32" width="10" height="14" rx="3.5"/>"##,
            r##"<rect x="41" y="32" width="10" height="14" rx="3.5"/>"##,
            r##"<path d="M46 46c0 3.5-3 5.5-8 5.5h-3" fill="none" stroke="#fff" stroke-width="3" stroke-linecap="round"/>"##,
            r##"<rect x="29" y="49" width="7" height="5" rx="2.5"/>"##,
        )
        .to_string(),
        "home" => r##"<path d="M32 13L53 31h-6v19H37V38H27v12H17V31h-6z" stroke="#fff" stroke-width="1.5" stroke-linejoin="round"/>"##.to_string(),
        "work" => concat!(
            r##"<path d="M25 22v-3.5a3.5 3.5 0 0 1 3.5-3.5h7a3.5 3.5 0 0 1 3.5 3.5V22" fill="none" stroke="#fff" stroke-width="3.5"/>"##,
            r##"<path d="M16 22h32a4 4 0 0 1 4 4v8H12v-8a4 4 0 0 1 4-4z"/>"##,
            r##"<path d="M12 37h17v3h6v-3h17v9a4 4 0 0 1-4 4H16a4 4 0 0 1-4-4z"/>"##,
        )
        .to_string(),
        "office" => {
            let mut d = rect(18.0, 12.0, 28.0, 40.0);
            for y in [17.0, 24.0, 31.0] {
                for x in [23.0, 30.0, 37.0] {
                    d.push_str(&rect(x, y, 4.0, 4.0));
                }
            }
            d.push_str(&rect(29.0, 41.0, 6.0, 11.0));
            format!(r#"<path fill-rule="evenodd" d="{d}"/>"#)
        }
        "user" => concat!(
            r##"<circle cx="32" cy="23" r="9"/>"##,
            r##"<path d="M15 50c0-9 7.6-15 17-15s17 6 17 15z"/>"##,
        )
        .to_string(),
        "star" => {
            let points: Vec<String> = (0..10)
                .map(|i| {
                    let r = if i % 2 == 0 { 20.0 } else { 8.5 };
                    let a = (-90.0 + 36.0 * i as f64).to_radians();
                    format!("{:.1},{:.1}", 32.0 + r * a.cos(), 33.5 + r * a.sin())
                })
                .collect();
            format!(
                r##"<polygon points="{}" stroke="#fff" stroke-width="1.5" stroke-linejoin="round"/>"##,
                points.join(" ")
            )
        }
        "globe" => concat!(
            r##"<g fill="none" stroke="#fff" stroke-width="3">"##,
            r##"<circle cx="32" cy="32" r="18"/>"##,
            r##"<ellipse cx="32" cy="32" rx="8" ry="18"/>"##,
            r##"<path d="M14 32h36M16.4 23h31.2M16.4 41h31.2"/>"##,
            r##"</g>"##,
        )
        .to_string(),
        "letter" => format!(
            r##"<text x="32" y="43" font-family="sans-serif" font-size="32" font-weight="bold" text-anchor="middle">{}</text>"##,
            xml_escape(&letter.to_string())
        ),
        // "phone", and anything unknown.
        _ => r##"<path d="M23.6 15.2c1.3-.4 2.7.2 3.4 1.4l3.2 5.6c.7 1.2.5 2.7-.5 3.7l-2.6 2.6c1.9 4 5.1 7.2 9.1 9.1l2.6-2.6c1-1 2.5-1.2 3.7-.5l5.6 3.2c1.2.7 1.8 2.1 1.4 3.4l-1.1 3.9c-.5 1.6-1.9 2.7-3.6 2.7C29.4 47.7 16.3 34.6 16.3 19.3c0-1.7 1.1-3.1 2.7-3.6z"/>"##.to_string(),
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The badge: `glyph` in white on `color`. `dim` fades it, for the tray while the account
/// isn't registered.
pub fn svg(glyph: &str, color: &str, letter: char, dim: bool) -> String {
    let (r, g, b) = rgb(color);
    let dark = |v: u8| (v as f32 * 0.78).round() as u8;
    let top = format!("#{r:02x}{g:02x}{b:02x}");
    let bottom = format!("#{:02x}{:02x}{:02x}", dark(r), dark(g), dark(b));
    format!(
        concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64">"##,
            r##"<defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1">"##,
            r##"<stop offset="0" stop-color="{top}"/><stop offset="1" stop-color="{bottom}"/>"##,
            r##"</linearGradient></defs>"##,
            r##"<g opacity="{opacity}">"##,
            r##"<rect x="4" y="4" width="56" height="56" rx="14" fill="url(#g)"/>"##,
            r##"<g fill="#fff">{glyph}</g>"##,
            r##"</g></svg>"##,
            "\n"
        ),
        top = top,
        bottom = bottom,
        opacity = if dim { "0.45" } else { "1" },
        glyph = glyph_svg(glyph, letter),
    )
}

pub fn icons_dir() -> PathBuf {
    crate::config::data_dir().join("icons")
}

/// The icon's file name. The name says what is in it, so a changed icon is a new file (and
/// nothing shows a cached copy of the old one).
pub fn icon_file_name(glyph: &str, color: &str, letter: char, dim: bool) -> String {
    let color = valid_color(color).unwrap_or_else(|| DEFAULT_COLOR.to_string());
    let mut name = format!("{glyph}-{}", &color[1..]);
    if glyph == "letter" {
        name.push_str(&format!("-u{:x}", letter as u32));
    }
    if dim {
        name.push_str("-dim");
    }
    name.push_str(".svg");
    name
}

/// Write the icon (if it isn't there yet) and return its path.
pub fn write_icon(glyph: &str, color: &str, name: &str, dim: bool) -> std::io::Result<PathBuf> {
    let letter = initial(name);
    let dir = icons_dir();
    let path = dir.join(icon_file_name(glyph, color, letter, dim));
    if !path.exists() {
        std::fs::create_dir_all(&dir)?;
        let tmp = path.with_extension("svg.tmp");
        std::fs::write(&tmp, svg(glyph, color, letter, dim))?;
        std::fs::rename(&tmp, &path)?;
    }
    Ok(path)
}

/// The app_id (and `.desktop` file name) of windows with this account active, when it has
/// an icon.
pub fn app_id_for(account_id: &str) -> String {
    let safe: String = account_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("{ACCOUNT_APP_ID_PREFIX}{safe}")
}

/// One string value of a `.desktop` file: one line, backslashes escaped.
fn entry_value(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .replace('\\', "\\\\")
}

/// A program path for `Exec=`, quoted as the desktop entry spec asks if it needs it.
fn exec_arg(s: &str) -> String {
    let plain = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+@".contains(c));
    let quoted = if plain {
        s.to_string()
    } else {
        let mut q = String::from("\"");
        for c in s.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                q.push('\\');
            }
            q.push(c);
        }
        q.push('"');
        q
    };
    entry_value(&quoted).replace('%', "%%")
}

/// The hidden `.desktop` file for an account's windows.
pub fn desktop_entry(acc: &Account, exe: &str, icon: &Path) -> String {
    let exe = exec_arg(exe);
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={name}\n\
         GenericName=SIP Softphone\n\
         Comment=MegaSIP with {title} active\n\
         Exec={exe} --account {id}\n\
         Icon={icon}\n\
         Terminal=false\n\
         NoDisplay=true\n\
         StartupNotify=false\n\
         SingleMainWindow=true\n\
         X-MegaSIP-Account={id}\n\
         Actions=Hangup;Answer;\n\
         \n\
         [Desktop Action Answer]\n\
         Name=Answer Call\n\
         Exec={exe} --answer\n\
         \n\
         [Desktop Action Hangup]\n\
         Name=Hang Up\n\
         Exec={exe} --hangup\n",
        name = entry_value(&format!("MegaSIP ({})", acc.title())),
        title = entry_value(&acc.title()),
        id = exec_arg(&acc.id),
        icon = entry_value(&icon.to_string_lossy()),
    )
}

fn applications_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("applications")
}

/// Ask KDE to re-read the `.desktop` files now rather than whenever it notices, so the
/// taskbar picks up a new or changed icon right away. Not there (GNOME): nothing to do.
fn refresh_desktop_database() {
    if let Ok(mut child) = std::process::Command::new("kbuildsycoca6")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

fn write_desktop_file(app_id: &str, contents: &str) -> std::io::Result<()> {
    let dir = applications_dir();
    let path = dir.join(format!("{app_id}.desktop"));
    if std::fs::read_to_string(&path).ok().as_deref() == Some(contents) {
        return Ok(());
    }
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!(".{app_id}.desktop.tmp"));
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, &path)?;
    tracing::debug!("wrote {}", path.display());
    refresh_desktop_database();
    Ok(())
}

/// What a window shows itself as: its app_id and window icon (`None`: MegaSIP's own).
#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    pub app_id: String,
    pub icon: Option<PathBuf>,
}

impl Identity {
    pub fn icon_str(&self) -> String {
        self.icon
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// The identity of a window with `acc` active (or none), writing its icon and `.desktop`
/// file if they aren't there yet.
pub fn identity_for(acc: Option<&Account>) -> Identity {
    let plain = Identity {
        app_id: DEFAULT_APP_ID.to_string(),
        icon: None,
    };
    let Some(a) = acc.filter(|a| is_glyph(&a.icon)) else {
        return plain;
    };
    let icon = match write_icon(&a.icon, &a.icon_color, &a.title(), false) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("couldn't write the icon for {}: {e}", a.title());
            return plain;
        }
    };
    let app_id = app_id_for(&a.id);
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "megasip".into());
    match write_desktop_file(&app_id, &desktop_entry(a, &exe, &icon)) {
        Ok(()) => Identity {
            app_id,
            icon: Some(icon),
        },
        Err(e) => {
            // The window and tray still get the icon; the taskbar keeps MegaSIP's.
            tracing::warn!("couldn't write the .desktop file for {}: {e}", a.title());
            Identity {
                app_id: DEFAULT_APP_ID.to_string(),
                icon: Some(icon),
            }
        }
    }
}

/// Remove the `.desktop` files and icons that no account uses any more (the account was
/// deleted, or its icon changed or was turned off).
pub fn prune(accounts: &[Account]) {
    let keep_ids: Vec<String> = accounts
        .iter()
        .filter(|a| is_glyph(&a.icon))
        .map(|a| app_id_for(&a.id))
        .collect();
    let mut removed = false;
    if let Ok(dir) = std::fs::read_dir(applications_dir()) {
        for e in dir.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(app_id) = name.strip_suffix(".desktop") else {
                continue;
            };
            if app_id.starts_with(ACCOUNT_APP_ID_PREFIX) && !keep_ids.iter().any(|k| k == app_id) {
                removed |= std::fs::remove_file(e.path()).is_ok();
            }
        }
    }
    if removed {
        refresh_desktop_database();
    }
    let keep_icons: Vec<String> = accounts
        .iter()
        .filter(|a| is_glyph(&a.icon))
        .flat_map(|a| {
            let l = initial(&a.title());
            [false, true].map(|dim| icon_file_name(&a.icon, &a.icon_color, l, dim))
        })
        .collect();
    if let Ok(dir) = std::fs::read_dir(icons_dir()) {
        for e in dir.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".svg") && !keep_icons.contains(&name) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// `path` as a `file://` URL for QML.
pub fn file_url(path: &Path) -> String {
    let mut url = String::from("file://");
    for b in path.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            url.push(b as char);
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        assert_eq!(valid_color("#1D99F3").as_deref(), Some("#1d99f3"));
        assert_eq!(valid_color(" #abc ").as_deref(), Some("#aabbcc"));
        assert_eq!(valid_color("1d99f3"), None);
        assert_eq!(valid_color("#12345"), None);
        assert_eq!(valid_color("#gggggg"), None);
        assert_eq!(rgb("nonsense"), rgb(DEFAULT_COLOR));
    }

    #[test]
    fn initials() {
        assert_eq!(initial("telnyx - Tim"), 'T');
        assert_eq!(initial("  (work) line"), 'W');
        assert_eq!(initial("élan"), 'É');
        assert_eq!(initial("---"), '?');
    }

    #[test]
    fn every_glyph_renders_and_differs() {
        let all: Vec<String> = GLYPHS
            .iter()
            .map(|(k, _)| svg(k, "#da4453", 'T', false))
            .collect();
        for (s, (k, _)) in all.iter().zip(GLYPHS) {
            assert!(s.starts_with("<svg"), "{k}");
            assert!(s.contains("#da4453"), "{k}: badge colour");
            assert!(s.trim_end().ends_with("</svg>"), "{k}");
        }
        for i in 0..all.len() {
            for j in i + 1..all.len() {
                assert_ne!(all[i], all[j], "{} and {}", GLYPHS[i].0, GLYPHS[j].0);
            }
        }
        assert!(svg("letter", "#000", '<', false).contains(">&lt;</text>"));
        assert!(svg("phone", "#000", 'A', true).contains(r#"opacity="0.45""#));
    }

    #[test]
    fn file_names_follow_the_contents() {
        assert_eq!(
            icon_file_name("phone", "#1D99F3", 'T', false),
            "phone-1d99f3.svg"
        );
        assert_eq!(
            icon_file_name("phone", "", 'T', true),
            "phone-1d99f3-dim.svg"
        );
        assert_eq!(
            icon_file_name("letter", "#fff", 'T', false),
            "letter-ffffff-u54.svg"
        );
    }

    #[test]
    fn app_ids() {
        assert_eq!(app_id_for("ab12cd"), "org.megasip.MegaSIP.acct_ab12cd");
        assert_eq!(app_id_for("a-b.c"), "org.megasip.MegaSIP.acct_a_b_c");
    }

    #[test]
    fn desktop_file() {
        let a = Account {
            id: "abc123".into(),
            label: "Work\nline \\ 2".into(),
            icon: "work".into(),
            ..Default::default()
        };
        let d = desktop_entry(&a, "/opt/Mega SIP/megasip", Path::new("/h/.local/x.svg"));
        assert!(d.contains("Name=MegaSIP (Work line \\\\ 2)\n"), "{d}");
        assert!(
            d.contains("Exec=\"/opt/Mega SIP/megasip\" --account abc123\n"),
            "{d}"
        );
        assert!(
            d.contains("Exec=\"/opt/Mega SIP/megasip\" --answer\n"),
            "{d}"
        );
        assert!(d.contains("Icon=/h/.local/x.svg\n"));
        assert!(d.contains("NoDisplay=true\n"));
        assert!(
            !d.contains("MimeType"),
            "only the main entry handles sip: links"
        );
        assert_eq!(exec_arg("/usr/bin/megasip"), "/usr/bin/megasip");
        assert_eq!(exec_arg("/a/50%/$x"), "\"/a/50%%/\\\\$x\"");
    }

    #[test]
    fn no_icon_keeps_the_app_identity() {
        let a = Account::default();
        assert_eq!(
            identity_for(Some(&a)),
            Identity {
                app_id: DEFAULT_APP_ID.into(),
                icon: None
            }
        );
        assert_eq!(identity_for(None).app_id, DEFAULT_APP_ID);
    }

    #[test]
    fn urls() {
        assert_eq!(
            file_url(Path::new("/home/t m/#1.svg")),
            "file:///home/t%20m/%231.svg"
        );
    }
}
