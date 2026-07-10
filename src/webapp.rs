//! High-level create / list / edit / delete API for web apps.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rand::Rng;

use crate::browser::{self, Browser, BrowserFamily, ProfileMode};
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
    /// Isolated or shared browser profile.
    pub profile_mode: ProfileMode,
}

/// Update an existing web app while keeping its codename (launcher id).
#[derive(Debug, Clone)]
pub struct EditRequest {
    pub existing: DesktopEntry,
    pub name: String,
    pub url: String,
    pub browser: Browser,
    pub icon_source: IconSource,
    pub profile_mode: ProfileMode,
}

#[derive(Debug, Clone)]
pub enum IconSource {
    /// Download favicon from the URL.
    Fetch,
    /// Use an existing local image path.
    Local(PathBuf),
    /// Bytes already prepared (e.g. from a successful preview fetch).
    PreparedPng(PathBuf),
    /// Keep the current icon file (edit only).
    KeepExisting,
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
    let icon_path = resolve_icon_file(&app.codename, &app.icon);

    // Ensure private profile exists. Re-seed Shared apps that never got extensions
    // (wrong Firefox profile root, empty stub profile, or pre-fix launchers).
    let seed_if_needed = app.profile_mode.seeds_from_browser()
        && match browser.family {
            BrowserFamily::Chromium => {
                let p = paths::chromium_profile_path(&app.codename)?;
                !p.join("Default").join("Preferences").exists()
                    && !p.join("Default").join("Extensions").exists()
            }
            BrowserFamily::Firefox => {
                let p = paths::firefox_profile_path(&app.codename)?;
                !firefox_profile_has_extensions(&p)
            }
        };
    let _ = prepare_private_profile(&browser, &app.codename, app.profile_mode, seed_if_needed);

    let new_exec = browser::build_exec(
        &browser,
        &app.codename,
        &app.url,
        &icon_path,
        app.profile_mode,
    )?;

    // Desktop Icon= must be an absolute PNG path so the Anchor list (and menus) show
    // the favicon immediately. Themed copies under StartupWMClass still cover the dock.
    let desktop_icon = icon_path.display().to_string();
    let expected_icon_line = format!("Icon={desktop_icon}");
    let current_desktop = fs::read_to_string(&app.path).unwrap_or_default();
    let icon_mismatch = !current_desktop
        .lines()
        .any(|l| l.trim() == expected_icon_line);

    let needs_fix = app.startup_wm_class != expected_class
        || app.exec != new_exec
        || !app.exec.contains(&expected_class)
        || icon_mismatch
        || (browser.family == BrowserFamily::Chromium && !app.exec.contains("--user-data-dir="))
        || (browser.family == BrowserFamily::Firefox
            && app.profile_mode.seeds_from_browser()
            && seed_if_needed);

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
        app.profile_mode,
    )?;
    Ok(true)
}

/// Resolve the on-disk PNG for a web app (canonical path or legacy Icon= values).
pub fn resolve_icon_file(codename: &str, icon_field: &str) -> PathBuf {
    let direct = PathBuf::from(icon_field);
    if !icon_field.is_empty() && direct.is_file() {
        return direct;
    }
    if let Ok(canonical) = paths::icon_path(codename) {
        if canonical.is_file() {
            return canonical;
        }
    }
    // Theme-name Icon= (older builds): try hicolor install of the class / webapp id.
    if !icon_field.is_empty() && !icon_field.contains('/') {
        if let Some(base) = dirs::data_local_dir() {
            for size in [256u32, 128, 64, 48, 32, 16] {
                let p = base.join(format!(
                    "icons/hicolor/{size}x{size}/apps/{icon_field}.png"
                ));
                if p.is_file() {
                    return p;
                }
            }
        }
    }
    paths::icon_path(codename).unwrap_or(direct)
}

fn firefox_profile_has_extensions(profile: &Path) -> bool {
    let ext_dir = profile.join("extensions");
    if ext_dir.is_dir() {
        if let Ok(rd) = fs::read_dir(&ext_dir) {
            for entry in rd.flatten() {
                let name = entry.file_name();
                let s = name.to_string_lossy();
                if s.ends_with(".xpi") || entry.path().is_dir() {
                    return true;
                }
            }
        }
    }
    false
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

    apply_icon_source(&req.icon_source, &url, &icon_dest)?;

    if let Some(override_path) = &req.icon_override {
        favicon::local_image_to_png(override_path, &icon_dest)?;
    }

    // Both modes use a private profile dir; Shared seeds from the browser.
    prepare_private_profile(&req.browser, &codename, req.profile_mode, true)?;

    write_launcher(
        &codename,
        name,
        &url,
        &req.browser,
        &icon_dest,
        req.profile_mode,
    )
}

