#[derive(Debug)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub featured_artists: Vec<String>, // Can be empty
    pub album: String,
    pub duration: u32,
    pub stream_url: String,
}

impl Track {
    pub async fn from_track_id(
        session: &crate::session::Session,
        track_response: &serde_json::Value,
    ) -> Self {
        let url = format!(
            "https://api.tidal.com/v1/tracks/{}/urlpostpaywall",
            track_response["id"]
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
            .await
            .expect("Failed to send request");

        let json = response
            .json::<serde_json::Value>()
            .await
            .expect("Failed to parse JSON");

        let id = format!("{}", json["trackId"]).to_string();

        let title = track_response["title"]
            .as_str()
            .expect("Expected title")
            .to_string();

        let artist = track_response["artists"][0]["name"]
            .as_str()
            .expect("Expected artist name")
            .to_string();

        let featured_artists = track_response["artists"]
            .as_array()
            .expect("Expected artists array")
            .iter()
            .skip(1) // Skip main artist
            .filter_map(|artist| artist["name"].as_str().map(String::from))
            .collect();

        let album = track_response["album"]["title"]
            .as_str()
            .expect("Expected album title")
            .to_string();

        let duration = track_response["duration"]
            .as_u64()
            .expect("Expected duration")
            .try_into()
            .expect("Duration overflow");

        let stream_url = json["urls"][0]
            .as_str()
            .expect("Expected stream URL")
            .to_string();

        Track {
            id,
            title,
            artist,
            featured_artists,
            album,
            duration,
            stream_url,
        }
    }
}
