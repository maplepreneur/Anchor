//! Detect installed browsers and build launch commands (isolated or shared profiles).

use std::path::{Path, PathBuf};

use url::Url;

use crate::paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserFamily {
    Chromium,
    Firefox,
}

/// How a web app uses browser profile data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProfileMode {
    /// Private empty profile under Anchor data dir (default).
    #[default]
    Isolated,
    /// Private profile seeded from the browser (extensions, cookies, logins).
    ///
    /// Always uses a dedicated `--user-data-dir` / Firefox profile so the web app is a
    /// separate process from the main browser. That is required for separate GNOME dock
    /// icons: Chromium shares one process per user-data-dir, so launching `--app` against
    /// the browser’s default profile groups the web app with the browser (Zorin Web App
    /// Manager had the same issue).
    Shared,
}

impl ProfileMode {
    pub fn as_desktop_value(self) -> &'static str {
        match self {
            Self::Isolated => "isolated",
            Self::Shared => "shared",
        }
    }

    pub fn from_desktop_value(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "isolated" => Some(Self::Isolated),
            "shared" => Some(Self::Shared),
            // Legacy: "Isolated with extensions" was removed; those apps keep their
            // private profile and behave as Isolated.
            "isolated-extensions" | "isolated_with_extensions" => Some(Self::Isolated),
            _ => None,
        }
    }

    /// `X-WebApp-Isolated` compatibility flag (false only for Shared).
    pub fn is_isolated(self) -> bool {
        !matches!(self, Self::Shared)
    }

    /// Both modes use a private profile directory so Chromium/Firefox get a separate
    /// process and the dock can show a distinct icon from the main browser.
    pub fn uses_private_profile(self) -> bool {
        true
    }

    /// Whether to copy extensions/cookies/logins from the browser into the private profile.
    pub fn seeds_from_browser(self) -> bool {
        matches!(self, Self::Shared)
    }
}

#[derive(Debug, Clone)]
pub struct Browser {
    pub name: String,
    pub exec_path: PathBuf,
    pub family: BrowserFamily,
}

/// Chromium-on-Wayland ignores `--class` and sets xdg-shell `app_id` to:
/// `{prefix}-{host}__{path_with_slashes_as_underscores}-Default`
///
/// e.g. Brave + `https://www.youtube.com/` → `brave-www.youtube.com__-Default`
///
/// GNOME/Zorin docks match windows via `StartupWMClass`, so it must equal this value.
pub fn chromium_wayland_app_id(browser: &Browser, page_url: &str) -> String {
    let prefix = chromium_app_id_prefix(browser);
    let parsed = Url::parse(page_url).ok();
    let host = parsed
        .as_ref()
        .and_then(|u| u.host_str())
        .unwrap_or("app");
    let path = parsed.as_ref().map(|u| u.path()).unwrap_or("/");
    let path_part = path.trim_start_matches('/').replace('/', "_");
    // Isolated --user-data-dir profiles still use the inner "Default" profile directory.
    format!("{prefix}-{host}__{path_part}-Default")
}

/// Short product prefix Chromium embeds in Wayland `app_id`.
fn chromium_app_id_prefix(browser: &Browser) -> &'static str {
    let path = browser.exec_path.to_string_lossy().to_ascii_lowercase();
    let name = browser.name.to_ascii_lowercase();
    let key = format!("{path} {name}");
    if key.contains("brave") {
        "brave"
    } else if key.contains("msedge") || key.contains("microsoft-edge") || key.contains("edge") {
        // Native Edge tends to use "microsoft-edge"; Flatpak PWAs often use "msedge".
        if key.contains("flatpak") || path.contains("com.microsoft.edge") {
            "msedge"
        } else {
            "microsoft-edge"
        }
    } else if key.contains("vivaldi") {
        "vivaldi"
    } else if key.contains("ungoogled") {
        "chromium"
    } else if key.contains("chromium") {
        "chromium"
    } else if key.contains("chrome") {
        "google-chrome"
    } else {
        "chromium"
    }
}

/// Window class / StartupWMClass for dock matching.
pub fn window_class(browser: &Browser, codename: &str, page_url: &str) -> String {
    match browser.family {
        BrowserFamily::Chromium => chromium_wayland_app_id(browser, page_url),
        BrowserFamily::Firefox => format!("WebApp-{codename}"),
    }
}

