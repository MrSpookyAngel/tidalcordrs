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
    _user_id: u64,
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
    oauth_device_auth_url: reqwest::Url,
    oauth_token_url: reqwest::Url,
    path_to_session: String,
    sessions_url: reqwest::Url,
    search_limit: String,
    pub user_agent: String,
}

impl Config {
    pub fn new() -> Self {
        Config {
            tidal_client_id: std::env::var("TIDAL_CLIENT_ID").expect("TIDAL_CLIENT_ID must be set"),
            tidal_client_secret: std::env::var("TIDAL_CLIENT_SECRET")
                .expect("TIDAL_CLIENT_SECRET must be set"),
            oauth_device_auth_url: reqwest::Url::parse(
                "https://auth.tidal.com/v1/oauth2/device_authorization",
            )
            .expect("Invalid URL for device auth"),
            oauth_token_url: reqwest::Url::parse("https://auth.tidal.com/v1/oauth2/token")
                .expect("Invalid URL for token exchange"),
            path_to_session: std::env::var("TIDAL_TOKEN_SESSION_PATH")
                .unwrap_or_else(|_| "data/tidal_token.json".to_string()),
            sessions_url: reqwest::Url::parse("https://api.tidal.com/v1/sessions")
                .expect("Invalid URL for sessions"),
            search_limit: std::env::var("TIDAL_SEARCH_LIMIT").unwrap_or_else(|_| "10".to_string()),
            user_agent: "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/134.0.0.0 Mobile Safari/537.3".to_string(),
        }
    }
}

pub struct Session {
    client: reqwest::Client,
    config: Config,
    access_token: String,
    refresh_token: String,
    token_type: String,
    session_id: String,
    country_code: String,
}

impl Session {
    pub fn new() -> Self {
        Session {
            client: reqwest::Client::new(),
            config: Config::new(),
            access_token: String::new(),
            refresh_token: String::new(),
            token_type: String::new(),
            session_id: String::new(),
            country_code: String::new(),
        }
    }

    pub async fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Attempt to load the token from the file
        if let Ok(()) = self.load_token_from_file().await {
            println!("Token loaded from file successfully.");
            return Ok(());
        } else {
            println!("No token found, starting device authorization flow.");
        }

        // If no token, then start the login process
        self.login().await?;
        println!("Login successful, session started.");

        Ok(())
    }

    async fn login(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut login_url = self.config.oauth_device_auth_url.clone();
        login_url
            .query_pairs_mut()
            .append_pair("client_id", &self.config.tidal_client_id)
            .append_pair("scope", "r_usr w_usr w_sub");

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("User-Agent", self.config.user_agent.parse()?);
        headers.insert("Content-Type", "application/x-www-form-urlencoded".parse()?);

        let response = self
            .client
            .post(login_url)
            .headers(headers)
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

    async fn create_token(
        &mut self,
        login_response: LoginResponse,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut token_url = self.config.oauth_token_url.clone();
        token_url
            .query_pairs_mut()
            .append_pair("client_id", &self.config.tidal_client_id)
            .append_pair("client_secret", &self.config.tidal_client_secret)
            .append_pair("device_code", &login_response.device_code)
            .append_pair("grant_type", "urn:ietf:params:oauth:grant-type:device_code")
            .append_pair("scope", "r_usr w_usr w_sub");

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("User-Agent", self.config.user_agent.parse()?);
        headers.insert("Content-Type", "application/x-www-form-urlencoded".parse()?);

        let interval = tokio::time::Duration::from_secs(login_response.interval);
        let mut counter = 0;

        while counter < login_response.expires_in {
            let response = self
                .client
                .post(token_url.clone())
                .headers(headers.clone())
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

                        (self.session_id, self.country_code) = {
                            let session_response = self.get_session_response().await?;
                            (session_response.session_id, session_response.country_code)
                        };

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

    pub async fn refresh_token(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut token_url = self.config.oauth_token_url.clone();
        token_url
            .query_pairs_mut()
            .append_pair("client_id", &self.config.tidal_client_id)
            .append_pair("client_secret", &self.config.tidal_client_secret)
            .append_pair("refresh_token", &self.refresh_token)
            .append_pair("grant_type", "refresh_token")
            .append_pair("scope", "r_usr w_usr w_sub");

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("User-Agent", self.config.user_agent.parse()?);
        headers.insert("Content-Type", "application/x-www-form-urlencoded".parse()?);

        let response = self.client.post(token_url).headers(headers).send().await;

        match response {
            Ok(resp) => {
                if resp.status().is_success() {
                    let token_response: TokenResponse = serde_json::from_str(&resp.text().await?)?;

                    self.access_token = token_response.access_token;
                    self.token_type = token_response.token_type;

                    (self.session_id, self.country_code) = {
                        let session_response = self.get_session_response().await?;
                        (session_response.session_id, session_response.country_code)
                    };

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

    async fn get_session_response(&self) -> Result<SessionResponse, Box<dyn std::error::Error>> {
        let sessions_url = self.config.sessions_url.clone();

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("User-Agent", self.config.user_agent.parse()?);
        headers.insert(
            "Authorization",
            format!("{} {}", self.token_type, self.access_token).parse()?,
        );

        let response = self
            .client
            .get(sessions_url)
            .headers(headers)
            .send()
            .await
            .expect("Failed to get session ID")
            .error_for_status()
            .expect("Failed to get session ID");

        let session_response: SessionResponse = serde_json::from_str(&response.text().await?)?;

        Ok(session_response)
    }

    fn save_token_to_file(&self, token: Token) -> Result<(), Box<dyn std::error::Error>> {
        let f = std::fs::File::create(&self.config.path_to_session);
        let writer = std::io::BufWriter::new(f?);
        serde_json::to_writer_pretty(writer, &token)?;

        Ok(())
    }

    async fn load_token_from_file(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let f = std::fs::File::open(&self.config.path_to_session)?;
        let reader = std::io::BufReader::new(f);

        let token: Token = serde_json::from_reader(reader)?;

        self.access_token = token.access_token.clone();
        self.refresh_token = token.refresh_token.clone();
        self.token_type = token.token_type.clone();
        (self.session_id, self.country_code) = {
            let session_response = self.get_session_response().await?;
            (session_response.session_id, session_response.country_code)
        };

        Ok(())
    }

    pub async fn search(
        &self,
        query: &str,
        search_types: Option<&str>,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let search_types = search_types.unwrap_or("artists,albums,playlists,tracks,videos");

        let mut search_url = reqwest::Url::parse("https://api.tidal.com/v1/search")?;
        search_url
            .query_pairs_mut()
            .append_pair("query", query)
            .append_pair("limit", &self.config.search_limit)
            .append_pair("countryCode", &self.country_code)
            .append_pair("offset", "0")
            .append_pair("types", search_types);

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("User-Agent", self.config.user_agent.parse()?);
        headers.insert(
            "Authorization",
            format!("{} {}", self.token_type, self.access_token).parse()?,
        );

        println!("Search URL: {}", search_url);
        println!("Headers: {:?}", headers);

        let response = self.client.get(search_url).headers(headers).send().await;

        match response {
            Ok(resp) => {
                if resp.status().is_success() {
                    let text = resp.text().await?;
                    return Ok(text.parse::<serde_json::Value>()?);
                } else {
                    return Err(format!("Search failed with status: {}", resp.status()).into());
                }
            }
            Err(e) => {
                println!("Please check your access token and network connection.");
                return Err(format!("Error during search: {}", e).into());
            }
        }
    }
}
