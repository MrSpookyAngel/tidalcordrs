use crate::commands;
use std::path::{Path, PathBuf};

const DEFAULT_COMMAND_PREFIX: &str = "!";
const DEFAULT_BOT_NAME: &str = "TidalCordRS";
const DEFAULT_BOT_PROFILE_STATE_PATH: &str = "data/bot_profile_state.json";
const DEFAULT_SPOOL_READ_AHEAD_MIB: u64 = 16;
const DEFAULT_COLLECTION_TRACK_FETCH_CONCURRENCY: usize = 8;
const DEFAULT_TIDAL_TOKEN_SESSION_PATH: &str = "data/tidal_token.json";
const DEFAULT_TIDAL_CLIENT_ID: &str = "fX2JxdmntZWK0ixT";
const DEFAULT_TIDAL_CLIENT_SECRET: &str = "1Nm5AfDAjxrgJFJbKNWLeAyKGVGmINuXPPLHVXAvxAg=";
const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Linux; Android 10; uis8581a2h10_Automotive) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/87.0.4280.101 Safari/537.36";
const DEFAULT_OAUTH_DEVICE_AUTH_URL: &str = "https://auth.tidal.com/v1/oauth2/device_authorization";
const DEFAULT_OAUTH_TOKEN_URL: &str = "https://auth.tidal.com/v1/oauth2/token";
const DEFAULT_SESSIONS_URL: &str = "https://api.tidal.com/v1/sessions";
const DEFAULT_SEARCH_URL: &str = "https://api.tidal.com/v1/search";

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub discord_token: String,
    pub command_prefix: String,
    pub bot_profile: BotProfileConfig,
    pub technical: TechnicalConfig,
    pub tidal: TidalConfig,
}

#[derive(Clone, Debug)]
pub struct BotProfileConfig {
    pub enabled: bool,
    pub name: String,
    pub avatar_path: Option<PathBuf>,
    pub state_path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct TechnicalConfig {
    pub spool_read_ahead_mib: u64,
    pub collection_track_fetch_concurrency: usize,
}

impl TechnicalConfig {
    pub fn spool_read_ahead_bytes(&self) -> Result<u64, commands::Error> {
        self.spool_read_ahead_mib
            .checked_mul(1024 * 1024)
            .ok_or("technical.spool_read_ahead_mib is too large".into())
    }
}

#[derive(Clone, Debug)]
pub struct TidalConfig {
    pub tidal_client_id: String,
    pub tidal_client_secret: String,
    pub path_to_session: String,
    pub user_agent: String,
    pub oauth_device_auth_url: String,
    pub oauth_token_url: String,
    pub sessions_url: String,
    pub search_url: String,
}

#[derive(Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FileConfig {
    command_prefix: Option<String>,
    bot_profile: FileBotProfileConfig,
    technical: FileTechnicalConfig,
    tidal: FileTidalConfig,
}

#[derive(Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FileBotProfileConfig {
    sync_enabled: Option<bool>,
    name: Option<String>,
    avatar_path: Option<String>,
    state_path: Option<String>,
}

#[derive(Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FileTechnicalConfig {
    spool_read_ahead_mib: Option<u64>,
    collection_track_fetch_concurrency: Option<usize>,
}

#[derive(Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FileTidalConfig {
    token_session_path: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    user_agent: Option<String>,
    oauth_device_auth_url: Option<String>,
    oauth_token_url: Option<String>,
    sessions_url: Option<String>,
    search_url: Option<String>,
}

impl AppConfig {
    pub fn load() -> Result<Self, commands::Error> {
        let mut config = Self::default_without_secrets();
        if let Some(path) = config_path()? {
            config.merge_file(&path)?;
        }
        config.apply_env_overrides()?;
        config.discord_token = required_env("DISCORD_TOKEN")?;
        config.validate()?;
        Ok(config)
    }

    fn default_without_secrets() -> Self {
        Self {
            discord_token: String::new(),
            command_prefix: DEFAULT_COMMAND_PREFIX.to_string(),
            bot_profile: BotProfileConfig {
                enabled: true,
                name: DEFAULT_BOT_NAME.to_string(),
                avatar_path: None,
                state_path: PathBuf::from(DEFAULT_BOT_PROFILE_STATE_PATH),
            },
            technical: TechnicalConfig {
                spool_read_ahead_mib: DEFAULT_SPOOL_READ_AHEAD_MIB,
                collection_track_fetch_concurrency: DEFAULT_COLLECTION_TRACK_FETCH_CONCURRENCY,
            },
            tidal: TidalConfig {
                tidal_client_id: DEFAULT_TIDAL_CLIENT_ID.to_string(),
                tidal_client_secret: DEFAULT_TIDAL_CLIENT_SECRET.to_string(),
                path_to_session: DEFAULT_TIDAL_TOKEN_SESSION_PATH.to_string(),
                user_agent: DEFAULT_USER_AGENT.to_string(),
                oauth_device_auth_url: DEFAULT_OAUTH_DEVICE_AUTH_URL.to_string(),
                oauth_token_url: DEFAULT_OAUTH_TOKEN_URL.to_string(),
                sessions_url: DEFAULT_SESSIONS_URL.to_string(),
                search_url: DEFAULT_SEARCH_URL.to_string(),
            },
        }
    }