/// Candidates checked in preference order (Chromium-family first for best app-mode UX).
fn candidates() -> Vec<(&'static str, &'static str, BrowserFamily)> {
    vec![
        ("Brave", "brave-browser", BrowserFamily::Chromium),
        ("Brave", "brave-browser-stable", BrowserFamily::Chromium),
        ("Brave", "brave", BrowserFamily::Chromium),
        ("Google Chrome", "google-chrome-stable", BrowserFamily::Chromium),
        ("Google Chrome", "google-chrome", BrowserFamily::Chromium),
        ("Chromium", "chromium-browser", BrowserFamily::Chromium),
        ("Chromium", "chromium", BrowserFamily::Chromium),
        ("Microsoft Edge", "microsoft-edge-stable", BrowserFamily::Chromium),
        ("Microsoft Edge", "microsoft-edge", BrowserFamily::Chromium),
        ("Vivaldi", "vivaldi-stable", BrowserFamily::Chromium),
        ("Vivaldi", "vivaldi", BrowserFamily::Chromium),
        ("Firefox", "firefox", BrowserFamily::Firefox),
        ("Firefox ESR", "firefox-esr", BrowserFamily::Firefox),
        // Official Ubuntu/Zorin package and common alternate names
        (
            "Firefox Developer Edition",
            "firefox-devedition",
            BrowserFamily::Firefox,
        ),
        (
            "Firefox Developer Edition",
            "firefox-developer-edition",
            BrowserFamily::Firefox,
        ),
        (
            "Firefox Developer Edition",
            "firefox-dev",
            BrowserFamily::Firefox,
        ),
        (
            "Firefox Developer Edition",
            "/usr/lib/firefox-devedition/firefox",
            BrowserFamily::Firefox,
        ),
        (
            "Firefox Developer Edition",
            "/usr/lib/firefox-devedition/firefox-bin",
            BrowserFamily::Firefox,
        ),
        ("LibreWolf", "librewolf", BrowserFamily::Firefox),
        ("Floorp", "floorp", BrowserFamily::Firefox),
        ("Waterfox", "waterfox", BrowserFamily::Firefox),
        ("Zen Browser", "zen-browser", BrowserFamily::Firefox),
        ("Zen Browser", "zen", BrowserFamily::Firefox),
    ]
}

fn flatpak_candidates() -> Vec<(&'static str, &'static str, BrowserFamily)> {
    vec![
        (
            "Brave (Flatpak)",
            "com.brave.Browser",
            BrowserFamily::Chromium,
        ),
        (
            "Chrome (Flatpak)",
            "com.google.Chrome",
            BrowserFamily::Chromium,
        ),
        (
            "Chromium (Flatpak)",
            "org.chromium.Chromium",
            BrowserFamily::Chromium,
        ),
        (
            "Edge (Flatpak)",
            "com.microsoft.Edge",
            BrowserFamily::Chromium,
        ),
        (
            "Firefox (Flatpak)",
            "org.mozilla.firefox",
            BrowserFamily::Firefox,
        ),
        (
            "LibreWolf (Flatpak)",
            "io.gitlab.librewolf-community",
            BrowserFamily::Firefox,
        ),
        (
            "Zen (Flatpak)",
            "app.zen_browser.zen",
            BrowserFamily::Firefox,
        ),
    ]
}

fn is_executable(path: &Path) -> bool {
    path.is_file()
        && std::fs::metadata(path)
            .map(|m| {
                use std::os::unix::fs::PermissionsExt;
                m.permissions().mode() & 0o111 != 0
            })
            .unwrap_or(false)
}