/// Update an existing web app in place (same codename / desktop path).
pub fn update_webapp(req: EditRequest) -> Result<DesktopEntry> {
    paths::ensure_dirs()?;

    let name = req.name.trim();
    if name.is_empty() {
        bail!("name is required");
    }
    let url = favicon::normalize_url(&req.url)?.to_string();
    let codename = req.existing.codename.clone();
    let icon_dest = paths::icon_path(&codename)?;

    // Ensure canonical icon path exists when keeping the previous icon.
    if matches!(req.icon_source, IconSource::KeepExisting) {
        let previous = PathBuf::from(&req.existing.icon);
        if !icon_dest.exists() {
            if previous.exists() && previous != icon_dest {
                fs::copy(&previous, &icon_dest).with_context(|| {
                    format!(
                        "copy existing icon from {} to {}",
                        previous.display(),
                        icon_dest.display()
                    )
                })?;
            } else if !previous.exists() {
                bail!("no existing icon to keep; fetch or choose an image");
            }
        }
    } else {
        apply_icon_source(&req.icon_source, &url, &icon_dest)?;
    }

    let old_mode = req.existing.profile_mode;
    let new_mode = req.profile_mode;

    // Switching Shared → Isolated: wipe seeded data for a clean private profile.
    if old_mode.seeds_from_browser() && !new_mode.seeds_from_browser() {
        remove_private_profiles(&codename);
    } else if let Some(old_browser) = resolve_browser_by_name(&req.existing.browser) {
        // If browser family changed, remove the unused family's private profile.
        if old_browser.family != req.browser.family {
            match old_browser.family {
                BrowserFamily::Chromium => {
                    if let Ok(p) = paths::chromium_profile_path(&codename) {
                        let _ = fs::remove_dir_all(p);
                    }
                }
                BrowserFamily::Firefox => {
                    if let Ok(p) = paths::firefox_profile_path(&codename) {
                        let _ = fs::remove_dir_all(p);
                    }
                }
            }
        }
    }

    // Seed when entering Shared, or when Shared + browser changed (refresh extensions).
    let force_seed = new_mode.seeds_from_browser()
        && (old_mode != new_mode || req.existing.browser != req.browser.name);
    prepare_private_profile(&req.browser, &codename, new_mode, force_seed)?;

    write_launcher(
        &codename,
        name,
        &url,
        &req.browser,
        &icon_dest,
        new_mode,
    )
}

fn apply_icon_source(source: &IconSource, url: &str, icon_dest: &Path) -> Result<()> {
    match source {
        IconSource::Fetch => {
            favicon::fetch_favicon(url, icon_dest)
                .context("favicon fetch failed; choose an icon image instead")?;
        }
        IconSource::Local(path) => {
            favicon::local_image_to_png(path, icon_dest)
                .with_context(|| format!("could not use icon {}", path.display()))?;
        }
        IconSource::PreparedPng(path) => {
            if path != icon_dest {
                fs::copy(path, icon_dest)
                    .with_context(|| format!("copy icon from {}", path.display()))?;
            }
        }
        IconSource::KeepExisting => {
            if !icon_dest.exists() {
                bail!("no existing icon to keep");
            }
        }
    }
    Ok(())
}

fn prepare_private_profile(
    browser: &Browser,
    codename: &str,
    mode: ProfileMode,
    allow_seed: bool,
) -> Result<()> {
    match browser.family {
        BrowserFamily::Chromium => {
            let profile = paths::chromium_profile_path(codename)?;
            fs::create_dir_all(&profile)?;
            if allow_seed && mode.seeds_from_browser() {
                let _ = seed_chromium_from_browser(browser, &profile);
            }
        }
        BrowserFamily::Firefox => {
            let profile = paths::firefox_profile_path(codename)?;
            if !profile.exists() {
                seed_firefox_profile(&profile)?;
            } else {
                fs::create_dir_all(&profile)?;
            }
            if allow_seed && mode.seeds_from_browser() {
                let _ = seed_firefox_from_browser(browser, &profile);
            }
        }
    }
    Ok(())
}

