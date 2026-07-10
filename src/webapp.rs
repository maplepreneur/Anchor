//! High-level create / list / delete API for web apps.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rand::Rng;

use crate::browser::{self, Browser, BrowserFamily};
use crate::desktop::{self, DesktopEntry};
use crate::favicon;
use crate::paths;

#[derive(Debug, Clone)]
pub struct CreateRequest {
    pub name: String,
    pub url: String,
    pub browser: Browser,
    /// If set, use this local image as the icon instead of fetching a favicon.
    pub icon_override: Option<PathBuf>,
    /// Pre-fetched icon bytes already written somewhere, or None to fetch.
    pub icon_source: IconSource,
}

#[derive(Debug, Clone)]
pub enum IconSource {
    /// Download favicon from the URL.
    Fetch,
    /// Use an existing local image path.
    Local(PathBuf),
    /// Bytes already prepared (e.g. from a successful preview fetch).
    PreparedPng(PathBuf),
}

/// Sanitize app name to alphanumeric-only, append 4 random digits.
pub fn generate_codename(name: &str) -> String {
    let alpha: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let base = if alpha.is_empty() {
        "WebApp".to_string()
    } else {
        alpha
    };
    let n: u16 = rand::thread_rng().gen_range(1000..10000);
    format!("{base}{n}")
}

pub fn list_webapps() -> Result<Vec<DesktopEntry>> {
    paths::ensure_dirs()?;
    // Repair outdated StartupWMClass (Chromium Wayland app_id) on read.
    let _ = repair_all_webapps();
    desktop::list_managed_apps()
}

/// Recompute Exec/StartupWMClass for Chromium Wayland dock matching and refresh icons.
pub fn repair_all_webapps() -> Result<usize> {
    let apps = desktop::list_managed_apps()?;
    let browsers = browser::detect_browsers();
    let mut fixed = 0;
    for app in apps {
        if repair_webapp(&app, &browsers)? {
            fixed += 1;
        }
    }
    if fixed > 0 {
        desktop::refresh_desktop_database();
    }
    Ok(fixed)
}

fn repair_webapp(app: &DesktopEntry, browsers: &[Browser]) -> Result<bool> {
    if app.url.is_empty() {
        return Ok(false);
    }

    let browser = match browsers.iter().find(|b| b.name == app.browser) {
        Some(b) => b.clone(),
        None => {
            // Fall back to Chromium-family match by name substring, else first browser.
            browsers
                .iter()
                .find(|b| {
                    app.browser.to_ascii_lowercase().contains(&b.name.to_ascii_lowercase())
                        || b.name.to_ascii_lowercase().contains(&app.browser.to_ascii_lowercase())
                })
                .cloned()
                .or_else(|| browsers.first().cloned())
                .ok_or_else(|| anyhow::anyhow!("no browser available to repair {}", app.name))?
        }
    };

    let expected_class = browser::window_class(&browser, &app.codename, &app.url);
    let icon_path = PathBuf::from(&app.icon);
    let icon_path = if icon_path.exists() {
        icon_path
    } else {
        paths::icon_path(&app.codename)?
    };

    let new_exec = browser::build_exec(&browser, &app.codename, &app.url, &icon_path)?;
    let needs_fix =
        app.startup_wm_class != expected_class || app.exec != new_exec || !app.exec.contains(&expected_class);

    // Always ensure themed icon exists under the window class name (dock fallback).
    if icon_path.exists() {
        let _ = install_themed_icons(&icon_path, &expected_class);
        let _ = install_themed_icons(&icon_path, &format!("webapp-{}", app.codename));
    }

    if !needs_fix {
        return Ok(false);
    }

    desktop::write_desktop_file(
        &app.codename,
        &app.name,
        &app.url,
        &browser.name,
        &icon_path,
        &new_exec,
        &expected_class,
    )?;
    Ok(true)
}