fn which(bin: &str) -> Option<PathBuf> {
    // Absolute path
    if bin.starts_with('/') {
        let p = PathBuf::from(bin);
        return is_executable(&p).then_some(p);
    }

    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(bin);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }

    // Common absolute fallbacks
    for prefix in ["/usr/bin", "/usr/local/bin", "/snap/bin"] {
        let candidate = PathBuf::from(prefix).join(bin);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }

    // Only look inside Firefox Developer Edition install dirs for devedition-related names
    // (otherwise plain "firefox" would incorrectly resolve to the Dev Edition binary).
    let bin_l = bin.to_ascii_lowercase();
    if bin_l.contains("devedition")
        || bin_l.contains("developer")
        || bin_l == "firefox-dev"
        || bin_l == "firefox-bin"
    {
        for prefix in [
            "/usr/lib/firefox-devedition",
            "/usr/lib/firefox-developer-edition",
            "/opt/firefox-devedition",
        ] {
            let candidate = PathBuf::from(prefix).join(bin);
            if is_executable(&candidate) {
                return Some(candidate);
            }
            // Also try the well-known binaries in that prefix
            for name in ["firefox-devedition", "firefox-bin", "firefox"] {
                let candidate = PathBuf::from(prefix).join(name);
                if is_executable(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// First token of a desktop `Exec=` line (handles quoting).
fn exec_first_token(exec: &str) -> Option<String> {
    let exec = exec.trim();
    if exec.is_empty() {
        return None;
    }
    if exec.starts_with('"') {
        let rest = &exec[1..];
        let end = rest.find('"')?;
        return Some(rest[..end].to_string());
    }
    Some(
        exec.split_whitespace()
            .next()?
            .trim_matches('"')
            .to_string(),
    )
}

fn family_from_name_and_path(name: &str, path: &Path) -> BrowserFamily {
    let key = format!(
        "{} {}",
        name.to_ascii_lowercase(),
        path.to_string_lossy().to_ascii_lowercase()
    );
    if key.contains("firefox")
        || key.contains("librewolf")
        || key.contains("waterfox")
        || key.contains("floorp")
        || key.contains("zen")
        || key.contains("devedition")
        || key.contains("developer")
    {
        BrowserFamily::Firefox
    } else {
        BrowserFamily::Chromium
    }
}

fn find_desktop_file(desktop_id: &str) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(home) = dirs::data_local_dir() {
        candidates.push(home.join("applications").join(desktop_id));
    }
    candidates.push(PathBuf::from("/usr/share/applications").join(desktop_id));
    candidates.push(PathBuf::from("/usr/local/share/applications").join(desktop_id));
    candidates
        .into_iter()
        .find(|p| p.is_file())
}

fn parse_desktop_name_and_exec(path: &Path) -> Option<(String, String)> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut name = None;
    let mut exec = None;
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line == "[Desktop Entry]" {
            in_entry = true;
            continue;
        }
        if line.starts_with('[') {
            // Only use the primary Desktop Entry group
            if in_entry {
                break;
            }
            continue;
        }
        if !in_entry && name.is_none() && exec.is_none() {
            // Some files omit explicit section checks; still parse keys
            in_entry = true;
        }
        if let Some(rest) = line.strip_prefix("Name=") {
            // Prefer the first non-localized Name=
            if name.is_none() && !rest.contains('[') {
                name = Some(rest.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("Exec=") {
            if exec.is_none() {
                exec = Some(rest.to_string());
            }
        }
    }
    Some((name.unwrap_or_else(|| "Browser".into()), exec?))
}

/// Resolve the user's default web browser via `xdg-settings` / desktop files.
pub fn resolve_default_browser() -> Option<Browser> {
    let output = std::process::Command::new("xdg-settings")
        .args(["get", "default-web-browser"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let desktop_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if desktop_id.is_empty() {
        return None;
    }

    let desktop_path = find_desktop_file(&desktop_id)?;
    let (pretty_name, exec_line) = parse_desktop_name_and_exec(&desktop_path)?;
    let token = exec_first_token(&exec_line)?;
    let exec_path = if token.contains('/') {
        let p = PathBuf::from(&token);
        if is_executable(&p) {
            p
        } else {
            which(&token)?
        }
    } else {
        which(&token)?
    };

    let family = family_from_name_and_path(&pretty_name, &exec_path);
    Some(Browser {
        // Store a stable label so repair can still match by name if needed
        name: format!("Default browser ({pretty_name})"),
        exec_path,
        family,
    })
}

fn flatpak_export_path(app_id: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    for base in [
        home.join(".local/share/flatpak/exports/bin"),
        PathBuf::from("/var/lib/flatpak/exports/bin"),
    ] {
        let p = base.join(app_id);
        if is_executable(&p) {
            return Some(p);
        }
    }
    None
}

/// Return installed browsers, de-duplicated by display name (first match wins).
///
/// The system default browser (if resolvable) is always listed first as
/// `Default browser (…)` so it is easy to pick without hunting for it.
pub fn detect_browsers() -> Vec<Browser> {
    let mut found: Vec<Browser> = Vec::new();
    let mut seen_names = std::collections::HashSet::new();
    let mut seen_paths = std::collections::HashSet::new();

    // Prefer default browser at the top of the list.
    if let Some(default) = resolve_default_browser() {
        seen_paths.insert(default.exec_path.clone());
        seen_names.insert(default.name.clone());
        found.push(default);
    }

    for (name, bin, family) in candidates() {
        if let Some(exec_path) = which(bin) {
            // Skip if this exact binary is already the default entry (avoid near-dupes).
            // Still allow listing Firefox Developer Edition separately from "Default browser (…)".
            if !seen_names.insert(name.to_string()) {
                continue;
            }
            seen_paths.insert(exec_path.clone());
            found.push(Browser {
                name: name.to_string(),
                exec_path,
                family,
            });
        }
    }

    for (name, app_id, family) in flatpak_candidates() {
        if let Some(exec_path) = flatpak_export_path(app_id) {
            if !seen_names.insert(name.to_string()) {
                continue;
            }
            seen_paths.insert(exec_path.clone());
            found.push(Browser {
                name: name.to_string(),
                exec_path,
                family,
            });
        }
    }

    let _ = seen_paths; // reserved for future path-based de-dupe
    found
}

/// Build the `Exec=` line for a `.desktop` file.
pub fn build_exec(
    browser: &Browser,
    codename: &str,
    url: &str,
    icon_path: &Path,
    profile_mode: ProfileMode,
) -> anyhow::Result<String> {
    let class = window_class(browser, codename, url);
    let exec = browser.exec_path.display().to_string();
    // Quote paths that may contain spaces
    let exec_q = shell_quote(&exec);
    let url_q = shell_quote(url);
    let icon_q = shell_quote(&icon_path.display().to_string());
    let class_q = shell_quote(&class);

    match browser.family {
        BrowserFamily::Chromium => {
            // --class is honored on X11; on Wayland Chromium derives app_id from the URL
            // (see chromium_wayland_app_id). We still pass matching --class/--name for X11.
            let base = format!("{exec_q} --app={url_q} --class={class_q} --name={class_q}");
            // Always use a private user-data-dir so this web app is not the browser’s
            // singleton process (required for a separate dock icon on GNOME/Wayland).
            let _ = profile_mode; // both modes use private dirs; Shared is seeded separately
            let profile = paths::chromium_profile_path(codename)?;
            let profile_q = shell_quote(&profile.display().to_string());
            Ok(format!("{base} --user-data-dir={profile_q}"))
        }
        BrowserFamily::Firefox => {
            // Wrap in sh -c so we can set XAPP_FORCE_GTKWINDOW_ICON for better icon behavior
            // on some desktops. Single-quoted inside so desktop file parsing is safe.
            // Private --profile keeps the web app out of the default-profile singleton.
            let _ = profile_mode;
            let profile = paths::firefox_profile_path(codename)?;
            let profile_q = shell_quote(&profile.display().to_string());
            Ok(format!(
                "sh -c 'XAPP_FORCE_GTKWINDOW_ICON={icon_q} {exec_q} --class {class_q} --name {class_q} --profile {profile_q} --no-remote {url_q}'"
            ))
        }
    }
}

/// Resolve the browser’s default Chromium user-data directory (native or Flatpak).
pub fn chromium_default_user_data_dir(browser: &Browser) -> Option<PathBuf> {
    if browser.family != BrowserFamily::Chromium {
        return None;
    }
    let home = dirs::home_dir()?;
    let config = dirs::config_dir()?;
    let path = browser.exec_path.to_string_lossy().to_ascii_lowercase();
    let name = browser.name.to_ascii_lowercase();
    let key = format!("{path} {name}");

    let flatpak_id = if key.contains("com.brave.browser") || path.contains("com.brave.browser") {
        Some(("com.brave.Browser", "BraveSoftware/Brave-Browser"))
    } else if key.contains("com.google.chrome") || path.contains("com.google.chrome") {
        Some(("com.google.Chrome", "google-chrome"))
    } else if key.contains("org.chromium.chromium") || path.contains("org.chromium.chromium") {
        Some(("org.chromium.Chromium", "chromium"))
    } else if key.contains("com.microsoft.edge") || path.contains("com.microsoft.edge") {
        Some(("com.microsoft.Edge", "microsoft-edge"))
    } else {
        None
    };

    if let Some((app_id, rel)) = flatpak_id {
        let p = home.join(".var/app").join(app_id).join("config").join(rel);
        if p.is_dir() {
            return Some(p);
        }
    }

    let native: PathBuf = if key.contains("brave") {
        config.join("BraveSoftware/Brave-Browser")
    } else if key.contains("msedge") || key.contains("microsoft-edge") || key.contains("edge") {
        config.join("microsoft-edge")
    } else if key.contains("vivaldi") {
        config.join("vivaldi")
    } else if key.contains("ungoogled") || key.contains("chromium") {
        config.join("chromium")
    } else if key.contains("chrome") {
        config.join("google-chrome")
    } else {
        config.join("chromium")
    };

    if native.is_dir() {
        Some(native)
    } else {
        None
    }
}

/// Resolve the Firefox-family default profile directory via profiles.ini when possible.
pub fn firefox_default_profile_dir(browser: &Browser) -> Option<PathBuf> {
    if browser.family != BrowserFamily::Firefox {
        return None;
    }
    let home = dirs::home_dir()?;
    let path = browser.exec_path.to_string_lossy().to_ascii_lowercase();
    let name = browser.name.to_ascii_lowercase();
    let key = format!("{path} {name}");

    let mut roots = Vec::new();
    if key.contains("librewolf") {
        roots.push(home.join(".librewolf"));
        roots.push(home.join(".var/app/io.gitlab.librewolf-community/.librewolf"));
    } else if key.contains("zen") {
        roots.push(home.join(".zen"));
        roots.push(home.join(".var/app/app.zen_browser.zen/.zen"));
    } else if key.contains("floorp") {
        roots.push(home.join(".floorp"));
    } else if key.contains("waterfox") {
        roots.push(home.join(".waterfox"));
    } else {
        roots.push(home.join(".mozilla/firefox"));
        roots.push(home.join(".var/app/org.mozilla.firefox/.mozilla/firefox"));
    }

    for root in roots {
        if let Some(p) = parse_firefox_default_profile(&root) {
            return Some(p);
        }
    }
    None
}

fn parse_firefox_default_profile(profiles_root: &Path) -> Option<PathBuf> {
    let ini = profiles_root.join("profiles.ini");
    let text = std::fs::read_to_string(ini).ok()?;
    let mut best: Option<(i32, PathBuf)> = None;
    let mut section_path: Option<String> = None;
    let mut section_default = false;
    let mut section_relative = true;
    let mut section_name_is_default = false;

    let flush = |best: &mut Option<(i32, PathBuf)>,
                 path: &Option<String>,
                 is_default: bool,
                 relative: bool,
                 name_default: bool,
                 root: &Path| {
        let Some(p) = path else { return };
        let full = if relative {
            root.join(p)
        } else {
            PathBuf::from(p)
        };
        if !full.is_dir() {
            return;
        }
        let score = if is_default {
            3
        } else if name_default {
            2
        } else {
            1
        };
        match best {
            None => *best = Some((score, full)),
            Some((s, _)) if score > *s => *best = Some((score, full)),
            _ => {}
        }
    };

    for line in text.lines().chain(std::iter::once("")) {
        let line = line.trim();
        if line.starts_with('[') || line.is_empty() {
            flush(
                &mut best,
                &section_path,
                section_default,
                section_relative,
                section_name_is_default,
                profiles_root,
            );
            section_path = None;
            section_default = false;
            section_relative = true;
            section_name_is_default = false;
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                "Path" => section_path = Some(v.trim().to_string()),
                "Default" => section_default = v.trim() == "1",
                "IsRelative" => section_relative = v.trim() != "0",
                "Name" => {
                    section_name_is_default = v.to_ascii_lowercase().contains("default");
                }
                _ => {}
            }
        }
    }

    best.map(|(_, p)| p)
}

/// Quote a value for use inside a desktop `Exec=` key / shell.
fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "\"\"".to_string();
    }
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_./:@?&=+%#".contains(c))
    {
        return s.to_string();
    }
    format!("\"{}\"", s.replace('"', "\\\"").replace('$', "\\$"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brave() -> Browser {
        Browser {
            name: "Brave".into(),
            exec_path: PathBuf::from("/usr/bin/brave-browser"),
            family: BrowserFamily::Chromium,
        }
    }

    #[test]
    fn wayland_app_id_youtube() {
        assert_eq!(
            chromium_wayland_app_id(&brave(), "https://www.youtube.com/"),
            "brave-www.youtube.com__-Default"
        );
        assert_eq!(
            chromium_wayland_app_id(&brave(), "https://www.youtube.com"),
            "brave-www.youtube.com__-Default"
        );
    }

    #[test]
    fn wayland_app_id_with_path() {
        assert_eq!(
            chromium_wayland_app_id(&brave(), "https://github.com/basecamp/omarchy"),
            "brave-github.com__basecamp_omarchy-Default"
        );
        assert_eq!(
            chromium_wayland_app_id(&brave(), "http://example.com/foo/bar"),
            "brave-example.com__foo_bar-Default"
        );
    }

    #[test]
    fn chromium_exec_has_isolation_flags() {
        let browser = brave();
        let exec = build_exec(
            &browser,
            "YouTube1234",
            "https://www.youtube.com/",
            Path::new("/tmp/icon.png"),
            ProfileMode::Isolated,
        )
        .unwrap();
        assert!(exec.contains("--app="));
        assert!(exec.contains("--user-data-dir="));
        assert!(exec.contains("--class=brave-www.youtube.com__-Default"));
    }

    #[test]
    fn chromium_shared_exec_uses_private_user_data_dir() {
        let browser = brave();
        let exec = build_exec(
            &browser,
            "YouTube1234",
            "https://www.youtube.com/",
            Path::new("/tmp/icon.png"),
            ProfileMode::Shared,
        )
        .unwrap();
        assert!(exec.contains("--app="));
        // Shared still uses a private user-data-dir so the dock icon is not the browser’s.
        assert!(exec.contains("--user-data-dir="));
        assert!(exec.contains("--class=brave-www.youtube.com__-Default"));
    }

    #[test]
    fn firefox_exec_has_isolation_flags() {
        let browser = Browser {
            name: "Firefox".into(),
            exec_path: PathBuf::from("/usr/bin/firefox"),
            family: BrowserFamily::Firefox,
        };
        let exec = build_exec(
            &browser,
            "Maps5678",
            "https://maps.google.com",
            Path::new("/tmp/icon.png"),
            ProfileMode::Isolated,
        )
        .unwrap();
        assert!(exec.contains("--no-remote"));
        assert!(exec.contains("--profile"));
        assert!(exec.contains("WebApp-Maps5678"));
    }

    #[test]
    fn firefox_shared_exec_uses_private_profile() {
        let browser = Browser {
            name: "Firefox".into(),
            exec_path: PathBuf::from("/usr/bin/firefox"),
            family: BrowserFamily::Firefox,
        };
        let exec = build_exec(
            &browser,
            "Maps5678",
            "https://maps.google.com",
            Path::new("/tmp/icon.png"),
            ProfileMode::Shared,
        )
        .unwrap();
        assert!(exec.contains("--profile"));
        assert!(exec.contains("--no-remote"));
        assert!(exec.contains("WebApp-Maps5678"));
    }

    #[test]
    fn profile_mode_desktop_roundtrip() {
        for mode in [ProfileMode::Isolated, ProfileMode::Shared] {
            assert_eq!(
                ProfileMode::from_desktop_value(mode.as_desktop_value()),
                Some(mode)
            );
        }
        // Legacy isolated-with-extensions maps to Isolated
        assert_eq!(
            ProfileMode::from_desktop_value("isolated-extensions"),
            Some(ProfileMode::Isolated)
        );
        assert_eq!(
            ProfileMode::from_desktop_value("isolated_with_extensions"),
            Some(ProfileMode::Isolated)
        );
    }

    #[test]
    fn exec_first_token_handles_path_and_args() {
        assert_eq!(
            exec_first_token("/usr/lib/firefox-devedition/firefox-bin %u").as_deref(),
            Some("/usr/lib/firefox-devedition/firefox-bin")
        );
        assert_eq!(
            exec_first_token("\"/opt/Brave Software/brave\" --app=%u").as_deref(),
            Some("/opt/Brave Software/brave")
        );
    }

    #[test]
    fn family_detects_firefox_devedition() {
        assert_eq!(
            family_from_name_and_path(
                "Firefox Developer Edition",
                Path::new("/usr/lib/firefox-devedition/firefox-bin")
            ),
            BrowserFamily::Firefox
        );
    }
}

#[cfg(test)]
mod detect_smoke {
    use super::*;
    #[test]
    fn lists_default_and_firefox_devedition() {
        let browsers = detect_browsers();
        for b in &browsers {
            eprintln!("  - {} => {} ({:?})", b.name, b.exec_path.display(), b.family);
        }
        assert!(
            browsers.iter().any(|b| b.name.starts_with("Default browser")),
            "missing default browser entry: {browsers:?}"
        );
        assert!(
            browsers.iter().any(|b| b.name == "Firefox Developer Edition"),
            "missing Firefox Developer Edition: {browsers:?}"
        );
        // Default should be first
        assert!(
            browsers[0].name.starts_with("Default browser"),
            "default not first: {}",
            browsers[0].name
        );
    }
}
