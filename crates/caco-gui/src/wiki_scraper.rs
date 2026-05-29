//! Doom Wiki image scraping via MediaWiki API.
//!
//! For WADs without local files (e.g., doomwiki imports), tries to fetch
//! a title screen image from the WAD's Doom Wiki page.

use reqwest::blocking::Client;
use serde::Deserialize;

const API_URL: &str = "https://doomwiki.org/w/api.php";
const USER_AGENT: &str = "Caco/1.0 (Doom WAD library manager)";

fn client() -> Option<Client> {
    Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .ok()
}

/// Fetch a title screen image from a known Doom Wiki page URL.
///
/// Returns raw image bytes if found.
pub fn fetch_wiki_image(wiki_url: &str) -> Option<Vec<u8>> {
    let page_title = wiki_url.split("/wiki/").last()?;
    if page_title.is_empty() {
        return None;
    }
    let client = client()?;
    fetch_image_for_page(&client, page_title)
}

/// Search the Doom Wiki for a WAD by title and fetch its page image.
///
/// Uses MediaWiki's opensearch API to find a matching page.
pub fn search_wiki_image(title: &str) -> Option<Vec<u8>> {
    if title.is_empty() {
        return None;
    }

    let client = client()?;

    // MediaWiki opensearch: returns [search_term, [titles], [descriptions], [urls]]
    let resp = client
        .get(API_URL)
        .query(&[
            ("action", "opensearch"),
            ("search", title),
            ("limit", "5"),
            ("namespace", "0"),
            ("format", "json"),
        ])
        .send()
        .ok()?;

    let data: serde_json::Value = resp.json().ok()?;
    let titles = data.get(1)?.as_array()?;

    for page_title in titles {
        let page_title = page_title.as_str()?;
        // opensearch ranks loosely; an unrelated page's screenshot is worse
        // than no thumbnail at all. Only follow results that actually match
        // the WAD title.
        if !titles_match(title, page_title) {
            continue;
        }
        if let Some(result) = fetch_image_for_page(&client, page_title) {
            return Some(result);
        }
    }

    None
}