/// Install PNG copies into the hicolor theme so GNOME can resolve icons by class name.
fn install_themed_icons(src: &Path, theme_name: &str) -> Result<()> {
    // Icon theme names must not contain path separators.
    if theme_name.contains('/') || theme_name.is_empty() {
        return Ok(());
    }
    let base = dirs::data_local_dir().context("XDG data dir")?;
    let img = image::open(src).with_context(|| format!("open icon {}", src.display()))?;

    for size in [16u32, 32, 48, 64, 128, 256] {
        let dir = base.join(format!("icons/hicolor/{size}x{size}/apps"));
        fs::create_dir_all(&dir)?;
        let dest = dir.join(format!("{theme_name}.png"));
        let resized = img.resize_exact(size, size, image::imageops::FilterType::Lanczos3);
        resized
            .save_with_format(&dest, image::ImageFormat::Png)
            .with_context(|| format!("write {}", dest.display()))?;
    }

    // Best-effort icon cache update
    let hicolor = base.join("icons/hicolor");
    let _ = std::process::Command::new("gtk-update-icon-cache")
        .args(["-f", "-t"])
        .arg(&hicolor)
        .status();
    Ok(())
}

pub fn create_webapp(req: CreateRequest) -> Result<DesktopEntry> {
    paths::ensure_dirs()?;

    let name = req.name.trim();
    if name.is_empty() {
        bail!("name is required");
    }
    let url = favicon::normalize_url(&req.url)?.to_string();
    let codename = generate_codename(name);
    let icon_dest = paths::icon_path(&codename)?;

    match &req.icon_source {
        IconSource::Fetch => {
            favicon::fetch_favicon(&url, &icon_dest)
                .context("favicon fetch failed; choose an icon image instead")?;
        }
        IconSource::Local(path) => {
            favicon::local_image_to_png(path, &icon_dest)
                .with_context(|| format!("could not use icon {}", path.display()))?;
        }
        IconSource::PreparedPng(path) => {
            if path != &icon_dest {
                fs::copy(path, &icon_dest)
                    .with_context(|| format!("copy icon from {}", path.display()))?;
            }
        }
    }

    if let Some(override_path) = &req.icon_override {
        favicon::local_image_to_png(override_path, &icon_dest)?;
    }

    // Prepare isolated profile directory
    match req.browser.family {
        BrowserFamily::Chromium => {
            let profile = paths::chromium_profile_path(&codename)?;
            fs::create_dir_all(&profile)?;
        }
        BrowserFamily::Firefox => {
            let profile = paths::firefox_profile_path(&codename)?;
            seed_firefox_profile(&profile)?;
        }
    }

    let window_class = browser::window_class(&req.browser, &codename, &url);
    let exec = browser::build_exec(&req.browser, &codename, &url, &icon_dest)?;
    let desktop_path = desktop::write_desktop_file(
        &codename,
        name,
        &url,
        &req.browser.name,
        &icon_dest,
        &exec,
        &window_class,
    )?;

    // Theme icons keyed by window class help the dock/alt-tab when matching by class.
    let _ = install_themed_icons(&icon_dest, &window_class);
    let _ = install_themed_icons(&icon_dest, &format!("webapp-{codename}"));

    Ok(DesktopEntry {
        path: desktop_path,
        codename,
        name: name.to_string(),
        url,
        browser: req.browser.name.clone(),
        icon: icon_dest.display().to_string(),
        exec,
        startup_wm_class: window_class,
    })
}

/// Minimal Firefox profile so the app starts without the profile manager.
fn seed_firefox_profile(profile: &Path) -> Result<()> {
    fs::create_dir_all(profile)?;
    let chrome = profile.join("chrome");
    fs::create_dir_all(&chrome)?;

    // Hide most chrome for a more app-like window (optional / soft).
    let user_chrome = r#"/* Anchor — minimal web-app chrome */
#TabsToolbar,
#nav-bar,
#PersonalToolbar,
#statuspanel {
  visibility: collapse !important;
}
"#;
    fs::write(chrome.join("userChrome.css"), user_chrome)?;

    let user_js = r#"// Generated by Anchor
user_pref("toolkit.legacyUserProfileCustomizations.stylesheets", true);
user_pref("browser.startup.homepage_override.mstone", "ignore");
user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("datareporting.policy.dataSubmissionEnabled", false);
user_pref("toolkit.telemetry.enabled", false);
"#;
    fs::write(profile.join("user.js"), user_js)?;

    // Empty places so Firefox treats this as a valid profile
    fs::write(profile.join("times.json"), r#"{"created":0}"#)?;
    Ok(())
}

