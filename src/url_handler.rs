use crate::commands::Error;
use crate::track::Track;
use html_escape::decode_html_entities;
use lol_html::{HtmlRewriter, Settings, element, text};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;
static RE: OnceLock<regex::Regex> = OnceLock::new();
static FEAT_RE: OnceLock<regex::Regex> = OnceLock::new();

#[derive(Debug)]
pub struct YouTubeMetadata {
    title: String,
    artist: String,
    album: String,
}

pub async fn handle_url(
    session: &mut crate::session::Session,
    input: &str,
) -> Result<Vec<Track>, Error> {
    let parsed_url = url::Url::parse(input);

    match parsed_url {
        Ok(url) => {
            let domain = url.domain().unwrap_or("");
            let youtube_domains = ["youtube.com", "youtu.be"];

            if youtube_domains.iter().any(|&d| domain.contains(d)) {
                println!("Detected YouTube URL. Extracting metadata...");
                let metadata = extract_youtube_metadata(input).await?;
                Ok(session
                    .find_track_by_details(&metadata.title, &metadata.artist, &metadata.album)
                    .await?
                    .into_iter()
                    .collect())
            } else if domain.contains("tidal.com") {
                println!("Detected Tidal URL. Resolving...");
                match parse_tidal_resource(&url) {
                    Some((TidalResource::Track, id)) => {
                        Ok(vec![session.find_track_by_id(&id).await?])
                    }
                    Some((TidalResource::Playlist, id)) => {
                        session.find_collection_tracks("playlists", &id).await
                    }
                    Some((TidalResource::Album, id)) => {
                        session.find_collection_tracks("albums", &id).await
                    }
                    None => match extract_tidal_info(input).await? {
                        Some(search) => session.find_tracks(&search, 1).await,
                        _ => Ok(Vec::new()),
                    },
                }
            } else {
                println!("Unsupported host: {:?}", url);
                Ok(Vec::new())
            }
        }
        Err(_) => Ok(Vec::new()),
    }
}

enum TidalResource {
    Track,
    Playlist,
    Album,
}

fn parse_tidal_resource(url: &url::Url) -> Option<(TidalResource, String)> {
    let segments = url.path_segments()?.collect::<Vec<_>>();

    for resource_name in ["track", "playlist", "album"] {
        if let Some(index) = segments
            .iter()
            .position(|segment| *segment == resource_name)
        {
            let resource = match resource_name {
                "track" => TidalResource::Track,
                "playlist" => TidalResource::Playlist,
                "album" => TidalResource::Album,
                _ => unreachable!(),
            };

            let id = segments.get(index + 1)?;
            if !id.is_empty() {
                return Some((resource, (*id).to_string()));
            }
        };
    }

    None
}

async fn extract_youtube_metadata(url: &str) -> Result<YouTubeMetadata, Error> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:150.0) Gecko/20100101 Firefox/150.0")
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

    let re = RE.get_or_init(|| regex::Regex::new(r"\s*[\[\(].*?[\]\)]").unwrap());
    let feat_re =
        FEAT_RE.get_or_init(|| regex::Regex::new(r"(?i)\b(feat|ft|featuring)\b.*").unwrap());

    // Remove brackets, remove feat, trim, lowercase
    let clean = |s: &str| -> String {
        let no_brackets = re.replace_all(s, "");
        let no_feat = feat_re.replace_all(&no_brackets, "");
        no_feat.trim().to_lowercase()
    };

    // 1. Attempt to get track details if in "Artist - Song" format
    let video_details = data.pointer(
        "/playerOverlays/playerOverlayRenderer/videoDetails/playerOverlayVideoDetailsRenderer",
    );

    if let Some(v) = video_details {
        let raw_video_title = v
            .pointer("/title/simpleText")
            .and_then(|s| s.as_str())
            .unwrap_or("");

        let clean_video_title = clean(raw_video_title);

        let separators = [" - ", " – ", " — ", " : "];
        for sep in separators {
            if clean_video_title.contains(sep) {
                let parts: Vec<&str> = clean_video_title.splitn(2, sep).collect();
                if parts.len() == 2 {
                    return Ok(YouTubeMetadata {
                        artist: parts[0].trim().to_string(),
                        title: parts[1].trim().to_string(),
                        album: "".to_string(),
                    });
                }
            }
        }
    }

    // 2. Attempt to get track details inside description
    if let Some(panels) = data.get("engagementPanels").and_then(|v| v.as_array()) {
        for panel in panels {
            let model = panel.pointer("/engagementPanelSectionListRenderer/content/structuredDescriptionContentRenderer/items/2/horizontalCardListRenderer/cards/0/videoAttributeViewModel");

            if let Some(m) = model {
                let raw_title = m.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let artist = m
                    .get("subtitle")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_lowercase();

                let title = clean(raw_title);

                if !title.is_empty() && !artist.is_empty() {
                    return Ok(YouTubeMetadata {
                        title,
                        artist,
                        album: m
                            .pointer("/secondarySubtitle/content")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_lowercase(),
                    });
                }
            }
        }
    }

    // 3. Fallback to uploader and video title
    if let Some(v) = video_details {
        let raw_title = v
            .pointer("/title/simpleText")
            .and_then(|s| s.as_str())
            .unwrap_or("");
        let uploader = v
            .pointer("/subtitle/runs/0/text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_lowercase();

        return Ok(YouTubeMetadata {
            title: clean(raw_title),
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
