use crate::commands::Error;
use crate::track;

pub const DEFAULT_COLLECTION_TRACK_FETCH_CONCURRENCY: usize = 8;

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
}

#[derive(serde::Deserialize, Debug)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    token_type: String,
    #[serde(rename = "expires_in")]
    _expires_in: u64,
}

#[derive(serde::Deserialize, Debug)]
struct SearchTracksResponse {
    tracks: TrackItemsResponse,
}

#[derive(serde::Deserialize, Debug)]
struct TrackItemsResponse {
    items: Vec<track::TidalTrackResponse>,
}

#[derive(serde::Deserialize, Debug)]
struct CollectionRelationshipsResponse {
    #[serde(default)]
    data: Vec<CollectionRelationshipItem>,
    #[serde(default)]
    links: CollectionLinks,
}

#[derive(serde::Deserialize, Debug)]
struct CollectionRelationshipItem {
    #[serde(rename = "type")]
    item_type: String,
    id: String,
}

#[derive(serde::Deserialize, Debug, Default)]
struct CollectionLinks {
    next: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
#[serde(untagged)]
enum LegacyCollectionTrackItem {
    Wrapped { item: track::TidalTrackResponse },
    Direct(track::TidalTrackResponse),
}

impl LegacyCollectionTrackItem {
    fn into_track_response(self) -> track::TidalTrackResponse {
        match self {
            Self::Wrapped { item } => item,
            Self::Direct(item) => item,
        }
    }
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct LegacyCollectionTracksPage {
    items: Vec<LegacyCollectionTrackItem>,
    total_number_of_items: Option<u64>,
    total: Option<u64>,
}

#[derive(Clone)]
struct TrackFetchContext {
    client: reqwest::Client,
    user_agent: String,
    token_type: String,
    access_token: String,
    session_id: String,
    country_code: String,
}

impl TrackFetchContext {
    async fn get_track_response(&self, track_id: &str) -> Result<track::TidalTrackResponse, Error> {
        let url = format!("https://api.tidal.com/v1/tracks/{}", track_id);
        let params = [
            ("sessionId", self.session_id.as_str()),
            ("countryCode", self.country_code.as_str()),
        ];

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("User-Agent", self.user_agent.parse()?);
        headers.insert(
            "Authorization",
            format!("{} {}", self.token_type, self.access_token).parse()?,
        );
        headers.insert("Accept", "application/json".parse()?);

        let response = self
            .client
            .get(&url)
            .headers(headers)
            .query(&params)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(response_error("Track lookup failed", response).await);
        }

        Ok(response.json().await?)
    }

    async fn fetch_track(&self, track_id: &str) -> Result<track::Track, FetchTrackError> {
        let track_response = self
            .get_track_response(track_id)
            .await
            .map_err(FetchTrackError::TrackResponse)?;

        track::Track::from_track_response(
            &self.client,
            &self.session_id,
            &self.country_code,
            &track_response,
        )
        .await
        .map_err(FetchTrackError::Stream)
    }
}

enum FetchTrackError {
    TrackResponse(Error),
    Stream(Error),
}

impl std::fmt::Display for FetchTrackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TrackResponse(error) | Self::Stream(error) => write!(f, "{error}"),
        }
    }
}

struct TrackFetchOutcome {
    index: usize,
    id: String,
    result: Result<track::Track, FetchTrackError>,
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

async fn response_error(message: &str, response: reqwest::Response) -> Error {
    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|error| format!("failed to read response body: {error}"));

    format!("{message}: status {status}, body: {body}").into()
}

#[cfg(unix)]
fn create_token_file(path: &str) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

#[cfg(not(unix))]
fn create_token_file(path: &str) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
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
    pub fn new() -> Result<Self, Error> {
        Ok(Config {
            tidal_client_id: required_env("TIDAL_CLIENT_ID")?,
            tidal_client_secret: required_env("TIDAL_CLIENT_SECRET")?,
            path_to_session: required_env("TIDAL_TOKEN_SESSION_PATH")?,
            user_agent: required_env("USER_AGENT")?,
            oauth_device_auth_url: required_env("OAUTH_DEVICE_AUTH_URL")?,
            oauth_token_url: required_env("OAUTH_TOKEN_URL")?,
            sessions_url: required_env("SESSIONS_URL")?,
            search_url: required_env("SEARCH_URL")?,
        })
    }
}