fn remove_private_profiles(codename: &str) {
    if let Ok(p) = paths::chromium_profile_path(codename) {
        let _ = fs::remove_dir_all(p);
    }
    if let Ok(p) = paths::firefox_profile_path(codename) {
        let _ = fs::remove_dir_all(p);
    }
}

fn resolve_browser_by_name(name: &str) -> Option<Browser> {
    let browsers = browser::detect_browsers();
    browsers
        .iter()
        .find(|b| b.name == name)
        .cloned()
        .or_else(|| {
            browsers.into_iter().find(|b| {
                name.to_ascii_lowercase()
                    .contains(&b.name.to_ascii_lowercase())
                    || b.name
                        .to_ascii_lowercase()
                        .contains(&name.to_ascii_lowercase())
            })
        })
}

fn write_launcher(
    codename: &str,
    name: &str,
    url: &str,
    browser: &Browser,
    icon_dest: &Path,
    profile_mode: ProfileMode,
) -> Result<DesktopEntry> {
    let window_class = browser::window_class(browser, codename, url);
    let exec = browser::build_exec(browser, codename, url, icon_dest, profile_mode)?;

    // Themed copies named after StartupWMClass so GNOME can resolve dock icons by app_id.
    let _ = install_themed_icons(icon_dest, &window_class);
    let _ = install_themed_icons(icon_dest, &format!("webapp-{codename}"));

    // Absolute path in Icon= so the Anchor list and app menu show the favicon on first
    // launch (theme-only names do not resolve as files in gtk::Image::from_file).
    let desktop_path = desktop::write_desktop_file(
        codename,
        name,
        url,
        &browser.name,
        icon_dest,
        &exec,
        &window_class,
        profile_mode,
    )?;

    Ok(DesktopEntry {
        path: desktop_path,
        codename: codename.to_string(),
        name: name.to_string(),
        url: url.to_string(),
        browser: browser.name.clone(),
        icon: icon_dest.display().to_string(),
        exec,
        startup_wm_class: window_class,
        profile_mode,
    })
}

/// Best-effort copy of extensions + session data from the browser into a private
/// Chromium user-data-dir so Shared mode keeps logins/password managers without
/// joining the browser process (which breaks dock icons).
fn seed_chromium_from_browser(browser: &Browser, isolated_user_data: &Path) -> Result<()> {
    let Some(source_root) = browser::chromium_default_user_data_dir(browser) else {
        return Ok(());
    };
    let source_default = source_root.join("Default");
    if !source_default.is_dir() {
        return Ok(());
    }
    let dest_default = isolated_user_data.join("Default");
    fs::create_dir_all(&dest_default)?;

    // Directories (extensions + settings)
    for name in [
        "Extensions",
        "Local Extension Settings",
        "Extension State",
        "Extension Rules",
        "Sync Extension Settings",
        "Managed Extension Settings",
        "Local Storage",
        "Session Storage",
        "IndexedDB",
        "Service Worker",
    ] {
        let src = source_default.join(name);
        if src.is_dir() {
            let dest = dest_default.join(name);
            let _ = copy_dir_recursive(&src, &dest);
        }
    }

    // Files (cookies, passwords, preferences) — ignore lock failures if browser is open
    for name in [
        "Cookies",
        "Cookies-journal",
        "Login Data",
        "Login Data-journal",
        "Login Data For Account",
        "Login Data For Account-journal",
        "Web Data",
        "Web Data-journal",
        "Preferences",
        "Secure Preferences",
        "Bookmarks",
    ] {
        let src = source_default.join(name);
        if src.is_file() {
            let dest = dest_default.join(name);
            let _ = fs::copy(&src, &dest);
        }
    }
    Ok(())
}

