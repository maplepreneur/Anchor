//! XDG paths used by Zorin Web App Manager.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

pub const APP_ID: &str = "com.voxelnorth.ZorinWebAppManager";
pub const MANAGER_TAG: &str = "zorin-webapp-manager";
pub const DESKTOP_PREFIX: &str = "webapp-";

/// `~/.local/share/zorin-webapp-manager`
pub fn data_dir() -> Result<PathBuf> {
    let base = dirs::data_local_dir().context("could not resolve XDG data directory")?;
    Ok(base.join(MANAGER_TAG))
}

pub fn icons_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("icons"))
}

pub fn chromium_profiles_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("profiles"))
}

pub fn firefox_profiles_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("firefox"))
}

/// `~/.local/share/applications`
pub fn applications_dir() -> Result<PathBuf> {
    let base = dirs::data_local_dir().context("could not resolve XDG data directory")?;
    Ok(base.join("applications"))
}

pub fn ensure_dirs() -> Result<()> {
    for dir in [
        data_dir()?,
        icons_dir()?,
        chromium_profiles_dir()?,
        firefox_profiles_dir()?,
        applications_dir()?,
    ] {
        fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create directory {}", dir.display()))?;
    }
    Ok(())
}

pub fn desktop_path(codename: &str) -> Result<PathBuf> {
    Ok(applications_dir()?.join(format!("{DESKTOP_PREFIX}{codename}.desktop")))
}

pub fn icon_path(codename: &str) -> Result<PathBuf> {
    Ok(icons_dir()?.join(format!("{codename}.png")))
}

pub fn chromium_profile_path(codename: &str) -> Result<PathBuf> {
    Ok(chromium_profiles_dir()?.join(codename))
}

pub fn firefox_profile_path(codename: &str) -> Result<PathBuf> {
    Ok(firefox_profiles_dir()?.join(codename))
}
