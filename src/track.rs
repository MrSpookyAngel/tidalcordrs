use crate::commands::Error;

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
        track_response: &serde_json::Value,
    ) -> Result<Self, Error> {
        let track_id = track_response["id"]
            .as_u64()
            .map(|id| id.to_string())
            .or_else(|| track_response["id"].as_str().map(String::from))
            .ok_or("Expected track id")?;

        let url = format!(
            "https://api.tidal.com/v1/tracks/{}/urlpostpaywall",
            track_id
        );

        let params = [
            ("sessionId", session.session_id.as_str()),
            ("countryCode", session.country_code.as_str()),
            ("urlusagemode", "STREAM"),
            ("audioquality", "HIGH"),
            ("assetpresentation", "FULL"),
        ];

        let response = session
            .client
            .get(&url)
            .query(&params)
            .send()
            .await?
            .error_for_status()?;

        let json = response.json::<serde_json::Value>().await?;

        let title = track_response["title"]
            .as_str()
            .ok_or("Expected title")?
            .to_string();

        let artist = track_response["artists"][0]["name"]
            .as_str()
            .ok_or("Expected artist name")?
            .to_string();

        let featured_artists = track_response["artists"]
            .as_array()
            .ok_or("Expected artists array")?
            .iter()
            .skip(1) // Skip main artist
            .filter_map(|artist| artist["name"].as_str().map(String::from))
            .collect();

        let duration = track_response["duration"]
            .as_u64()
            .ok_or("Expected duration")?
            .try_into()?;

        let stream_url = json["urls"][0]
            .as_str()
            .ok_or("Expected stream URL")?
            .to_string();

        Ok(Track {
            title,
            artist,
            featured_artists,
            duration,
            stream_url,
        })
    }
}