/// Best-effort copy of Firefox extension/session files into an isolated profile.
fn seed_firefox_from_browser(browser: &Browser, isolated_profile: &Path) -> Result<()> {
    let Some(source) = browser::firefox_default_profile_dir(browser) else {
        return Ok(());
    };

    // Directories first — XPI payloads live under extensions/.
    for name in [
        "extensions",
        "extension-store",
        "extension-store-menus",
        "browser-extension-data",
    ] {
        let src = source.join(name);
        if src.is_dir() {
            let dest = isolated_profile.join(name);
            let _ = copy_dir_recursive(&src, &dest);
        }
    }

    // Extension storage (moz-extension+++…) — large but needed for logged-in addons.
    let storage_src = source.join("storage/default");
    if storage_src.is_dir() {
        let storage_dest = isolated_profile.join("storage/default");
        if let Ok(rd) = fs::read_dir(&storage_src) {
            for entry in rd.flatten() {
                let name = entry.file_name();
                let s = name.to_string_lossy();
                if s.starts_with("moz-extension+++") || s.starts_with("moz-extension+") {
                    let _ = copy_dir_recursive(&entry.path(), &storage_dest.join(name));
                }
            }
        }
    }

    // Registry / state files. Rewrite absolute paths inside JSON so addons resolve
    // under the new profile instead of the source browser profile.
    for name in [
        "extensions.json",
        "addons.json",
        "addonStartup.json.lz4",
        "extension-preferences.json",
        "extension-settings.json",
        "cookies.sqlite",
        "cookies.sqlite-wal",
        "cookies.sqlite-shm",
        "logins.json",
        "logins-backup.json",
        "key4.db",
        "cert9.db",
        "places.sqlite",
        "prefs.js",
    ] {
        let src = source.join(name);
        let dest = isolated_profile.join(name);
        if !src.is_file() {
            continue;
        }
        if let Some(parent) = dest.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if name.ends_with(".json") {
            if let Ok(text) = fs::read_to_string(&src) {
                let rewritten = rewrite_firefox_profile_paths(&text, &source, isolated_profile);
                let _ = fs::write(&dest, rewritten);
                continue;
            }
        }
        let _ = fs::copy(&src, &dest);
    }

    // Ensure userChrome still applies after prefs.js seed.
    let user_js = isolated_profile.join("user.js");
    if !user_js.exists() {
        let _ = fs::write(
            &user_js,
            r#"// Generated by Anchor
user_pref("toolkit.legacyUserProfileCustomizations.stylesheets", true);
user_pref("browser.startup.homepage_override.mstone", "ignore");
user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("datareporting.policy.dataSubmissionEnabled", false);
user_pref("toolkit.telemetry.enabled", false);
"#,
        );
    }
    Ok(())
}

