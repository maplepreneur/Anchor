//! Read and write XDG `.desktop` entries for managed web apps.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};

use crate::paths::{self, is_manager_tag, MANAGER_TAG};

#[derive(Debug, Clone)]
pub struct DesktopEntry {
    pub path: PathBuf,
    pub codename: String,
    pub name: String,
    pub url: String,
    pub browser: String,
    pub icon: String,
    pub exec: String,
    pub startup_wm_class: String,
}

/// Escape a value for a desktop-entry key (no newlines).
fn escape_value(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "")
}

pub fn write_desktop_file(
    codename: &str,
    name: &str,
    url: &str,
    browser_name: &str,
    icon: &Path,
    exec: &str,
    window_class: &str,
) -> Result<PathBuf> {
    let path = paths::desktop_path(codename)?;
    let content = format!(
        r#"[Desktop Entry]
Version=1.0
Type=Application
Name={name}
Comment=Web App
Exec={exec}
Terminal=false
Icon={icon}
StartupWMClass={class}
StartupNotify=true
Categories=Network;WebBrowser;
X-WebApp-Manager={manager}
X-WebApp-URL={url}
X-WebApp-Browser={browser}
X-WebApp-Isolated=true
"#,
        name = escape_value(name),
        exec = exec, // Exec is already carefully built
        icon = escape_value(&icon.display().to_string()),
        class = escape_value(window_class),
        manager = MANAGER_TAG,
        url = escape_value(url),
        browser = escape_value(browser_name),
    );

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, content).with_context(|| format!("write {}", path.display()))?;
    let mut perms = fs::metadata(&path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms)?;

    refresh_desktop_database();
    Ok(path)
}

/// Best-effort desktop database refresh (GNOME/Zorin pick up new apps).
pub fn refresh_desktop_database() {
    if let Ok(apps) = paths::applications_dir() {
        let _ = Command::new("update-desktop-database")
            .arg(apps)
            .status();
    }
}

fn parse_desktop_file(path: &Path) -> Result<DesktopEntry> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut name = None;
    let mut url = None;
    let mut browser = None;
    let mut icon = None;
    let mut exec = None;
    let mut manager = None;
    let mut startup_wm_class = None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            match k {
                "Name" => name = Some(v.to_string()),
                "Icon" => icon = Some(v.to_string()),
                "Exec" => exec = Some(v.to_string()),
                "StartupWMClass" => startup_wm_class = Some(v.to_string()),
                "X-WebApp-URL" => url = Some(v.to_string()),
                "X-WebApp-Browser" => browser = Some(v.to_string()),
                "X-WebApp-Manager" => manager = Some(v.to_string()),
                _ => {}
            }
        }
    }

    let Some(manager_tag) = manager.as_deref() else {
        return Err(anyhow!("missing X-WebApp-Manager"));
    };
    if !is_manager_tag(manager_tag) {
        return Err(anyhow!("not managed by Anchor (got {manager_tag})"));
    }

    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("bad filename"))?;
    let codename = file_name
        .strip_prefix(paths::DESKTOP_PREFIX)
        .and_then(|s| s.strip_suffix(".desktop"))
        .ok_or_else(|| anyhow!("unexpected desktop filename"))?
        .to_string();

    Ok(DesktopEntry {
        path: path.to_path_buf(),
        codename,
        name: name.ok_or_else(|| anyhow!("missing Name"))?,
        url: url.unwrap_or_default(),
        browser: browser.unwrap_or_default(),
        icon: icon.unwrap_or_default(),
        exec: exec.unwrap_or_default(),
        startup_wm_class: startup_wm_class.unwrap_or_default(),
    })
}

/// List all web apps created by this manager.
pub fn list_managed_apps() -> Result<Vec<DesktopEntry>> {
    let dir = paths::applications_dir()?;
    let mut apps = Vec::new();
    if !dir.is_dir() {
        return Ok(apps);
    }
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !name.starts_with(paths::DESKTOP_PREFIX) || !name.ends_with(".desktop") {
            continue;
        }
        match parse_desktop_file(&path) {
            Ok(app) => apps.push(app),
            Err(_) => continue,
        }
    }
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(apps)
}

pub fn delete_desktop_file(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path).with_context(|| format!("remove {}", path.display()))?;
    }
    refresh_desktop_database();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn write_and_parse_roundtrip() {
        let tmp = env::temp_dir().join(format!(
            "anchor-desktop-test-{}",
            std::process::id()
        ));
        let apps = tmp.join("applications");
        fs::create_dir_all(&apps).unwrap();

        // Monkey-patch via env is hard; test parse on a hand-written file instead.
        let path = apps.join("webapp-TestApp9999.desktop");
        let content = format!(
            r#"[Desktop Entry]
Version=1.0
Type=Application
Name=Test App
Comment=Web App
Exec=/usr/bin/brave-browser --app=https://example.com --class=WebApp-TestApp9999 --name=WebApp-TestApp9999 --user-data-dir=/tmp/prof
Terminal=false
Icon=/tmp/icon.png
StartupWMClass=WebApp-TestApp9999
StartupNotify=true
Categories=Network;WebBrowser;
X-WebApp-Manager={MANAGER_TAG}
X-WebApp-URL=https://example.com
X-WebApp-Browser=Brave
X-WebApp-Isolated=true
"#
        );
        // Also verify legacy tag is accepted
        let legacy_path = apps.join("webapp-Legacy9999.desktop");
        let legacy = content.replace(
            &format!("X-WebApp-Manager={MANAGER_TAG}"),
            "X-WebApp-Manager=zorin-webapp-manager",
        ).replace("TestApp9999", "Legacy9999").replace("Test App", "Legacy App");
        fs::write(&legacy_path, legacy).unwrap();
        let legacy_entry = parse_desktop_file(&legacy_path).unwrap();
        assert_eq!(legacy_entry.codename, "Legacy9999");
        fs::write(&path, content).unwrap();
        let entry = parse_desktop_file(&path).unwrap();
        assert_eq!(entry.name, "Test App");
        assert_eq!(entry.url, "https://example.com");
        assert_eq!(entry.browser, "Brave");
        assert_eq!(entry.codename, "TestApp9999");
        assert!(entry.exec.contains("--app="));
        assert_eq!(entry.startup_wm_class, "WebApp-TestApp9999");
        let _ = fs::remove_dir_all(&tmp);
    }
}
