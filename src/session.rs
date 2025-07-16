use crate::commands::Error;
use crate::track;

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct LoginResponse {
    device_code: String,
    #[serde(rename = "userCode")]
    _user_code: String,
    #[serde(rename = "verificationUri")]
    _verification_uri: String,
    verification_uri_complete: String,
    expires_in: u64,
    interval: u64,
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct SessionResponse {
    session_id: String,
    #[serde(rename = "userId")]
    user_id: u64,
    country_code: String,
    #[serde(rename = "channelId")]
    _channel_id: u64,
    #[serde(rename = "partnerId")]
    _partner_id: u64,
    #[serde(rename = "client")]
    _client: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(serde::Deserialize, Debug)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    token_type: String,
    #[serde(rename = "expires_in")]
    _expires_in: u64,
    #[serde(rename = "user")]
    _user: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct Token {
    access_token: String,
    refresh_token: String,
    token_type: String,
}

impl Token {
    pub fn new(access_token: String, refresh_token: String, token_type: String) -> Self {
        Token {
            access_token,
            refresh_token,
            token_type,
        }
    }
}

#[derive(Debug)]
pub struct Config {
    tidal_client_id: String,
    tidal_client_secret: String,
    path_to_session: String,
    pub user_agent: String,
    oauth_device_auth_url: String,
    oauth_token_url: String,
    sessions_url: String,
    search_url: String,
}

impl Config {
    pub fn new() -> Self {
        Config {
            tidal_client_id: std::env::var("TIDAL_CLIENT_ID").expect("TIDAL_CLIENT_ID must be set"),
            tidal_client_secret: std::env::var("TIDAL_CLIENT_SECRET")
                .expect("TIDAL_CLIENT_SECRET must be set"),
            path_to_session: std::env::var("TIDAL_TOKEN_SESSION_PATH")
            .unwrap_or_else(|_| "data/tidal_token.json".to_string()),
            user_agent: "Mozilla/5.0 (Linux; Android 10; uis8581a2h10_Automotive) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/87.0.4280.101 Safari/537.36".to_string(),
            oauth_device_auth_url: "https://auth.tidal.com/v1/oauth2/device_authorization".to_string(),
            oauth_token_url: "https://auth.tidal.com/v1/oauth2/token".to_string(),
            sessions_url: "https://api.tidal.com/v1/sessions".to_string(),
            search_url: "https://api.tidal.com/v1/search".to_string(),
        }
    }
}

#[derive(Debug)]
pub struct Session {
    pub client: reqwest::Client,
    config: Config,
    pub access_token: String,
    refresh_token: String,
    pub token_type: String,
    pub session_id: String,
    pub country_code: String,
    pub user_id: u64,
}

impl Session {
    pub async fn new() -> Self {
        let mut session = Session {
            client: reqwest::Client::new(),
            config: Config::new(),
            access_token: String::new(),
            refresh_token: String::new(),
            token_type: String::new(),
            session_id: String::new(),
            country_code: String::new(),
            user_id: 0,
        };

        session.start().await.expect("Failed to start session.");

        session
    }

    async fn start(&mut self) -> Result<(), Error> {
        // Attempt to load the token from the file
        if let Ok(()) = self.load_token_from_file().await {
            println!("Token loaded from file successfully.");
            match self.set_session_response().await {
                Ok(()) => {
                    println!("Session started successfully.");
                }
                Err(e) => {
                    println!("Failed to set session response: {}.", e);
                    println!("Attempting to refresh token.");

                    // If setting the session response fails, try to refresh the token
                    self.refresh_token().await?;

                    // Retry setting the session response after refreshing the token
                    match self.set_session_response().await {
                        Ok(()) => {
                            println!("Session response set successfully after refreshing token.");
                            return Ok(());
                        }
                        Err(e2) => {
                            println!(
                                "Failed to set session response after refreshing token: {}",
                                e2
                            );
                            println!("Perhaps you should try to delete your token and re-login.");
                            return Err(e2);
                        }
                    }
                }
            }
            return Ok(());
        } else {
            println!("No token found, starting device authorization flow.");
        }

        // If no token, then start the login process
        self.login().await?;
        println!("Login successful, session started.");

        Ok(())
    }

    async fn login(&mut self) -> Result<(), Error> {
        let auth_url = self.config.oauth_device_auth_url.clone();

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("User-Agent", self.config.user_agent.parse()?);
        headers.insert("Content-Type", "application/x-www-form-urlencoded".parse()?);

        let mut params = std::collections::HashMap::new();
        params.insert("client_id", self.config.tidal_client_id.clone());
        params.insert("scope", "r_usr w_usr w_sub".to_string());

        let response = self
            .client
            .post(auth_url)
            .headers(headers)
            .form(&params)
            .send()
            .await?
            .error_for_status()?;

        let login_response: LoginResponse = serde_json::from_str(&response.text().await?)?;

        println!(
            "Please visit: https://{}",
            login_response.verification_uri_complete
        );

        self.create_token(login_response).await?;

        Ok(())
    }

    async fn create_token(&mut self, login_response: LoginResponse) -> Result<(), Error> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("User-Agent", self.config.user_agent.parse()?);
        headers.insert("Content-Type", "application/x-www-form-urlencoded".parse()?);

        let mut params = std::collections::HashMap::new();
        params.insert("client_id", self.config.tidal_client_id.clone());
        params.insert("client_secret", self.config.tidal_client_secret.clone());
        params.insert("device_code", login_response.device_code.clone());
        params.insert(
            "grant_type",
            "urn:ietf:params:oauth:grant-type:device_code".to_string(),
        );
        params.insert("scope", "r_usr w_usr w_sub".to_string());

        let interval = tokio::time::Duration::from_secs(login_response.interval);
        let mut counter = 0;

        while counter < login_response.expires_in {
            let response = self
                .client
                .post(&self.config.oauth_token_url)
                .headers(headers.clone())
                .form(&params)
                .send()
                .await;

            match response {
                Ok(resp) => {
                    if resp.status().is_success() {
                        let token_response: TokenResponse =
                            serde_json::from_str(&resp.text().await?)?;

                        self.access_token = token_response.access_token.clone();
                        self.refresh_token = token_response
                            .refresh_token
                            .expect("Refresh token is missing")
                            .clone();
                        self.token_type = token_response.token_type.clone();

                        self.set_session_response().await?;

                        let token = Token::new(
                            self.access_token.clone(),
                            self.refresh_token.clone(),
                            self.token_type.clone(),
                        );
                        self.save_token_to_file(token)?;

                        println!("Token created successfully.");
                        return Ok(());
                    } else {
                        tokio::time::sleep(interval).await;
                        counter += login_response.interval;
                    }
                }
                Err(_) => {
                    break;
                }
            }
        }
        Err("Failed to verify login within the allowed time".into())
    }

    pub async fn refresh_token(&mut self) -> Result<(), Error> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("User-Agent", self.config.user_agent.parse()?);
        headers.insert("Content-Type", "application/x-www-form-urlencoded".parse()?);

        let mut params = std::collections::HashMap::new();
        params.insert("client_id", self.config.tidal_client_id.clone());
        params.insert("client_secret", self.config.tidal_client_secret.clone());
        params.insert("refresh_token", self.refresh_token.clone());
        params.insert("grant_type", "refresh_token".to_string());

        let response = self
            .client
            .post(&self.config.oauth_token_url)
            .headers(headers)
            .form(&params)
            .send()
            .await;

        match response {
            Ok(resp) => {
                if resp.status().is_success() {
                    let token_response: TokenResponse = serde_json::from_str(&resp.text().await?)?;

                    self.access_token = token_response.access_token;
                    self.token_type = token_response.token_type;

                    self.set_session_response().await?;

                    let token = Token::new(
                        self.access_token.clone(),
                        self.refresh_token.clone(),
                        self.token_type.clone(),
                    );

                    self.save_token_to_file(token)?;

                    return Ok(());
                } else {
                    return Err("Failed to refresh token".into());
                }
            }
            Err(e) => {
                return Err(format!("Error refreshing token: {}", e).into());
            }
        }
    }

    async fn set_session_response(&mut self) -> Result<(), Error> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("User-Agent", self.config.user_agent.parse()?);
        headers.insert(
            "Authorization",
            format!("{} {}", self.token_type, self.access_token).parse()?,
        );

        let response = self
            .client
            .get(&self.config.sessions_url)
            .headers(headers)
            .send()
            .await?;

        (self.session_id, self.country_code, self.user_id) = {
            let session_response: SessionResponse = serde_json::from_str(&response.text().await?)?;
            (
                session_response.session_id,
                session_response.country_code,
                session_response.user_id,
            )
        };

        Ok(())
    }

    fn save_token_to_file(&self, token: Token) -> Result<(), Error> {
        if let Some(parent) = std::path::Path::new(&self.config.path_to_session).parent() {
            std::fs::create_dir_all(parent)?;
        }

        let f = std::fs::File::create(&self.config.path_to_session);
        let writer = std::io::BufWriter::new(f?);
        serde_json::to_writer_pretty(writer, &token)?;

        Ok(())
    }

    async fn load_token_from_file(&mut self) -> Result<(), Error> {
        if !std::path::Path::new(&self.config.path_to_session).exists() {
            return Err("Session file does not exist".into());
        }

        let f = std::fs::File::open(&self.config.path_to_session)?;
        let reader = std::io::BufReader::new(f);

        let token: Token = serde_json::from_reader(reader)?;

        self.access_token = token.access_token.clone();
        self.refresh_token = token.refresh_token.clone();
        self.token_type = token.token_type.clone();

        Ok(())
    }

    async fn search(
        &self,
        query: &str,
        search_types: Option<&str>,
        limit: u32,
    ) -> Result<serde_json::Value, Error> {
        let limit = limit.to_string();
        let search_types = search_types.unwrap_or("artists,albums,playlists,tracks,videos");

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("User-Agent", self.config.user_agent.parse()?);
        headers.insert(
            "Authorization",
            format!("{} {}", self.token_type, self.access_token).parse()?,
        );
        headers.insert("Accept", "application/json".parse()?);

        let mut params = std::collections::HashMap::new();
        params.insert("query", query);
        params.insert("limit", &limit);
        params.insert("countryCode", &self.country_code);
        params.insert("offset", "0");
        params.insert("types", search_types);

        let response = self
            .client
            .get(&self.config.search_url)
            .headers(headers)
            .query(&params)
            .send()
            .await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                let text = resp.text().await?;
                let json: serde_json::Value = text.parse()?;

                if let serde_json::Value::Object(ref map) = json {
                    let mut filtered = serde_json::Map::new();
                    for key in search_types.split(',') {
                        let key = key.trim();
                        if let Some(v) = map.get(key) {
                            filtered.insert(key.to_string(), v.clone());
                        }
                    }
                    return Ok(serde_json::Value::Object(filtered));
                }

                Err("Search response was not a JSON object".into())
            }
            Ok(resp) => Err(format!("Search failed with status: {}", resp.status()).into()),
            Err(e) => {
                println!("Please check your access token and network connection.");
                Err(format!("Error during search: {}", e).into())
            }
        }
    }

    pub async fn find_tracks(
        &mut self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<track::Track>, Error> {
        async fn try_search(
            this: &crate::session::Session,
            query: &str,
            limit: u32,
        ) -> Result<serde_json::Value, Error> {
            let res = this.search(query, Some("tracks"), limit).await?;
            Ok(res)
        }

        let mut search_result = try_search(self, query, limit).await;

        // If search fails, refresh token and try again
        if search_result.is_err() {
            self.refresh_token().await?;
            search_result = try_search(self, query, limit).await;
        }

        let search_result = search_result?;

        let items = search_result
            .get("tracks")
            .ok_or("No tracks found")?
            .get("items")
            .ok_or("No items found in tracks")?
            .as_array()
            .ok_or("Expected an array of track items")?;

        let mut tracks = Vec::with_capacity(items.len());

        for item in items {
            let track = track::Track::from_track_id(self, item).await;
            tracks.push(track);
        }

        Ok(tracks)
    }
}
