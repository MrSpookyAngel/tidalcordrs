use crate::commands::Error;
use crate::track::Track;
use html_escape::decode_html_entities;
use lol_html::{HtmlRewriter, Settings, element, text};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug)]
pub struct YouTubeMetadata {
    title: String,
    artist: String,
    album: String,
}

pub async fn handle_url(
    session: &mut crate::session::Session,
    input: &str,
) -> Result<Option<Track>, Error> {
    let parsed_url = url::Url::parse(input);

    match parsed_url {
        Ok(url) => {
            let domain = url.domain().unwrap_or("");
            let youtube_domains = ["youtube.com", "youtu.be"];

            if youtube_domains.iter().any(|&d| domain.contains(d)) {
                println!("Detected YouTube URL. Extracting metadata...");
                let metadata = extract_youtube_metadata(input).await?;
                session
                    .find_track_by_details(&metadata.title, &metadata.artist, &metadata.album)
                    .await
            } else if domain.contains("tidal.com") {
                println!("Detected Tidal URL. Fetching track...");
                match extract_tidal_info(input).await? {
                    Some(search) => {
                        let mut tracks = session.find_tracks(&search, 1).await?;
                        Ok(tracks.pop())
                    }
                    _ => Ok(None),
                }
            } else {
                println!("Unsupported host: {:?}", url);
                Ok(None)
            }
        }
        Err(_) => Ok(None),
    }
}

async fn extract_youtube_metadata(url: &str) -> Result<YouTubeMetadata, Error> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()
        .map_err(|e| Error::from(e.to_string()))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| Error::from(e.to_string()))?
        .text()
        .await
        .map_err(|e| Error::from(e.to_string()))?;

    let script_buffer = Rc::new(RefCell::new(String::new()));
    let json_string = Rc::new(RefCell::new(None));

    let script_buf_handle = Rc::clone(&script_buffer);
    let json_handle = Rc::clone(&json_string);

    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![text!("script", move |t| {
                let mut buf = script_buf_handle.borrow_mut();

                buf.push_str(t.as_str());

                if t.last_in_text_node() {
                    if buf.contains("var ytInitialData =") {
                        if let Some(start) = buf.find("var ytInitialData =") {
                            let json_part = &buf[start + "var ytInitialData =".len()..];
                            let trimmed = json_part.trim().trim_end_matches(';');

                            *json_handle.borrow_mut() = Some(trimmed.to_string());
                        }
                    }
                    buf.clear();
                }
                Ok(())
            })],
            ..Settings::default()
        },
        |_: &[u8]| {},
    );

    rewriter
        .write(resp.as_bytes())
        .map_err(|e| Error::from(e.to_string()))?;
    rewriter.end().map_err(|e| Error::from(e.to_string()))?;

    let raw_json = json_string
        .take()
        .ok_or_else(|| Error::from("Could not find ytInitialData"))?;
    let data: serde_json::Value =
        serde_json::from_str(&raw_json).map_err(|e| Error::from(e.to_string()))?;

    // 1. Attempt to get track details inside description
    if let Some(panels) = data.get("engagementPanels").and_then(|v| v.as_array()) {
        for panel in panels {
            let model = panel.pointer("/engagementPanelSectionListRenderer/content/structuredDescriptionContentRenderer/items/2/horizontalCardListRenderer/cards/0/videoAttributeViewModel");

            if let Some(m) = model {
                return Ok(YouTubeMetadata {
                    title: m
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_lowercase(),
                    artist: m
                        .get("subtitle")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_lowercase(),
                    album: m
                        .pointer("/secondarySubtitle/content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_lowercase(),
                });
            }
        }
    }

    // 2. Fallback to uploader and video title
    let video_details = data.pointer(
        "/playerOverlays/playerOverlayRenderer/videoDetails/playerOverlayVideoDetailsRenderer",
    );
    if let Some(v) = video_details {
        let title = v
            .pointer("/title/simpleText")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_lowercase();
        let uploader = v
            .pointer("/subtitle/runs/0/text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_lowercase();

        return Ok(YouTubeMetadata {
            title,
            artist: uploader,
            album: "".to_string(),
        });
    }

    Err(Error::from("Could not extract valid metadata"))
}

async fn extract_tidal_info(url: &str) -> Result<Option<String>, Error> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:150.0) Gecko/20100101 Firefox/150.0")
        .build()?;

    let resp = client.get(url).send().await?.text().await?;

    let extracted_title = Rc::new(RefCell::new(None));
    let title_handle = Rc::clone(&extracted_title);

    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![element!("meta[property='og:title']", move |el| {
                if let Some(content) = el.get_attribute("content") {
                    let decoded = decode_html_entities(&content).into_owned();
                    *title_handle.borrow_mut() = Some(decoded);
                }
                Ok(())
            })],
            ..Settings::default()
        },
        |_: &[u8]| {},
    );

    rewriter.write(resp.as_bytes())?;
    rewriter.end()?;

    Ok(extracted_title.take())
}
