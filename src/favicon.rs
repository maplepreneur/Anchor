//! Fetch and normalize a site favicon to a PNG icon.

use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use image::imageops::FilterType;
use image::ImageReader;
use scraper::{Html, Selector};
use url::Url;

const USER_AGENT: &str = concat!(
    "Mountie/0.1 (+https://github.com/maplepreneur/Mountie; ",
    "favicon fetcher)"
);
const ICON_SIZE: u32 = 128;

/// Ensure the URL has a scheme; default to https.
pub fn normalize_url(input: &str) -> Result<Url> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("URL is empty"));
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    Url::parse(&with_scheme).context("invalid URL")
}

fn client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent(USER_AGENT)
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .context("failed to build HTTP client")
}

/// Download image bytes from `link`, resolving relative URLs against `base`.
fn fetch_bytes(client: &reqwest::blocking::Client, base: &Url, link: &str) -> Result<Vec<u8>> {
    let abs = if link.contains("://") {
        Url::parse(link)?
    } else {
        base.join(link)?
    };
    let resp = client
        .get(abs)
        .send()
        .context("request failed")?
        .error_for_status()
        .context("HTTP error")?;
    let bytes = resp.bytes().context("read body")?.to_vec();
    if bytes.is_empty() {
        return Err(anyhow!("empty image body"));
    }
    Ok(bytes)
}

/// Decode arbitrary image bytes and write a 128×128 PNG to `dest`.
pub fn bytes_to_png_file(bytes: &[u8], dest: &Path) -> Result<()> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .context("guess image format")?;
    let img = reader.decode().context("decode image")?;
    let rgba = img.into_rgba8();
    let resized = image::imageops::resize(&rgba, ICON_SIZE, ICON_SIZE, FilterType::Lanczos3);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    resized
        .save_with_format(dest, image::ImageFormat::Png)
        .context("save PNG")?;
    Ok(())
}

/// Copy a local image file to a standardized PNG icon at `dest`.
pub fn local_image_to_png(src: &Path, dest: &Path) -> Result<()> {
    let bytes = std::fs::read(src).with_context(|| format!("read {}", src.display()))?;
    bytes_to_png_file(&bytes, dest)
}

fn collect_html_icon_links(html: &str) -> Vec<String> {
    let document = Html::parse_document(html);
    let mut links = Vec::new();

    let rel_selectors = [
        r#"link[rel="apple-touch-icon"]"#,
        r#"link[rel="apple-touch-icon-precomposed"]"#,
        r#"link[rel="icon"]"#,
        r#"link[rel="shortcut icon"]"#,
    ];
    for sel in rel_selectors {
        if let Ok(selector) = Selector::parse(sel) {
            for el in document.select(&selector) {
                if let Some(href) = el.value().attr("href") {
                    links.push(href.to_string());
                }
            }
        }
    }

    if let Ok(selector) = Selector::parse(r#"meta[property="og:image"]"#) {
        for el in document.select(&selector) {
            if let Some(content) = el.value().attr("content") {
                links.push(content.to_string());
            }
        }
    }

    links
}

/// Try hard to obtain a favicon for `page_url` and write it as PNG to `dest`.
///
/// Returns `Ok(())` on success, `Err` if every strategy fails.
pub fn fetch_favicon(page_url: &str, dest: &Path) -> Result<()> {
    let url = normalize_url(page_url)?;
    let client = client()?;
    let origin = Url::parse(&format!(
        "{}://{}",
        url.scheme(),
        url.host_str().unwrap_or("localhost")
    ))?;

    let mut candidates: Vec<String> = Vec::new();

    // 1) Parse the page HTML for icon links
    if let Ok(resp) = client.get(url.clone()).send() {
        if let Ok(resp) = resp.error_for_status() {
            if let Ok(body) = resp.text() {
                candidates.extend(collect_html_icon_links(&body));
            }
        }
    }

    // 2) Conventional /favicon.ico
    candidates.push("/favicon.ico".to_string());

    // 3) Google favicon service (Omarchy-style)
    if let Some(host) = url.host_str() {
        candidates.push(format!(
            "https://www.google.com/s2/favicons?domain={host}&sz=128"
        ));
    }

    let mut last_err = anyhow!("no favicon candidates");
    for link in candidates {
        match fetch_bytes(&client, &origin, &link) {
            Ok(bytes) => match bytes_to_png_file(&bytes, dest) {
                Ok(()) => return Ok(()),
                Err(e) => last_err = e,
            },
            Err(e) => last_err = e,
        }
    }

    Err(last_err.context("could not fetch a usable favicon"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_adds_https() {
        let u = normalize_url("example.com/path").unwrap();
        assert_eq!(u.scheme(), "https");
        assert_eq!(u.host_str(), Some("example.com"));
    }

    #[test]
    fn normalize_keeps_scheme() {
        let u = normalize_url("http://example.com").unwrap();
        assert_eq!(u.scheme(), "http");
    }

    #[test]
    fn png_roundtrip_from_minimal() {
        use image::{ImageBuffer, Rgba};
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_fn(8, 8, |_, _| Rgba([255, 0, 0, 255]));
        let mut png = Vec::new();
        img.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let dir = std::env::temp_dir().join("mountie-favicon-test");
        let _ = std::fs::create_dir_all(&dir);
        let dest = dir.join("out.png");
        bytes_to_png_file(&png, &dest).unwrap();
        assert!(dest.is_file());
        let meta = std::fs::metadata(&dest).unwrap();
        assert!(meta.len() > 0);
    }
}