    fn merge_file(&mut self, path: &Path) -> Result<(), commands::Error> {
        let content = std::fs::read_to_string(path)
            .map_err(|error| format!("Failed to read config file {}: {error}", path.display()))?;
        let file_config: FileConfig = toml::from_str(&content)
            .map_err(|error| format!("Failed to parse config file {}: {error}", path.display()))?;

        if let Some(command_prefix) = file_config.command_prefix {
            self.command_prefix = command_prefix;
        }
        if let Some(sync_enabled) = file_config.bot_profile.sync_enabled {
            self.bot_profile.enabled = sync_enabled;
        }
        if let Some(name) = file_config.bot_profile.name {
            self.bot_profile.name = name;
        }
        if let Some(avatar_path) = file_config.bot_profile.avatar_path {
            self.bot_profile.avatar_path = nonempty_path(avatar_path);
        }
        if let Some(state_path) = file_config.bot_profile.state_path {
            self.bot_profile.state_path = PathBuf::from(state_path);
        }
        if let Some(spool_read_ahead_mib) = file_config.technical.spool_read_ahead_mib {
            self.technical.spool_read_ahead_mib = spool_read_ahead_mib;
        }
        if let Some(collection_track_fetch_concurrency) =
            file_config.technical.collection_track_fetch_concurrency
        {
            self.technical.collection_track_fetch_concurrency = collection_track_fetch_concurrency;
        }
        if let Some(token_session_path) = file_config.tidal.token_session_path {
            self.tidal.path_to_session = token_session_path;
        }
        if let Some(client_id) = file_config.tidal.client_id {
            self.tidal.tidal_client_id = client_id;
        }
        if let Some(client_secret) = file_config.tidal.client_secret {
            self.tidal.tidal_client_secret = client_secret;
        }
        if let Some(user_agent) = file_config.tidal.user_agent {
            self.tidal.user_agent = user_agent;
        }
        if let Some(oauth_device_auth_url) = file_config.tidal.oauth_device_auth_url {
            self.tidal.oauth_device_auth_url = oauth_device_auth_url;
        }
        if let Some(oauth_token_url) = file_config.tidal.oauth_token_url {
            self.tidal.oauth_token_url = oauth_token_url;
        }
        if let Some(sessions_url) = file_config.tidal.sessions_url {
            self.tidal.sessions_url = sessions_url;
        }
        if let Some(search_url) = file_config.tidal.search_url {
            self.tidal.search_url = search_url;
        }

        Ok(())
    }

    fn apply_env_overrides(&mut self) -> Result<(), commands::Error> {
        apply_string(
            &mut self.command_prefix,
            &["TIDALCORDRS_COMMAND_PREFIX", "COMMAND_PREFIX"],
        )?;
        apply_bool(
            &mut self.bot_profile.enabled,
            &[
                "TIDALCORDRS_BOT_PROFILE__SYNC_ENABLED",
                "BOT_PROFILE_SYNC_ENABLED",
            ],
        )?;
        apply_string(
            &mut self.bot_profile.name,
            &["TIDALCORDRS_BOT_PROFILE__NAME", "BOT_NAME"],
        )?;
        apply_optional_path(
            &mut self.bot_profile.avatar_path,
            &["TIDALCORDRS_BOT_PROFILE__AVATAR_PATH", "BOT_AVATAR_PATH"],
        )?;
        apply_path(
            &mut self.bot_profile.state_path,
            &[
                "TIDALCORDRS_BOT_PROFILE__STATE_PATH",
                "BOT_PROFILE_STATE_PATH",
            ],
        )?;
        apply_parse(
            &mut self.technical.spool_read_ahead_mib,
            &[
                "TIDALCORDRS_TECHNICAL__SPOOL_READ_AHEAD_MIB",
                "SPOOL_READ_AHEAD_MIB",
            ],
        )?;
        apply_parse(
            &mut self.technical.collection_track_fetch_concurrency,
            &[
                "TIDALCORDRS_TECHNICAL__COLLECTION_TRACK_FETCH_CONCURRENCY",
                "COLLECTION_TRACK_FETCH_CONCURRENCY",
            ],
        )?;
        apply_string(
            &mut self.tidal.path_to_session,
            &[
                "TIDALCORDRS_TIDAL__TOKEN_SESSION_PATH",
                "TIDAL_TOKEN_SESSION_PATH",
            ],
        )?;
        apply_string(
            &mut self.tidal.tidal_client_id,
            &["TIDALCORDRS_TIDAL__CLIENT_ID", "TIDAL_CLIENT_ID"],
        )?;
        apply_string(
            &mut self.tidal.tidal_client_secret,
            &["TIDALCORDRS_TIDAL__CLIENT_SECRET", "TIDAL_CLIENT_SECRET"],
        )?;
        apply_string(
            &mut self.tidal.user_agent,
            &["TIDALCORDRS_TIDAL__USER_AGENT", "USER_AGENT"],
        )?;
        apply_string(
            &mut self.tidal.oauth_device_auth_url,
            &[
                "TIDALCORDRS_TIDAL__OAUTH_DEVICE_AUTH_URL",
                "OAUTH_DEVICE_AUTH_URL",
            ],
        )?;
        apply_string(
            &mut self.tidal.oauth_token_url,
            &["TIDALCORDRS_TIDAL__OAUTH_TOKEN_URL", "OAUTH_TOKEN_URL"],
        )?;
        apply_string(
            &mut self.tidal.sessions_url,
            &["TIDALCORDRS_TIDAL__SESSIONS_URL", "SESSIONS_URL"],
        )?;
        apply_string(
            &mut self.tidal.search_url,
            &["TIDALCORDRS_TIDAL__SEARCH_URL", "SEARCH_URL"],
        )?;

        Ok(())
    }