/// Rewrite absolute profile paths inside Firefox JSON so seeded addons load from
/// the Anchor profile rather than the original browser profile.
fn rewrite_firefox_profile_paths(text: &str, source: &Path, dest: &Path) -> String {
    let mut out = text.to_string();
    let src_s = source.display().to_string();
    let dst_s = dest.display().to_string();
    if src_s != dst_s {
        out = out.replace(&src_s, &dst_s);
        // Also handle file:// URIs
        let src_uri = format!("file://{src_s}");
        let dst_uri = format!("file://{dst_s}");
        out = out.replace(&src_uri, &dst_uri);
    }
    out
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest).with_context(|| format!("create {}", dest.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("read {}", src.display()))? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if ty.is_file() {
            fs::copy(&from, &to)
                .with_context(|| format!("copy {} -> {}", from.display(), to.display()))?;
        }
    }
    Ok(())
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

    let icon = resolve_icon_file(&app.codename, &app.icon);
    if icon.is_file() {
        let _ = fs::remove_file(&icon);
    }
    // Also try canonical icon path
    if let Ok(p) = paths::icon_path(&app.codename) {
        let _ = fs::remove_file(p);
    }

    // Only remove Anchor-owned private profiles — never the browser default profile.
    if app.profile_mode.uses_private_profile() {
        if let Ok(p) = paths::chromium_profile_path(&app.codename) {
            let _ = fs::remove_dir_all(p);
        }
        if let Ok(p) = paths::firefox_profile_path(&app.codename) {
            let _ = fs::remove_dir_all(p);
        }
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
            profile_mode: ProfileMode::Isolated,
        })
        .expect("create");

        assert!(entry.path.exists());
        assert!(PathBuf::from(&entry.icon).exists());
        assert_eq!(entry.profile_mode, ProfileMode::Isolated);
        let listed = list_webapps().unwrap();
        assert!(listed.iter().any(|a| a.codename == entry.codename));

        let desktop = std::fs::read_to_string(&entry.path).unwrap();
        assert!(desktop.contains("X-WebApp-Manager=anchor"));
        assert!(desktop.contains("X-WebApp-ProfileMode=isolated"));
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
            assert!(entry.exec.contains("--user-data-dir="));
        } else {
            assert!(desktop.contains("StartupWMClass=WebApp-"));
        }
        assert!(desktop.contains("--app=") || desktop.contains("--no-remote"));

        delete_webapp(&entry).unwrap();
        assert!(!entry.path.exists());
        let listed = list_webapps().unwrap();
        assert!(!listed.iter().any(|a| a.codename == entry.codename));

        // Shared profile: private user-data-dir (separate dock process) + seeded data
        let browser = browser::detect_browsers()
            .into_iter()
            .find(|b| b.family == BrowserFamily::Chromium)
            .or_else(|| browser::detect_browsers().into_iter().next())
            .expect("browser");
        let shared = create_webapp(CreateRequest {
            name: "ZWM Shared Test".into(),
            url: "https://example.com".into(),
            browser: browser.clone(),
            icon_override: None,
            icon_source: IconSource::Local(tmp.clone()),
            profile_mode: ProfileMode::Shared,
        })
        .expect("create shared");
        assert_eq!(shared.profile_mode, ProfileMode::Shared);
        let shared_desktop = std::fs::read_to_string(&shared.path).unwrap();
        assert!(shared_desktop.contains("X-WebApp-ProfileMode=shared"));
        assert!(shared_desktop.contains("X-WebApp-Isolated=false"));
        if browser.family == BrowserFamily::Chromium {
            assert!(
                shared.exec.contains("--user-data-dir="),
                "shared mode must use private user-data-dir for dock separation"
            );
            let profile = paths::chromium_profile_path(&shared.codename).unwrap();
            assert!(
                profile.exists(),
                "shared mode must create a private profile directory"
            );
            // Icon= is absolute PNG path so the list UI can show it on first launch
            assert!(
                shared_desktop.contains("Icon=/") && shared_desktop.contains(".png"),
                "desktop Icon should be absolute PNG path:\n{shared_desktop}"
            );
            assert!(shared_desktop.contains(&format!(
                "StartupWMClass={}",
                shared.startup_wm_class
            )));
            // resolve_icon_file must find the PNG for the list UI
            let resolved = resolve_icon_file(&shared.codename, &shared.icon);
            assert!(
                resolved.is_file(),
                "resolved icon missing: {}",
                resolved.display()
            );
        }
        delete_webapp(&shared).unwrap();
        assert!(!shared.path.exists());

        // Edit: create then change name/url/mode in place (codename stable)
        let browser = browser::detect_browsers()
            .into_iter()
            .find(|b| b.family == BrowserFamily::Chromium)
            .or_else(|| browser::detect_browsers().into_iter().next())
            .expect("browser");
        let created = create_webapp(CreateRequest {
            name: "ZWM Edit Test".into(),
            url: "https://example.com".into(),
            browser: browser.clone(),
            icon_override: None,
            icon_source: IconSource::Local(tmp.clone()),
            profile_mode: ProfileMode::Isolated,
        })
        .expect("create for edit");
        let codename = created.codename.clone();
        let updated = update_webapp(EditRequest {
            existing: created.clone(),
            name: "ZWM Edited".into(),
            url: "https://example.org/path".into(),
            browser: browser.clone(),
            icon_source: IconSource::KeepExisting,
            profile_mode: ProfileMode::Shared,
        })
        .expect("update");
        assert_eq!(updated.codename, codename);
        assert_eq!(updated.name, "ZWM Edited");
        assert!(updated.url.contains("example.org"));
        assert_eq!(updated.profile_mode, ProfileMode::Shared);
        assert_eq!(updated.path, created.path);
        let desktop = std::fs::read_to_string(&updated.path).unwrap();
        assert!(desktop.contains("Name=ZWM Edited"));
        assert!(desktop.contains("X-WebApp-ProfileMode=shared"));
        if browser.family == BrowserFamily::Chromium {
            assert!(
                updated.exec.contains("--user-data-dir="),
                "shared edit must keep private user-data-dir"
            );
        }
        delete_webapp(&updated).unwrap();

        let _ = std::fs::remove_file(tmp);
    }
}