/// Loose-but-not-fuzzy title match: collapse both to lowercase alphanumerics
/// and require one to contain the other. Guards against opensearch linking a
/// WAD to an unrelated page (and serving that page's screenshot as the
/// thumbnail).
fn titles_match(a: &str, b: &str) -> bool {
    fn norm(s: &str) -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    }
    let (a, b) = (norm(a), norm(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || a.contains(&b) || b.contains(&a)
}

/// True for wiki chrome, award badges, and boilerplate that aren't a
/// screenshot of the WAD itself.
///
/// The Cacoward/Mordeth trophy badges are the worst offenders: they appear on
/// every award-winning WAD's page, so the "first image on the page" fallback
/// was grabbing the gold Cacoward statue as the thumbnail for cacoward
/// winners.
fn is_chrome_image(name_lower: &str) -> bool {
    const DENY: &[&str] = &[
        "cacoward",
        "mordeth",
        "espi award",
        "icon",
        "logo",
        "stub",
        "ambox",
        "commons",
        "magnify",
        "disambig",
        "padlock",
        "nuvola",
        "crystal",
        "emblem",
        "wiki.png",
        "button",
        "symbol",
        "placeholder",
    ];
    DENY.iter().any(|d| name_lower.contains(d))
}

/// Core logic: fetch a title screen image from a Doom Wiki page.
fn fetch_image_for_page(client: &Client, page_title: &str) -> Option<Vec<u8>> {
    // Step 1: Get images on the page
    let resp = client
        .get(API_URL)
        .query(&[
            ("action", "query"),
            ("titles", page_title),
            ("prop", "images"),
            ("imlimit", "50"),
            ("format", "json"),
        ])
        .send()
        .ok()?;

    let data: QueryResponse = resp.json().ok()?;
    let pages = data.query?.pages;
    let page = pages.into_values().next()?;

    if page.missing.is_some() {
        return None;
    }

    let images = page.images.unwrap_or_default();

    // Step 2: Filter for title screen images
    let title_keywords = ["titlepic", "title screen", "title.png", "title.jpg"];
    let mut title_images: Vec<&str> = Vec::new();

    for img in &images {
        let name_lower = img.title.to_lowercase();
        if is_chrome_image(&name_lower) {
            continue;
        }
        if title_keywords.iter().any(|k| name_lower.contains(k)) {
            title_images.push(&img.title);
        }
    }

    // Fallback: first .png/.jpg image that isn't wiki chrome / an award badge.
    if title_images.is_empty() {
        for img in &images {
            let name_lower = img.title.to_lowercase();
            if is_chrome_image(&name_lower) {
                continue;
            }
            if name_lower.ends_with(".png")
                || name_lower.ends_with(".jpg")
                || name_lower.ends_with(".jpeg")
                || name_lower.ends_with(".gif")
            {
                title_images.push(&img.title);
                break;
            }
        }
    }

    let image_title = title_images.first()?;

    // Step 3: Get direct URL for the image
    let resp = client
        .get(API_URL)
        .query(&[
            ("action", "query"),
            ("titles", image_title),
            ("prop", "imageinfo"),
            ("iiprop", "url"),
            ("format", "json"),
        ])
        .send()
        .ok()?;

    let data: QueryResponse = resp.json().ok()?;
    let pages = data.query?.pages;
    let page = pages.into_values().next()?;

    let imageinfo = page.imageinfo?;
    let info = imageinfo.first()?;
    let image_url = info.url.as_deref()?;

    // Step 4: Download the image
    let resp = client.get(image_url).send().ok()?;

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if content_type.starts_with("image/") {
        resp.bytes().ok().map(|b| b.to_vec())
    } else {
        None
    }
}

// MediaWiki API response types (minimal, just what we need)

#[derive(Deserialize)]
struct QueryResponse {
    query: Option<QueryData>,
}

#[derive(Deserialize)]
struct QueryData {
    pages: std::collections::HashMap<String, PageData>,
}

#[derive(Deserialize)]
struct PageData {
    missing: Option<serde_json::Value>,
    images: Option<Vec<ImageRef>>,
    imageinfo: Option<Vec<ImageInfo>>,
}

#[derive(Deserialize)]
struct ImageRef {
    title: String,
}

#[derive(Deserialize)]
struct ImageInfo {
    url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_images_rejected() {
        // The gold trophy badge on every award-winning WAD's page.
        assert!(is_chrome_image("file:cacoward.png"));
        assert!(is_chrome_image("file:cacoward 2022.png"));
        assert!(is_chrome_image("file:mordeth award.png"));
        assert!(is_chrome_image("file:doomwiki_logo.png"));
        assert!(is_chrome_image("file:wiki.png"));
    }

    #[test]
    fn real_screenshots_accepted() {
        assert!(!is_chrome_image("file:hardfest3 titlepic.png"));
        assert!(!is_chrome_image("file:sunder map01.png"));
        assert!(!is_chrome_image("file:eviternity_screenshot.jpg"));
    }

    #[test]
    fn titles_match_ignores_case_punctuation_and_spacing() {
        assert!(titles_match(
            "The Smoking Dog [part2]",
            "The Smoking Dog [part2]"
        ));
        assert!(titles_match("Eviternity", "eviternity!"));
        assert!(titles_match("Ancient Aliens", "Ancient   Aliens"));
        // Substring containment is allowed (subtitles, episode suffixes).
        assert!(titles_match("Sunder", "Sunder (megawad)"));
    }

    #[test]
    fn titles_match_rejects_unrelated_pages() {
        assert!(!titles_match("Infernal Odyssey", "Doom II"));
        assert!(!titles_match("The Smoking Dog [part2]", "Plutonia"));
        assert!(!titles_match("Hardfest 3", ""));
    }
}
