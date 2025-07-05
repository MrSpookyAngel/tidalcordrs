#[derive(Debug)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: u32,
    pub stream_url: String,
    pub is_available: bool,
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
            ("audioquality", "HIGH"), // Assuming 'HIGH' is the desired quality
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

        let is_available = track_response["allowStreaming"]
            .as_bool()
            .expect("Expected allow streaming status");

        Track {
            id,
            title,
            artist,
            album,
            duration,
            stream_url,
            is_available,
        }
    }

    pub fn _print_info(&self) {
        println!("Track ID: {}", self.id);
        println!("Title: {}", self.title);
        println!("Artist: {}", self.artist);
        println!("Album: {}", self.album);
        println!("Duration: {} seconds", self.duration);
        println!("Stream URL: {}", self.stream_url);
        println!("Is Available: {}", self.is_available);
    }
}