fn required_env(name: &str) -> Result<String, Error> {
    match std::env::var(name) {
        Ok(value) => Ok(value),
        Err(std::env::VarError::NotPresent) => Err(format!("{name} must be set").into()),
        Err(std::env::VarError::NotUnicode(_)) => Err(format!("{name} must be valid UTF-8").into()),
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
    pub async fn new() -> Result<Self, Error> {
        let mut session = Session {
            client: reqwest::Client::new(),
            config: Config::new()?,
            access_token: String::new(),
            refresh_token: String::new(),
            token_type: String::new(),
            session_id: String::new(),
            country_code: String::new(),
            user_id: 0,
        };

        session.start().await?;

        Ok(session)
    }

    async fn start(&mut self) -> Result<(), Error> {
        // Attempt to load the token from the file
        if let Ok(()) = self.load_token_from_file().await {
            tracing::info!("Token loaded from file successfully");
            match self.set_session_response().await {
                Ok(()) => {
                    tracing::info!("Session started successfully");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to set session response");
                    tracing::info!("Attempting to refresh token");

                    // If setting the session response fails, try to refresh the token
                    self.refresh_token().await?;

                    // Retry setting the session response after refreshing the token
                    match self.set_session_response().await {
                        Ok(()) => {
                            tracing::info!(
                                "Session response set successfully after refreshing token"
                            );
                            return Ok(());
                        }
                        Err(e2) => {
                            tracing::error!(
                                error = %e2,
                                "Failed to set session response after refreshing token"
                            );
                            tracing::warn!(
                                "Perhaps you should try to delete your token and re-login"
                            );
                            return Err(e2);
                        }
                    }
                }
            }
            return Ok(());
        } else {
            tracing::info!("No token found, starting device authorization flow");
        }

        // If no token, then start the login process
        self.login().await?;
        tracing::info!("Login successful, session started");

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

        let login_response: LoginResponse = response.json().await?;

        let verification_url = format!("https://{}", login_response.verification_uri_complete);
        tracing::info!(%verification_url, "Please authorize Tidal device login");

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
                        let token_response: TokenResponse = resp.json().await?;

                        self.access_token = token_response.access_token;
                        self.refresh_token = token_response
                            .refresh_token
                            .ok_or("Token response did not include a refresh token")?;
                        self.token_type = token_response.token_type;

                        self.set_session_response().await?;

                        let token = Token::new(
                            self.access_token.clone(),
                            self.refresh_token.clone(),
                            self.token_type.clone(),
                        );
                        self.save_token_to_file(token)?;

                        tracing::info!("Token created successfully");
                        return Ok(());
                    } else {
                        tokio::time::sleep(interval).await;
                        counter += login_response.interval;
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "Failed to poll Tidal token endpoint");
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
                    let token_response: TokenResponse = resp.json().await?;

                    self.access_token = token_response.access_token;
                    if let Some(refresh_token) = token_response.refresh_token {
                        self.refresh_token = refresh_token;
                    }
                    self.token_type = token_response.token_type;

                    self.set_session_response().await?;

                    let token = Token::new(
                        self.access_token.clone(),
                        self.refresh_token.clone(),
                        self.token_type.clone(),
                    );

                    self.save_token_to_file(token)?;

                    Ok(())
                } else {
                    Err(response_error("Failed to refresh token", resp).await)
                }
            }
            Err(e) => Err(format!("Error refreshing token: {}", e).into()),
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
            let session_response: SessionResponse = response.json().await?;
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

        let f = create_token_file(&self.config.path_to_session)?;
        let writer = std::io::BufWriter::new(f);
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

    fn track_fetch_context(&self) -> TrackFetchContext {
        TrackFetchContext {
            client: self.client.clone(),
            user_agent: self.config.user_agent.clone(),
            token_type: self.token_type.clone(),
            access_token: self.access_token.clone(),
            session_id: self.session_id.clone(),
            country_code: self.country_code.clone(),
        }
    }

    async fn fetch_tracks_bounded(
        context: TrackFetchContext,
        ids: Vec<(usize, String)>,
        concurrency: usize,
    ) -> Vec<TrackFetchOutcome> {
        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency));
        let mut tasks = tokio::task::JoinSet::new();

        for (index, id) in ids {
            let permit = semaphore
                .clone()
                .acquire_owned()
                .await
                .expect("Track fetch semaphore should not close");
            let context = context.clone();

            tasks.spawn(async move {
                let _permit = permit;
                let result = context.fetch_track(&id).await;
                TrackFetchOutcome { index, id, result }
            });
        }

        let mut outcomes = Vec::new();

        while let Some(result) = tasks.join_next().await {
            match result {
                Ok(outcome) => outcomes.push(outcome),
                Err(error) => {
                    tracing::error!(%error, "Collection track fetch task failed");
                }
            }
        }

        outcomes.sort_by_key(|outcome| outcome.index);
        outcomes
    }

    async fn find_collection_tracks_by_ids(
        &mut self,
        ids: Vec<String>,
        concurrency: usize,
    ) -> Vec<track::Track> {
        let mut tracks = Vec::new();
        tracks.resize_with(ids.len(), || None);

        let indexed_ids = ids.into_iter().enumerate().collect::<Vec<_>>();
        let first_pass =
            Self::fetch_tracks_bounded(self.track_fetch_context(), indexed_ids, concurrency).await;
        let mut retry_ids = Vec::new();

        for outcome in first_pass {
            match outcome.result {
                Ok(track) => tracks[outcome.index] = Some(track),
                Err(FetchTrackError::TrackResponse(_)) => {
                    retry_ids.push((outcome.index, outcome.id));
                }
                Err(FetchTrackError::Stream(error)) => {
                    tracing::warn!(track_id = %outcome.id, %error, "Skipping collection track");
                }
            }
        }

        if !retry_ids.is_empty() {
            match self.refresh_token().await {
                Ok(()) => {
                    let second_pass = Self::fetch_tracks_bounded(
                        self.track_fetch_context(),
                        retry_ids,
                        concurrency,
                    )
                    .await;

                    for outcome in second_pass {
                        match outcome.result {
                            Ok(track) => tracks[outcome.index] = Some(track),
                            Err(error) => {
                                tracing::warn!(
                                    track_id = %outcome.id,
                                    %error,
                                    "Skipping collection track"
                                );
                            }
                        }
                    }
                }
                Err(error) => {
                    for (_, id) in retry_ids {
                        tracing::warn!(track_id = %id, %error, "Skipping collection track");
                    }
                }
            }
        }

        tracks.into_iter().flatten().collect()
    }

    async fn search_tracks(&self, query: &str, limit: u32) -> Result<SearchTracksResponse, Error> {
        let limit = limit.to_string();

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
        params.insert("types", "tracks");

        let response = self
            .client
            .get(&self.config.search_url)
            .headers(headers)
            .query(&params)
            .send()
            .await;

        match response {
            Ok(resp) if resp.status().is_success() => Ok(resp.json().await?),
            Ok(resp) => Err(format!("Search failed with status: {}", resp.status()).into()),
            Err(e) => {
                tracing::warn!("Please check your access token and network connection");
                Err(format!("Error during search: {}", e).into())
            }
        }
    }

    async fn get_track_response(&self, track_id: &str) -> Result<track::TidalTrackResponse, Error> {
        let url = format!("https://api.tidal.com/v1/tracks/{}", track_id);
        let params = [
            ("sessionId", self.session_id.as_str()),
            ("countryCode", self.country_code.as_str()),
        ];

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("User-Agent", self.config.user_agent.parse()?);
        headers.insert(
            "Authorization",
            format!("{} {}", self.token_type, self.access_token).parse()?,
        );
        headers.insert("Accept", "application/json".parse()?);

        let response = self
            .client
            .get(&url)
            .headers(headers)
            .query(&params)
            .send()
            .await?
            .error_for_status()?;

        Ok(response.json().await?)
    }

    pub async fn find_track_by_id(&mut self, track_id: &str) -> Result<track::Track, Error> {
        let mut track_response = self.get_track_response(track_id).await;

        if track_response.is_err() {
            self.refresh_token().await?;
            track_response = self.get_track_response(track_id).await;
        }

        track::Track::from_track_id(self, &track_response?).await
    }

    async fn collection_track_ids(
        &self,
        collection_type: &str,
        collection_id: &str,
    ) -> Result<Vec<String>, Error> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("User-Agent", self.config.user_agent.parse()?);
        headers.insert(
            "Authorization",
            format!("{} {}", self.token_type, self.access_token).parse()?,
        );
        headers.insert("Accept", "application/vnd.api+json".parse()?);

        let mut next_url = Some(format!(
            "https://openapi.tidal.com/v2/{}/{}/relationships/items?countryCode={}",
            collection_type, collection_id, self.country_code
        ));
        let mut track_ids = Vec::new();

        while let Some(url) = next_url.take() {
            let response = self
                .client
                .get(&url)
                .headers(headers.clone())
                .send()
                .await?
                .error_for_status()?;
            let page = response.json::<CollectionRelationshipsResponse>().await?;

            track_ids.extend(page.data.into_iter().filter_map(|item| {
                if item.item_type == "tracks" {
                    Some(item.id)
                } else {
                    None
                }
            }));

            next_url = page.links.next.filter(|link| !link.is_empty()).map(|link| {
                if link.starts_with("http") {
                    link
                } else {
                    format!("https://openapi.tidal.com/v2{}", link)
                }
            });
        }

        Ok(track_ids)
    }

    async fn legacy_collection_tracks_page(
        &self,
        collection_type: &str,
        collection_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<LegacyCollectionTracksPage, Error> {
        let url = format!(
            "https://api.tidal.com/v1/{}/{}/tracks",
            collection_type, collection_id
        );
        let limit = limit.to_string();
        let offset = offset.to_string();
        let params = [
            ("sessionId", self.session_id.as_str()),
            ("countryCode", self.country_code.as_str()),
            ("limit", limit.as_str()),
            ("offset", offset.as_str()),
        ];

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("User-Agent", self.config.user_agent.parse()?);
        headers.insert(
            "Authorization",
            format!("{} {}", self.token_type, self.access_token).parse()?,
        );
        headers.insert("Accept", "application/json".parse()?);

        let response = self
            .client
            .get(&url)
            .headers(headers)
            .query(&params)
            .send()
            .await?
            .error_for_status()?;

        Ok(response.json().await?)
    }

    async fn legacy_collection_tracks(
        &mut self,
        collection_type: &str,
        collection_id: &str,
    ) -> Result<Vec<track::Track>, Error> {
        let mut tracks = Vec::new();
        let limit = 100;
        let mut offset = 0;

        loop {
            let mut page = self
                .legacy_collection_tracks_page(collection_type, collection_id, limit, offset)
                .await;

            if page.is_err() {
                self.refresh_token().await?;
                page = self
                    .legacy_collection_tracks_page(collection_type, collection_id, limit, offset)
                    .await;
            }

            let page = page?;
            let item_count = page.items.len();

            if page.items.is_empty() {
                break;
            }

            for item in page.items {
                let track_response = item.into_track_response();
                if track_response.is_video() {
                    continue;
                }

                match track::Track::from_track_id(self, &track_response).await {
                    Ok(track) => tracks.push(track),
                    Err(error) => {
                        tracing::warn!(%error, "Skipping collection track");
                    }
                }
            }

            offset += item_count as u32;

            let total = page.total_number_of_items.or(page.total);

            if item_count < limit as usize || total.is_some_and(|total| offset as u64 >= total) {
                break;
            }
        }

        Ok(tracks)
    }

    pub async fn find_collection_tracks(
        &mut self,
        collection_type: &str,
        collection_id: &str,
        concurrency: usize,
    ) -> Result<Vec<track::Track>, Error> {
        let mut ids = self
            .collection_track_ids(collection_type, collection_id)
            .await;

        if ids.is_err() {
            self.refresh_token().await?;
            ids = self
                .collection_track_ids(collection_type, collection_id)
                .await;
        }

        match ids {
            Ok(ids) => Ok(self.find_collection_tracks_by_ids(ids, concurrency).await),
            Err(error) => {
                tracing::warn!(
                    collection_type,
                    collection_id,
                    %error,
                    "Falling back to legacy collection endpoint"
                );
                self.legacy_collection_tracks(collection_type, collection_id)
                    .await
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
        ) -> Result<SearchTracksResponse, Error> {
            let res = this.search_tracks(query, limit).await?;
            Ok(res)
        }

        let mut search_result = try_search(self, query, limit).await;

        // If search fails, refresh token and try again
        if search_result.is_err() {
            self.refresh_token().await?;
            search_result = try_search(self, query, limit).await;
        }

        let items = search_result?.tracks.items;

        let mut tracks = Vec::with_capacity(items.len());

        for item in &items {
            let track = track::Track::from_track_id(self, item).await?;
            tracks.push(track);
        }

        Ok(tracks)
    }

    pub async fn find_track_by_details(
        &mut self,
        title: &str,
        artist: &str,
        album: &str,
    ) -> Result<Option<track::Track>, Error> {
        // No album included in first search because sometimes album name is a song name that is more popular than the title
        let short_query = format!("{} {}", artist, title);
        let mut short_tracks = self.find_tracks(&short_query, 1).await?;

        if !short_tracks.is_empty() {
            return Ok(short_tracks.pop());
        }

        let full_query = format!("{} {} {}", artist, title, album);
        let mut tracks = self.find_tracks(&full_query, 1).await?;

        Ok(tracks.pop())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_search_tracks_response() {
        let response: SearchTracksResponse = serde_json::from_str(
            r#"{
                "tracks": {
                    "items": [
                        {
                            "id": 123,
                            "title": "First Track",
                            "artists": [{"name": "First Artist"}],
                            "duration": 180
                        },
                        {
                            "id": "456",
                            "title": "Second Track",
                            "artists": [{"name": "Second Artist"}],
                            "duration": 240
                        }
                    ]
                }
            }"#,
        )
        .unwrap();

        assert_eq!(response.tracks.items.len(), 2);
        assert_eq!(response.tracks.items[0].id(), "123");
        assert_eq!(response.tracks.items[1].id(), "456");
    }

    #[test]
    fn deserializes_collection_relationships_response() {
        let response: CollectionRelationshipsResponse = serde_json::from_str(
            r#"{
                "data": [
                    {"type": "tracks", "id": "track-one"},
                    {"type": "videos", "id": "video-one"}
                ],
                "links": {
                    "next": "/playlists/abc/relationships/items?page[cursor]=next"
                }
            }"#,
        )
        .unwrap();

        let track_ids = response
            .data
            .into_iter()
            .filter_map(|item| (item.item_type == "tracks").then_some(item.id))
            .collect::<Vec<_>>();

        assert_eq!(track_ids, vec!["track-one".to_string()]);
        assert_eq!(
            response.links.next.as_deref(),
            Some("/playlists/abc/relationships/items?page[cursor]=next")
        );
    }

    #[test]
    fn deserializes_empty_collection_links() {
        let response: CollectionRelationshipsResponse = serde_json::from_str(
            r#"{
                "data": [
                    {"type": "tracks", "id": "track-one"}
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(response.data.len(), 1);
        assert_eq!(response.links.next, None);
    }

    #[test]
    fn deserializes_legacy_collection_wrapped_and_direct_items() {
        let page: LegacyCollectionTracksPage = serde_json::from_str(
            r#"{
                "items": [
                    {
                        "item": {
                            "id": 123,
                            "title": "Wrapped Track",
                            "artists": [{"name": "Wrapped Artist"}],
                            "duration": 180
                        }
                    },
                    {
                        "id": "456",
                        "title": "Direct Track",
                        "artists": [{"name": "Direct Artist"}],
                        "duration": 240,
                        "type": "video"
                    }
                ],
                "totalNumberOfItems": 2
            }"#,
        )
        .unwrap();

        assert_eq!(page.items.len(), 2);
        assert_eq!(page.total_number_of_items, Some(2));
        assert_eq!(page.total, None);

        let mut items = page.items.into_iter();
        let wrapped = items.next().unwrap().into_track_response();
        let direct = items.next().unwrap().into_track_response();

        assert_eq!(wrapped.id(), "123");
        assert!(!wrapped.is_video());
        assert_eq!(direct.id(), "456");
        assert!(direct.is_video());
    }
}