    fn validate(&self) -> Result<(), commands::Error> {
        if self.command_prefix.trim().is_empty() {
            return Err("command_prefix must not be empty".into());
        }
        if self.bot_profile.name.trim().is_empty() {
            return Err("bot_profile.name must not be empty".into());
        }
        if self.technical.collection_track_fetch_concurrency == 0 {
            return Err(
                "technical.collection_track_fetch_concurrency must be greater than 0".into(),
            );
        }
        self.technical.spool_read_ahead_bytes()?;
        Ok(())
    }
}

fn config_path() -> Result<Option<PathBuf>, commands::Error> {
    match std::env::var("CONFIG_PATH") {
        Ok(value) if value.trim().is_empty() => Ok(None),
        Ok(value) => Ok(Some(PathBuf::from(value))),
        Err(std::env::VarError::NotPresent) => {
            let default_path = PathBuf::from("config.toml");
            Ok(default_path.exists().then_some(default_path))
        }
        Err(std::env::VarError::NotUnicode(_)) => Err("CONFIG_PATH must be valid UTF-8".into()),
    }
}

fn required_env(name: &str) -> Result<String, commands::Error> {
    match std::env::var(name) {
        Ok(value) if value.trim().is_empty() => Err(format!("{name} must not be empty").into()),
        Ok(value) => Ok(value),
        Err(std::env::VarError::NotPresent) => Err(format!("{name} must be set").into()),
        Err(std::env::VarError::NotUnicode(_)) => Err(format!("{name} must be valid UTF-8").into()),
    }
}

fn env_override<'a>(names: &'a [&str]) -> Result<Option<(&'a str, String)>, commands::Error> {
    for name in names {
        match std::env::var(name) {
            Ok(value) if value.trim().is_empty() => {}
            Ok(value) => return Ok(Some((name, value))),
            Err(std::env::VarError::NotPresent) => {}
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(format!("{name} must be valid UTF-8").into());
            }
        }
    }
    Ok(None)
}

fn apply_string(target: &mut String, names: &[&str]) -> Result<(), commands::Error> {
    if let Some((_, value)) = env_override(names)? {
        *target = value;
    }
    Ok(())
}

fn apply_bool(target: &mut bool, names: &[&str]) -> Result<(), commands::Error> {
    apply_parse(target, names)
}

fn apply_parse<T>(target: &mut T, names: &[&str]) -> Result<(), commands::Error>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    if let Some((name, value)) = env_override(names)? {
        *target = value
            .parse::<T>()
            .map_err(|error| format!("Failed to parse {name}: {error}"))?;
    }
    Ok(())
}

fn apply_path(target: &mut PathBuf, names: &[&str]) -> Result<(), commands::Error> {
    if let Some((_, value)) = env_override(names)? {
        *target = PathBuf::from(value);
    }
    Ok(())
}

fn apply_optional_path(
    target: &mut Option<PathBuf>,
    names: &[&str],
) -> Result<(), commands::Error> {
    if let Some((_, value)) = env_override(names)? {
        *target = nonempty_path(value);
    }
    Ok(())
}

fn nonempty_path(value: String) -> Option<PathBuf> {
    (!value.trim().is_empty()).then(|| PathBuf::from(value))
}
