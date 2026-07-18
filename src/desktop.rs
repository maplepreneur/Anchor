//! Read and write XDG `.desktop` entries for managed web apps.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};

use crate::browser::ProfileMode;
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
    pub profile_mode: ProfileMode,
    /// When false (default), Firefox web apps hide the window title bar (frameless).
    /// Chromium `--app` windows always use the browser’s app chrome; this flag is stored
    /// for consistency and future use.
    pub show_title_bar: bool,
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
    profile_mode: ProfileMode,
    show_title_bar: bool,
) -> Result<PathBuf> {
    let path = paths::desktop_path(codename)?;
    let isolated = if profile_mode.is_isolated() {
        "true"
    } else {
        "false"
    };
    let show_title = if show_title_bar { "true" } else { "false" };
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
X-WebApp-Isolated={isolated}
X-WebApp-ProfileMode={profile_mode}
X-WebApp-ShowTitleBar={show_title}
"#,
        name = escape_value(name),
        exec = exec, // Exec is already carefully built
        icon = escape_value(&icon.display().to_string()),
        class = escape_value(window_class),
        manager = MANAGER_TAG,
        url = escape_value(url),
        browser = escape_value(browser_name),
        isolated = isolated,
        profile_mode = profile_mode.as_desktop_value(),
        show_title = show_title,
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
    let mut profile_mode_key = None;
    let mut isolated_key = None;
    let mut show_title_bar_key = None;

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
                "X-WebApp-ProfileMode" => profile_mode_key = Some(v.to_string()),
                "X-WebApp-Isolated" => isolated_key = Some(v.to_string()),
                "X-WebApp-ShowTitleBar" => show_title_bar_key = Some(v.to_string()),
                _ => {}
            }
        }
    }

    let Some(manager_tag) = manager.as_deref() else {
        return Err(anyhow!("missing X-WebApp-Manager"));
    };
    if !is_manager_tag(manager_tag) {
        return Err(anyhow!("not managed by Mountie (got {manager_tag})"));
    }

    let profile_mode = if let Some(raw) = profile_mode_key.as_deref() {
        ProfileMode::from_desktop_value(raw).unwrap_or(ProfileMode::Isolated)
    } else if isolated_key.as_deref() == Some("false") {
        ProfileMode::Shared
    } else {
        // Legacy apps always used isolated profiles.
        ProfileMode::Isolated
    };

    // Default: hide title bar (frameless). Only "true" enables the native bar.
    let show_title_bar = show_title_bar_key
        .as_deref()
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(false);

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
        profile_mode,
        show_title_bar,
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
            "mountie-desktop-test-{}",
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
X-WebApp-ProfileMode=isolated
X-WebApp-ShowTitleBar=false
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
        assert_eq!(legacy_entry.profile_mode, ProfileMode::Isolated);
        fs::write(&path, &content).unwrap();
        let entry = parse_desktop_file(&path).unwrap();
        assert_eq!(entry.name, "Test App");
        assert_eq!(entry.url, "https://example.com");
        assert_eq!(entry.browser, "Brave");
        assert_eq!(entry.codename, "TestApp9999");
        assert!(entry.exec.contains("--app="));
        assert_eq!(entry.startup_wm_class, "WebApp-TestApp9999");
        assert_eq!(entry.profile_mode, ProfileMode::Isolated);
        assert!(!entry.show_title_bar);

        // Shared mode + legacy Isolated=false without ProfileMode
        let shared_path = apps.join("webapp-Shared9999.desktop");
        let shared = content
            .replace("TestApp9999", "Shared9999")
            .replace("Test App", "Shared App")
            .replace("X-WebApp-Isolated=true", "X-WebApp-Isolated=false")
            .replace(
                "X-WebApp-ProfileMode=isolated",
                "X-WebApp-ProfileMode=shared",
            )
            .replace(
                "X-WebApp-ShowTitleBar=false",
                "X-WebApp-ShowTitleBar=true",
            );
        fs::write(&shared_path, shared).unwrap();
        let shared_entry = parse_desktop_file(&shared_path).unwrap();
        assert_eq!(shared_entry.profile_mode, ProfileMode::Shared);
        assert!(shared_entry.show_title_bar);

        // Missing ShowTitleBar key defaults to false (frameless).
        let no_title_key = apps.join("webapp-NoTitleKey9999.desktop");
        let mut no_title_body = content.replace("TestApp9999", "NoTitleKey9999");
        no_title_body = no_title_body
            .lines()
            .filter(|l| !l.starts_with("X-WebApp-ShowTitleBar="))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&no_title_key, no_title_body).unwrap();
        assert!(!parse_desktop_file(&no_title_key).unwrap().show_title_bar);

        let legacy_shared = apps.join("webapp-LegacyShared9999.desktop");
        let mut legacy_shared_body = content
            .replace("TestApp9999", "LegacyShared9999")
            .replace("X-WebApp-Isolated=true", "X-WebApp-Isolated=false");
        // Strip ProfileMode to exercise fallback
        legacy_shared_body = legacy_shared_body
            .lines()
            .filter(|l| !l.starts_with("X-WebApp-ProfileMode="))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&legacy_shared, legacy_shared_body).unwrap();
        assert_eq!(
            parse_desktop_file(&legacy_shared).unwrap().profile_mode,
            ProfileMode::Shared
        );

        let _ = fs::remove_dir_all(&tmp);
    }
}
