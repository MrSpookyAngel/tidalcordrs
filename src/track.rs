use crate::commands::Error;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum TidalTrackId {
    Number(u64),
    String(String),
}

impl std::fmt::Display for TidalTrackId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(id) => write!(f, "{id}"),
            Self::String(id) => f.write_str(id),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TidalArtist {
    name: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TidalTrackResponse {
    id: TidalTrackId,
    title: String,
    artists: Vec<TidalArtist>,
    duration: u32,
    #[serde(default, rename = "type")]
    item_type: Option<String>,
}

impl TidalTrackResponse {
    pub fn id(&self) -> String {
        self.id.to_string()
    }

    pub fn is_video(&self) -> bool {
        self.item_type
            .as_deref()
            .is_some_and(|item_type| item_type.eq_ignore_ascii_case("video"))
    }
}

#[derive(Debug, serde::Deserialize)]
struct StreamUrlResponse {
    urls: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub featured_artists: Vec<String>, // Can be empty
    pub duration: u32,
    pub stream_url: String,
}

impl Track {
    pub async fn from_track_id(
        session: &crate::session::Session,
        track_response: &TidalTrackResponse,
    ) -> Result<Self, Error> {
        Self::from_track_response(
            &session.client,
            &session.session_id,
            &session.country_code,
            track_response,
        )
        .await
    }

    pub async fn from_track_response(
        client: &reqwest::Client,
        session_id: &str,
        country_code: &str,
        track_response: &TidalTrackResponse,
    ) -> Result<Self, Error> {
        let track_id = track_response.id();

        let url = format!(
            "https://api.tidal.com/v1/tracks/{}/urlpostpaywall",
            track_id
        );

        let params = [
            ("sessionId", session_id),
            ("countryCode", country_code),
            ("urlusagemode", "STREAM"),
            ("audioquality", "HIGH"),
            ("assetpresentation", "FULL"),
        ];

        let response = client
            .get(&url)
            .query(&params)
            .send()
            .await?
            .error_for_status()?;

        let stream_response = response.json::<StreamUrlResponse>().await?;

        let title = track_response.title.clone();

        let artist = track_response
            .artists
            .first()
            .ok_or("Expected at least one artist")?
            .name
            .clone();

        let featured_artists = track_response
            .artists
            .iter()
            .skip(1) // Skip main artist
            .map(|artist| artist.name.clone())
            .collect();

        let duration = track_response.duration;

        let stream_url = stream_response
            .urls
            .into_iter()
            .next()
            .ok_or("Expected stream URL")?;

        Ok(Track {
            title,
            artist,
            featured_artists,
            duration,
            stream_url,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_numeric_track_ids() {
        let track: TidalTrackResponse = serde_json::from_str(
            r#"{
                "id": 123456789,
                "title": "Song Title",
                "artists": [{"name": "Main Artist"}, {"name": "Guest Artist"}],
                "duration": 185
            }"#,
        )
        .unwrap();

        assert_eq!(track.id(), "123456789");
        assert_eq!(track.title, "Song Title");
        assert_eq!(track.artists[0].name, "Main Artist");
        assert_eq!(track.artists[1].name, "Guest Artist");
        assert_eq!(track.duration, 185);
        assert!(!track.is_video());
    }

    #[test]
    fn deserializes_string_track_ids_and_video_type() {
        let track: TidalTrackResponse = serde_json::from_str(
            r#"{
                "id": "987654321",
                "title": "Video Title",
                "artists": [{"name": "Main Artist"}],
                "duration": 240,
                "type": "video"
            }"#,
        )
        .unwrap();

        assert_eq!(track.id(), "987654321");
        assert!(track.is_video());
    }

    #[test]
    fn deserializes_stream_url_response() {
        let stream_response: StreamUrlResponse = serde_json::from_str(
            r#"{
                "urls": ["https://example.com/stream-one", "https://example.com/stream-two"]
            }"#,
        )
        .unwrap();

        assert_eq!(
            stream_response.urls,
            vec![
                "https://example.com/stream-one".to_string(),
                "https://example.com/stream-two".to_string()
            ]
        );
    }
}