pub fn delete_webapp(app: &DesktopEntry) -> Result<()> {
    desktop::delete_desktop_file(&app.path)?;

    let icon = PathBuf::from(&app.icon);
    if icon.exists() {
        let _ = fs::remove_file(&icon);
    }
    // Also try canonical icon path
    if let Ok(p) = paths::icon_path(&app.codename) {
        let _ = fs::remove_file(p);
    }

    if let Ok(p) = paths::chromium_profile_path(&app.codename) {
        let _ = fs::remove_dir_all(p);
    }
    if let Ok(p) = paths::firefox_profile_path(&app.codename) {
        let _ = fs::remove_dir_all(p);
    }

    Ok(())
}

/// Try to fetch a favicon into a temporary path for UI preview.
pub fn preview_favicon(url: &str) -> Result<PathBuf> {
    paths::ensure_dirs()?;
    let tmp = paths::icons_dir()?.join(format!(
        ".preview-{}.png",
        std::process::id()
    ));
    favicon::fetch_favicon(url, &tmp)?;
    Ok(tmp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codename_is_alphanumeric_with_digits() {
        let c = generate_codename("You Tube!");
        assert!(c.starts_with("YouTube"), "got {c}");
        assert_eq!(c.len(), "YouTube".len() + 4);
        assert!(c.chars().all(|ch| ch.is_ascii_alphanumeric()));
    }

    #[test]
    fn codename_fallback_when_empty() {
        let c = generate_codename("!!!");
        assert!(c.starts_with("WebApp"));
    }
}

#[cfg(test)]
mod integration {
    use super::*;
    use crate::browser::{self, BrowserFamily};
    use std::path::PathBuf;

    #[test]
    fn create_list_delete_with_local_icon() {
        let browsers = browser::detect_browsers();
        assert!(!browsers.is_empty(), "need at least one browser installed");
        let browser = browsers
            .into_iter()
            .find(|b| b.family == BrowserFamily::Chromium)
            .or_else(|| browser::detect_browsers().into_iter().next())
            .expect("browser");

        // Make a temp PNG
        let tmp = std::env::temp_dir().join("zwm-int-icon.png");
        {
            use image::{ImageBuffer, Rgba};
            let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
                ImageBuffer::from_fn(32, 32, |_, _| Rgba([10, 100, 200, 255]));
            img.save(&tmp).unwrap();
        }

        let entry = create_webapp(CreateRequest {
            name: "ZWM Smoke Test".into(),
            url: "https://example.com".into(),
            browser,
            icon_override: None,
            icon_source: IconSource::Local(tmp.clone()),
        })
        .expect("create");

        assert!(entry.path.exists());
        assert!(PathBuf::from(&entry.icon).exists());
        let listed = list_webapps().unwrap();
        assert!(listed.iter().any(|a| a.codename == entry.codename));

        let desktop = std::fs::read_to_string(&entry.path).unwrap();
        assert!(desktop.contains("X-WebApp-Manager=anchor"));
        assert!(desktop.contains("StartupWMClass="));
        // Chromium-family apps must use the Wayland app_id, not WebApp-*
        if entry.exec.contains("--app=") {
            assert!(
                desktop.contains("StartupWMClass=brave-")
                    || desktop.contains("StartupWMClass=google-chrome-")
                    || desktop.contains("StartupWMClass=chromium-")
                    || desktop.contains("StartupWMClass=microsoft-edge-")
                    || desktop.contains("StartupWMClass=msedge-")
                    || desktop.contains("StartupWMClass=vivaldi-"),
                "unexpected StartupWMClass in:\n{desktop}"
            );
        } else {
            assert!(desktop.contains("StartupWMClass=WebApp-"));
        }
        assert!(desktop.contains("--app=") || desktop.contains("--no-remote"));

        delete_webapp(&entry).unwrap();
        assert!(!entry.path.exists());
        let listed = list_webapps().unwrap();
        assert!(!listed.iter().any(|a| a.codename == entry.codename));
        let _ = std::fs::remove_file(tmp);
    }
}
