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
    /// When false (default), hide the window title bar for a frameless app look.
    pub show_title_bar: bool,
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
    /// When false (default), hide the window title bar for a frameless app look.
    pub show_title_bar: bool,
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
    // or site session data (IndexedDB/localStorage) — e.g. WhatsApp Web logins.
    let seed_if_needed = app.profile_mode.seeds_from_browser()
        && match browser.family {
            BrowserFamily::Chromium => {
                let p = paths::chromium_profile_path(&app.codename)?;
                !p.join("Default").join("Preferences").exists()
                    && !p.join("Default").join("Extensions").exists()
            }
            BrowserFamily::Firefox => {
                let p = paths::firefox_profile_path(&app.codename)?;
                let missing_ext = !firefox_profile_has_extensions(&p);
                let missing_site = browser::firefox_default_profile_dir(&browser)
                    .map(|src| {
                        source_has_origin_storage(&src, &app.url)
                            && !firefox_profile_has_site_session(&p, &app.url)
                    })
                    .unwrap_or(false);
                missing_ext || missing_site
            }
        };
    let _ = prepare_private_profile(
        &browser,
        &app.codename,
        app.profile_mode,
        seed_if_needed,
        &app.url,
        app.show_title_bar,
    );

    let new_exec = browser::build_exec(
        &browser,
        &app.codename,
        &app.url,
        &icon_path,
        app.profile_mode,
    )?;

    // Desktop Icon= must be an absolute PNG path so the Mountie list (and menus) show
    // the favicon immediately. Themed copies under StartupWMClass still cover the dock.
    let desktop_icon = icon_path.display().to_string();
    let expected_icon_line = format!("Icon={desktop_icon}");
    let current_desktop = fs::read_to_string(&app.path).unwrap_or_default();
    let icon_mismatch = !current_desktop
        .lines()
        .any(|l| l.trim() == expected_icon_line);
    let title_bar_key_missing = !current_desktop
        .lines()
        .any(|l| l.trim().starts_with("X-WebApp-ShowTitleBar="));

    let needs_fix = app.startup_wm_class != expected_class
        || app.exec != new_exec
        || !app.exec.contains(&expected_class)
        || icon_mismatch
        || title_bar_key_missing
        || (browser.family == BrowserFamily::Chromium && !app.exec.contains("--user-data-dir="))
        || (browser.family == BrowserFamily::Firefox
            && app.profile_mode.seeds_from_browser()
            && seed_if_needed);

    // Always ensure themed icon exists under the window class name (dock fallback).
    if icon_path.exists() {
        let _ = install_themed_icons(&icon_path, &expected_class);
        let _ = install_themed_icons(&icon_path, &format!("webapp-{}", app.codename));
    }

    // Re-apply Firefox chrome for the stored title-bar preference even when the
    // .desktop file is already correct (Shared seed / runtime may rewrite prefs).
    let mut chrome_fixed = false;
    if browser.family == BrowserFamily::Firefox {
        if let Ok(profile) = paths::firefox_profile_path(&app.codename) {
            if profile.is_dir() {
                chrome_fixed = ensure_firefox_app_chrome(&profile, app.show_title_bar)?;
            }
        }
    }

    if !needs_fix {
        return Ok(chrome_fixed);
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
        app.show_title_bar,
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

/// Host from a page URL (lowercase), if parseable.
fn host_from_url(page_url: &str) -> Option<String> {
    url::Url::parse(page_url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
}

/// Last two labels of a host (`web.whatsapp.com` → `whatsapp.com`).
fn domain_suffix(host: &str) -> Option<&str> {
    let mut parts = host.rsplitn(3, '.');
    let tld = parts.next()?;
    let sld = parts.next()?;
    if tld.is_empty() || sld.is_empty() {
        return None;
    }
    // host is borrowed; return a slice into it
    let start = host.len().checked_sub(sld.len() + 1 + tld.len())?;
    Some(&host[start..])
}

/// Whether a Firefox `storage/default` directory name belongs to `host` / its domain.
///
/// Names look like `https+++web.whatsapp.com` or
/// `https+++flows.whatsapp.net^partitionKey=%28https%2Cwhatsapp.com%29`.
fn origin_matches_host(dir_name: &str, host: &str) -> bool {
    let name = dir_name.to_ascii_lowercase();
    if name.starts_with("moz-extension") {
        return false;
    }
    if name.contains(host) {
        return true;
    }
    if let Some(suffix) = domain_suffix(host) {
        // Match sibling subdomains (accounts.google.com for mail.google.com).
        if name.contains(suffix) {
            return true;
        }
    }
    false
}

/// True if the browser profile has origin storage for this page URL.
fn source_has_origin_storage(source_profile: &Path, page_url: &str) -> bool {
    let Some(host) = host_from_url(page_url) else {
        return false;
    };
    let storage = source_profile.join("storage/default");
    let Ok(rd) = fs::read_dir(&storage) else {
        return false;
    };
    rd.flatten().any(|e| {
        let name = e.file_name();
        origin_matches_host(&name.to_string_lossy(), &host) && e.path().is_dir()
    })
}

/// True if the Mountie profile already has non-stub site storage for the page URL.
///
/// Empty Firefox IndexedDB shells are typically ~48 KiB; real WhatsApp sessions are multi-MB.
fn firefox_profile_has_site_session(profile: &Path, page_url: &str) -> bool {
    let Some(host) = host_from_url(page_url) else {
        return true;
    };
    let storage = profile.join("storage/default");
    let Ok(rd) = fs::read_dir(&storage) else {
        return false;
    };
    for entry in rd.flatten() {
        let name = entry.file_name();
        if !origin_matches_host(&name.to_string_lossy(), &host) {
            continue;
        }
        if origin_has_substantial_data(&entry.path()) {
            return true;
        }
    }
    false
}

fn origin_has_substantial_data(origin_dir: &Path) -> bool {
    // Anything above an empty SQLite shell (~48–64 KiB) counts as real session data.
    const MIN_BYTES: u64 = 100 * 1024;
    for sub in ["idb", "ls", "cache"] {
        let dir = origin_dir.join(sub);
        if !dir.is_dir() {
            continue;
        }
        if let Ok(rd) = fs::read_dir(&dir) {
            for entry in rd.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Ok(meta) = entry.metadata() {
                        if meta.len() >= MIN_BYTES {
                            return true;
                        }
                    }
                } else if path.is_dir() {
                    // cache/morgue/… blobs
                    if dir_has_file_at_least(&path, MIN_BYTES) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn dir_has_file_at_least(dir: &Path, min_bytes: u64) -> bool {
    let Ok(rd) = fs::read_dir(dir) else {
        return false;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Ok(meta) = entry.metadata() {
                if meta.len() >= min_bytes {
                    return true;
                }
            }
        } else if path.is_dir() && dir_has_file_at_least(&path, min_bytes) {
            return true;
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
    prepare_private_profile(
        &req.browser,
        &codename,
        req.profile_mode,
        true,
        &url,
        req.show_title_bar,
    )?;

    write_launcher(
        &codename,
        name,
        &url,
        &req.browser,
        &icon_dest,
        req.profile_mode,
        req.show_title_bar,
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
    // Always re-apply chrome when title-bar preference or browser/profile changed.
    prepare_private_profile(
        &req.browser,
        &codename,
        new_mode,
        force_seed,
        &url,
        req.show_title_bar,
    )?;

    write_launcher(
        &codename,
        name,
        &url,
        &req.browser,
        &icon_dest,
        new_mode,
        req.show_title_bar,
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
    page_url: &str,
    show_title_bar: bool,
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
                seed_firefox_profile(&profile, show_title_bar)?;
            } else {
                fs::create_dir_all(&profile)?;
            }
            if allow_seed && mode.seeds_from_browser() {
                let _ = seed_firefox_from_browser(browser, &profile, page_url, show_title_bar);
            }
            // Always re-apply app chrome after seed for the chosen title-bar mode.
            let _ = ensure_firefox_app_chrome(&profile, show_title_bar);
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
    show_title_bar: bool,
) -> Result<DesktopEntry> {
    let window_class = browser::window_class(browser, codename, url);
    let exec = browser::build_exec(browser, codename, url, icon_dest, profile_mode)?;

    // Themed copies named after StartupWMClass so GNOME can resolve dock icons by app_id.
    let _ = install_themed_icons(icon_dest, &window_class);
    let _ = install_themed_icons(icon_dest, &format!("webapp-{codename}"));

    // Absolute path in Icon= so the Mountie list and app menu show the favicon on first
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
        show_title_bar,
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
        show_title_bar,
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

/// Best-effort copy of Firefox extensions + session data into a private profile.
///
/// Chromium Shared mode copies IndexedDB/Local Storage; Firefox must do the same for
/// sites like WhatsApp Web that keep logins in origin storage, not only cookies.
fn seed_firefox_from_browser(
    browser: &Browser,
    isolated_profile: &Path,
    page_url: &str,
    show_title_bar: bool,
) -> Result<()> {
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

    // Extension storage + site origin storage (IndexedDB / localStorage / Cache API).
    // Only copy origins that match the web app URL so we don't clone the entire
    // browser profile (~hundreds of MB) into every Shared app.
    let storage_src = source.join("storage/default");
    if storage_src.is_dir() {
        let storage_dest = isolated_profile.join("storage/default");
        let host = host_from_url(page_url);
        if let Ok(rd) = fs::read_dir(&storage_src) {
            for entry in rd.flatten() {
                let name = entry.file_name();
                let s = name.to_string_lossy();
                let is_extension =
                    s.starts_with("moz-extension+++") || s.starts_with("moz-extension+");
                let is_site = host
                    .as_ref()
                    .map(|h| origin_matches_host(&s, h))
                    .unwrap_or(false);
                if !is_extension && !is_site {
                    continue;
                }
                let dest = storage_dest.join(&name);
                // Replace incomplete stubs left by a prior launch before seed ran.
                if dest.exists() {
                    let _ = fs::remove_dir_all(&dest);
                }
                let _ = copy_dir_recursive(&entry.path(), &dest);
            }
        }
    }

    // QuotaManager / legacy localStorage registries (best-effort; missing is OK).
    for rel in [
        "storage.sqlite",
        "storage.sqlite-wal",
        "storage.sqlite-shm",
        "storage/ls-archive.sqlite",
        "storage/ls-archive.sqlite-wal",
        "storage/ls-archive.sqlite-shm",
    ] {
        let src = source.join(rel);
        let dest = isolated_profile.join(rel);
        if src.is_file() {
            if let Some(parent) = dest.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::copy(&src, &dest);
        }
    }

    // Registry / state files. Rewrite absolute paths inside JSON so addons resolve
    // under the new profile instead of the source browser profile.
    // Include SQLite WAL/SHM so we don't seed a stale main DB while Firefox is open.
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
        "places.sqlite-wal",
        "places.sqlite-shm",
        "permissions.sqlite",
        "permissions.sqlite-wal",
        "permissions.sqlite-shm",
        "webappsstore.sqlite",
        "webappsstore.sqlite-wal",
        "webappsstore.sqlite-shm",
        "serviceworker.txt",
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

    // Always re-apply app chrome / user.js after prefs.js seed (seed overwrites prefs).
    let _ = ensure_firefox_app_chrome(isolated_profile, show_title_bar);
    Ok(())
}

/// Rewrite absolute profile paths inside Firefox JSON so seeded addons load from
/// the Mountie profile rather than the original browser profile.
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

/// Hide tabs / URL bar / sidebars, and fully collapse the CSD title strip.
///
/// Firefox 133+ vertical tabs live under `#sidebar-container` *outside*
/// `#navigator-toolbox`. Shared profiles seed `sidebar.verticalTabs=true` from
/// the main browser, which left a tab strip on apps like Grok.
const FIREFOX_USER_CHROME_FRAMELESS: &str = r#"/* Mountie — frameless web-app chrome (no title bar) */
#navigator-toolbox {
  display: none !important;
}

#titlebar,
#TabsToolbar,
#tabbrowser-tabs,
#nav-bar,
#PersonalToolbar {
  display: none !important;
  visibility: collapse !important;
}

/* Vertical tabs + new sidebar (outside navigator-toolbox) */
#sidebar-container,
sidebar-main,
#vertical-tabs,
#sidebar-launcher-splitter,
#sidebar-box,
#sidebar-header,
#sidebar-splitter,
#ai-window-box,
#ai-window-splitter,
#statuspanel {
  display: none !important;
  visibility: collapse !important;
  width: 0 !important;
  min-width: 0 !important;
  max-width: 0 !important;
}
"#;

/// Hide tabs / URL bar but keep the native window title bar (close/min/max + title).
const FIREFOX_USER_CHROME_WITH_TITLEBAR: &str = r#"/* Mountie — web-app chrome with window title bar */
#TabsToolbar,
#tabbrowser-tabs,
#nav-bar,
#PersonalToolbar {
  visibility: collapse !important;
}

/* Vertical tabs + new sidebar (outside navigator-toolbox) */
#sidebar-container,
sidebar-main,
#vertical-tabs,
#sidebar-launcher-splitter,
#sidebar-box,
#sidebar-header,
#sidebar-splitter,
#ai-window-box,
#ai-window-splitter,
#statuspanel {
  display: none !important;
  visibility: collapse !important;
  width: 0 !important;
  min-width: 0 !important;
  max-width: 0 !important;
}
"#;

fn firefox_user_js(show_title_bar: bool) -> String {
    // inTitlebar=0 → separate native GNOME title bar.
    // inTitlebar=1 → tabs draw into CSD; collapsing chrome removes the strip.
    let in_titlebar = if show_title_bar { 0 } else { 1 };
    format!(
        r#"// Generated by Mountie — do not remove; re-applied on repair
user_pref("toolkit.legacyUserProfileCustomizations.stylesheets", true);
user_pref("browser.tabs.inTitlebar", {in_titlebar});
/* Disable Firefox vertical tabs / sidebar revamp (seeded from Shared browser). */
user_pref("sidebar.verticalTabs", false);
user_pref("sidebar.revamp", false);
user_pref("sidebar.visibility", "hide");
user_pref("browser.startup.homepage_override.mstone", "ignore");
user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("datareporting.policy.dataSubmissionEnabled", false);
user_pref("toolkit.telemetry.enabled", false);
user_pref("browser.startup.firstrunSkipsHomepage", true);
user_pref("trailhead.firstrun.didSeeAboutWelcome", true);
"#
    )
}

fn firefox_user_chrome(show_title_bar: bool) -> &'static str {
    if show_title_bar {
        FIREFOX_USER_CHROME_WITH_TITLEBAR
    } else {
        FIREFOX_USER_CHROME_FRAMELESS
    }
}

/// Write (or refresh) userChrome.css + user.js for the chosen title-bar mode.
///
/// Returns true if either file was created or content changed.
fn ensure_firefox_app_chrome(profile: &Path, show_title_bar: bool) -> Result<bool> {
    fs::create_dir_all(profile)?;
    let chrome_dir = profile.join("chrome");
    fs::create_dir_all(&chrome_dir)?;

    let mut changed = false;
    let wanted_chrome = firefox_user_chrome(show_title_bar);
    let wanted_js = firefox_user_js(show_title_bar);

    let chrome_path = chrome_dir.join("userChrome.css");
    let current_chrome = fs::read_to_string(&chrome_path).unwrap_or_default();
    if current_chrome != wanted_chrome {
        fs::write(&chrome_path, wanted_chrome)
            .with_context(|| format!("write {}", chrome_path.display()))?;
        changed = true;
    }

    let user_js_path = profile.join("user.js");
    let current_js = fs::read_to_string(&user_js_path).unwrap_or_default();
    if current_js != wanted_js {
        fs::write(&user_js_path, &wanted_js)
            .with_context(|| format!("write {}", user_js_path.display()))?;
        changed = true;
    }

    // Soften prefs.js if present so a live prefs.js does not fight user.js.
    let prefs_path = profile.join("prefs.js");
    if prefs_path.is_file() {
        if let Ok(text) = fs::read_to_string(&prefs_path) {
            let fixed = normalize_firefox_prefs_js(&text, show_title_bar);
            if fixed != text {
                let _ = fs::write(&prefs_path, fixed);
                changed = true;
            }
        }
    }

    Ok(changed)
}

/// Force critical Mountie prefs inside an existing prefs.js blob.
fn normalize_firefox_prefs_js(text: &str, show_title_bar: bool) -> String {
    let in_titlebar = if show_title_bar { 0 } else { 1 };
    let mut lines: Vec<String> = text
        .lines()
        .filter(|line| {
            let t = line.trim();
            // Drop lines we will re-append so we never leave conflicting values.
            !(t.contains("browser.tabs.inTitlebar")
                || t.contains("toolkit.legacyUserProfileCustomizations.stylesheets")
                || t.contains("sidebar.verticalTabs")
                || t.contains("sidebar.revamp")
                || t.contains("sidebar.visibility")
                || t.contains("sidebar.backupState"))
        })
        .map(|l| l.to_string())
        .collect();
    lines.push(r#"user_pref("toolkit.legacyUserProfileCustomizations.stylesheets", true);"#.into());
    lines.push(format!(
        r#"user_pref("browser.tabs.inTitlebar", {in_titlebar});"#
    ));
    lines.push(r#"user_pref("sidebar.verticalTabs", false);"#.into());
    lines.push(r#"user_pref("sidebar.revamp", false);"#.into());
    lines.push(r#"user_pref("sidebar.visibility", "hide");"#.into());
    let mut out = lines.join("\n");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Minimal Firefox profile so the app starts without the profile manager.
fn seed_firefox_profile(profile: &Path, show_title_bar: bool) -> Result<()> {
    fs::create_dir_all(profile)?;
    ensure_firefox_app_chrome(profile, show_title_bar)?;

    // Empty places so Firefox treats this as a valid profile
    let times = profile.join("times.json");
    if !times.exists() {
        fs::write(&times, r#"{"created":0}"#)?;
    }
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

    // Only remove Mountie-owned private profiles — never the browser default profile.
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

    #[test]
    fn origin_matches_whatsapp_and_google_hosts() {
        assert!(origin_matches_host(
            "https+++web.whatsapp.com",
            "web.whatsapp.com"
        ));
        assert!(origin_matches_host(
            "https+++www.whatsapp.com",
            "web.whatsapp.com"
        ));
        assert!(origin_matches_host(
            "https+++flows.whatsapp.net^partitionKey=%28https%2Cwhatsapp.com%29",
            "web.whatsapp.com"
        ));
        assert!(origin_matches_host(
            "https+++accounts.google.com",
            "mail.google.com"
        ));
        assert!(!origin_matches_host(
            "https+++app.notion.com",
            "web.whatsapp.com"
        ));
        assert!(!origin_matches_host(
            "moz-extension+++83b1683e-ad7e-4388-a5d6-ec89dd05df0c",
            "web.whatsapp.com"
        ));
    }

    #[test]
    fn normalize_prefs_forces_in_titlebar() {
        let input = r#"// Mozilla User Preferences
user_pref("browser.tabs.inTitlebar", 0);
user_pref("toolkit.legacyUserProfileCustomizations.stylesheets", false);
user_pref("browser.search.region", "US");
"#;
        let out = normalize_firefox_prefs_js(input, false);
        assert!(out.contains(r#"user_pref("browser.tabs.inTitlebar", 1)"#));
        assert!(out.contains(
            r#"user_pref("toolkit.legacyUserProfileCustomizations.stylesheets", true)"#
        ));
        assert!(!out.contains(r#"user_pref("browser.tabs.inTitlebar", 0)"#));
        assert!(out.contains(r#"user_pref("browser.search.region", "US")"#));

        let with_bar = normalize_firefox_prefs_js(input, true);
        assert!(with_bar.contains(r#"user_pref("browser.tabs.inTitlebar", 0)"#));
    }

    #[test]
    fn ensure_firefox_app_chrome_writes_frameless_and_titled() {
        let tmp = std::env::temp_dir().join(format!(
            "mountie-ff-chrome-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // Simulate a broken Shared profile (native title bar).
        fs::write(
            tmp.join("prefs.js"),
            r#"user_pref("browser.tabs.inTitlebar", 0);
user_pref("toolkit.legacyUserProfileCustomizations.stylesheets", true);
"#,
        )
        .unwrap();

        assert!(ensure_firefox_app_chrome(&tmp, false).unwrap());
        let chrome = fs::read_to_string(tmp.join("chrome/userChrome.css")).unwrap();
        assert!(chrome.contains("#navigator-toolbox"));
        assert!(chrome.contains("#titlebar"));
        assert!(
            chrome.contains("#sidebar-container") && chrome.contains("#vertical-tabs"),
            "must hide Firefox vertical tabs sidebar"
        );
        let user_js = fs::read_to_string(tmp.join("user.js")).unwrap();
        assert!(user_js.contains(r#"user_pref("browser.tabs.inTitlebar", 1)"#));
        assert!(user_js.contains(r#"user_pref("sidebar.verticalTabs", false)"#));
        assert!(user_js.contains(r#"user_pref("sidebar.revamp", false)"#));
        let prefs = fs::read_to_string(tmp.join("prefs.js")).unwrap();
        assert!(prefs.contains(r#"user_pref("browser.tabs.inTitlebar", 1)"#));
        assert!(!prefs.contains(r#"user_pref("browser.tabs.inTitlebar", 0)"#));
        assert!(prefs.contains(r#"user_pref("sidebar.verticalTabs", false)"#));
        assert!(!prefs.contains(r#"user_pref("sidebar.verticalTabs", true)"#));

        // Second call is a no-op when files already match.
        assert!(!ensure_firefox_app_chrome(&tmp, false).unwrap());

        // Switch to show title bar — sidebars still hidden.
        assert!(ensure_firefox_app_chrome(&tmp, true).unwrap());
        let chrome = fs::read_to_string(tmp.join("chrome/userChrome.css")).unwrap();
        assert!(!chrome.contains("#navigator-toolbox {\n  display: none"));
        assert!(chrome.contains("#TabsToolbar"));
        assert!(chrome.contains("#sidebar-container"));
        let user_js = fs::read_to_string(tmp.join("user.js")).unwrap();
        assert!(user_js.contains(r#"user_pref("browser.tabs.inTitlebar", 0)"#));
        assert!(user_js.contains(r#"user_pref("sidebar.verticalTabs", false)"#));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn site_session_detects_substantial_idb() {
        let tmp = std::env::temp_dir().join(format!(
            "mountie-ff-session-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&tmp);
        let origin = tmp
            .join("storage/default")
            .join("https+++web.whatsapp.com");
        fs::create_dir_all(origin.join("idb")).unwrap();
        // Empty stub (~48 KiB) should not count
        fs::write(origin.join("idb/stub.sqlite"), vec![0u8; 48 * 1024]).unwrap();
        assert!(!firefox_profile_has_site_session(
            &tmp,
            "https://web.whatsapp.com/"
        ));
        // Real session blob
        fs::write(origin.join("idb/session.sqlite"), vec![0u8; 200 * 1024]).unwrap();
        assert!(firefox_profile_has_site_session(
            &tmp,
            "https://web.whatsapp.com/"
        ));
        let _ = fs::remove_dir_all(&tmp);
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
            show_title_bar: false,
        })
        .expect("create");

        assert!(entry.path.exists());
        assert!(PathBuf::from(&entry.icon).exists());
        assert_eq!(entry.profile_mode, ProfileMode::Isolated);
        assert!(!entry.show_title_bar);
        let listed = list_webapps().unwrap();
        assert!(listed.iter().any(|a| a.codename == entry.codename));

        let desktop = std::fs::read_to_string(&entry.path).unwrap();
        assert!(desktop.contains("X-WebApp-Manager=mountie"));
        assert!(desktop.contains("X-WebApp-ProfileMode=isolated"));
        assert!(desktop.contains("X-WebApp-ShowTitleBar=false"));
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
            show_title_bar: true,
        })
        .expect("create shared");
        assert_eq!(shared.profile_mode, ProfileMode::Shared);
        assert!(shared.show_title_bar);
        let shared_desktop = std::fs::read_to_string(&shared.path).unwrap();
        assert!(shared_desktop.contains("X-WebApp-ProfileMode=shared"));
        assert!(shared_desktop.contains("X-WebApp-Isolated=false"));
        assert!(shared_desktop.contains("X-WebApp-ShowTitleBar=true"));
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
            show_title_bar: false,
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
            show_title_bar: true,
        })
        .expect("update");
        assert_eq!(updated.codename, codename);
        assert_eq!(updated.name, "ZWM Edited");
        assert!(updated.url.contains("example.org"));
        assert_eq!(updated.profile_mode, ProfileMode::Shared);
        assert!(updated.show_title_bar);
        assert_eq!(updated.path, created.path);
        let desktop = std::fs::read_to_string(&updated.path).unwrap();
        assert!(desktop.contains("Name=ZWM Edited"));
        assert!(desktop.contains("X-WebApp-ProfileMode=shared"));
        assert!(desktop.contains("X-WebApp-ShowTitleBar=true"));
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
