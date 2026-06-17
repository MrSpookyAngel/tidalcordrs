mod commands;
mod config;
mod ffmpeg_spool;
mod session;
mod track;
mod url_handler;

use config::BotProfileConfig;
use poise::serenity_prelude as serenity;
use serde::{Deserialize, Serialize};
use songbird::SerenityInit;
use std::path::Path;
use std::process::ExitCode;

const DEFAULT_BOT_AVATAR_FILE_NAME: &str = "default-avatar.png";
const DEFAULT_BOT_AVATAR_SOURCE: &str = "embedded:default-avatar.png";
const DEFAULT_BOT_AVATAR_BYTES: &[u8] = include_bytes!("../assets/default-avatar.png");

#[derive(Debug)]
struct BotProfileAvatar {
    bytes: Vec<u8>,
    file_name: String,
    source: String,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct BotProfileState {
    bot_name: Option<String>,
    avatar_source: Option<String>,
    avatar_fingerprint: Option<String>,
    discord_avatar_hash: Option<String>,
}

async fn event_handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, commands::Data, commands::Error>,
    data: &commands::Data,
) -> Result<(), commands::Error> {
    if let serenity::FullEvent::VoiceStateUpdate { old: _, new } = event {
        let guild_id = match new.guild_id {
            Some(id) => id,
            None => return Ok(()),
        };

        let bot_id = ctx.cache.current_user().id;

        let (bot_channel_id, users_in_channel) = {
            let guild = match ctx.cache.guild(guild_id) {
                Some(g) => g,
                None => return Ok(()),
            };

            let channel_id = match guild.voice_states.get(&bot_id).and_then(|vs| vs.channel_id) {
                Some(id) => id,
                None => return Ok(()),
            };

            let users = guild
                .voice_states
                .values()
                .filter(|vs| vs.channel_id == Some(channel_id))
                .count();

            (channel_id, users)
        };

        // If only 1 user inside channel (should be the bot)
        if users_in_channel == 1 {
            let ctx_clone = ctx.clone();
            let playback_status = data.playback_status.clone();

            tokio::spawn(async move {
                // Wait 5 minutes
                tokio::time::sleep(tokio::time::Duration::from_secs(300)).await;

                let should_disconnect = {
                    if let Some(guild) = ctx_clone.cache.guild(guild_id) {
                        let current_users = guild
                            .voice_states
                            .values()
                            .filter(|vs| vs.channel_id == Some(bot_channel_id))
                            .count();

                        let bot_still_in_channel =
                            guild.voice_states.get(&bot_id).and_then(|vs| vs.channel_id)
                                == Some(bot_channel_id);

                        bot_still_in_channel && current_users == 1
                    } else {
                        false
                    }
                };

                // Disconnect if bot is the remaining user in a voice channel after 5 minutes
                if should_disconnect && let Some(manager) = songbird::get(&ctx_clone).await {
                    let _ = manager.remove(guild_id).await;
                    commands::clear_playback_status_for_guild(
                        &ctx_clone,
                        playback_status,
                        guild_id,
                    )
                    .await;
                    tracing::info!(guild_id = %guild_id, "Left voice channel due to inactivity");
                }
            });
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    init_logging();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "Application error");
            ExitCode::FAILURE
        }
    }
}

fn init_logging() {
    tracing_subscriber::fmt()
        .with_timer(tracing_subscriber::fmt::time::LocalTime::rfc_3339())
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("tidalcordrs=info")),
        )
        .init();
}

async fn sync_bot_profile(
    ctx: &serenity::Context,
    ready_user: &serenity::CurrentUser,
    config: &BotProfileConfig,
) -> Result<serenity::CurrentUser, commands::Error> {
    if !config.enabled {
        tracing::info!(user = %ready_user.name, "Bot profile sync disabled");
        return Ok(ready_user.clone());
    }

    let avatar = load_bot_profile_avatar(config).await?;
    let avatar_fingerprint = fingerprint_bytes(&avatar.bytes);
    let state = read_bot_profile_state(&config.state_path)?;
    let current_avatar_hash = ready_user.avatar.as_ref().map(ToString::to_string);

    let should_update_name = ready_user.name != config.name;
    let avatar_is_current = match (
        state.avatar_fingerprint.as_deref(),
        state.discord_avatar_hash.as_deref(),
        current_avatar_hash.as_deref(),
    ) {
        (Some(stored_fingerprint), Some(stored_discord_hash), Some(current_discord_hash)) => {
            stored_fingerprint == avatar_fingerprint && stored_discord_hash == current_discord_hash
        }
        _ => false,
    };
    let should_update_avatar = !avatar_is_current;

    if !should_update_name && !should_update_avatar {
        return Ok(ready_user.clone());
    }

    let mut edit_profile = serenity::EditProfile::new();
    if should_update_name {
        edit_profile = edit_profile.username(config.name.clone());
    }

    let avatar_attachment = should_update_avatar
        .then(|| serenity::CreateAttachment::bytes(avatar.bytes, avatar.file_name));
    if let Some(avatar_attachment) = &avatar_attachment {
        edit_profile = edit_profile.avatar(avatar_attachment);
    }

    let mut current_user = ready_user.clone();
    current_user
        .edit(ctx, edit_profile)
        .await
        .map_err(|error| {
            format!(
                "Failed to update Discord bot profile for {}: {error}",
                current_user.name
            )
        })?;

    let updated_avatar_hash = current_user.avatar.as_ref().map(ToString::to_string);
    write_bot_profile_state(
        &config.state_path,
        &BotProfileState {
            bot_name: Some(config.name.clone()),
            avatar_source: Some(avatar.source),
            avatar_fingerprint: Some(avatar_fingerprint),
            discord_avatar_hash: updated_avatar_hash,
        },
    )?;

    if should_update_name && should_update_avatar {
        tracing::info!(user = %current_user.name, "Updated bot name and avatar");
    } else if should_update_name {
        tracing::info!(user = %current_user.name, "Updated bot name");
    } else {
        tracing::info!(user = %current_user.name, "Updated bot avatar");
    }

    Ok(current_user)
}

async fn load_bot_profile_avatar(
    config: &BotProfileConfig,
) -> Result<BotProfileAvatar, commands::Error> {
    let Some(avatar_path) = &config.avatar_path else {
        return Ok(BotProfileAvatar {
            bytes: DEFAULT_BOT_AVATAR_BYTES.to_vec(),
            file_name: DEFAULT_BOT_AVATAR_FILE_NAME.to_string(),
            source: DEFAULT_BOT_AVATAR_SOURCE.to_string(),
        });
    };

    let bytes = tokio::fs::read(avatar_path).await.map_err(|error| {
        format!(
            "Failed to read BOT_AVATAR_PATH {}: {error}",
            avatar_path.display()
        )
    })?;
    let file_name = avatar_path
        .file_name()
        .ok_or("BOT_AVATAR_PATH must point to a file")?
        .to_string_lossy()
        .to_string();

    Ok(BotProfileAvatar {
        bytes,
        file_name,
        source: avatar_path.display().to_string(),
    })
}

fn read_bot_profile_state(path: &Path) -> Result<BotProfileState, commands::Error> {
    match std::fs::File::open(path) {
        Ok(file) => Ok(serde_json::from_reader(file)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(BotProfileState::default())
        }
        Err(error) => Err(format!("Failed to read {}: {error}", path.display()).into()),
    }
}

fn write_bot_profile_state(path: &Path, state: &BotProfileState) -> Result<(), commands::Error> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    let file = std::fs::File::create(path)
        .map_err(|error| format!("Failed to create {}: {error}", path.display()))?;
    serde_json::to_writer_pretty(file, state)?;
    Ok(())
}

fn fingerprint_bytes(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

async fn run() -> Result<(), commands::Error> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Load environment variables from .env file if it exists
    dotenvy::dotenv_override().ok();
    let app_config = config::AppConfig::load()?;
    let token = app_config.discord_token.clone();
    let prefix = app_config.command_prefix.clone();
    let bot_profile_config = app_config.bot_profile.clone();
    let spool_read_ahead_bytes = app_config.technical.spool_read_ahead_bytes()?;
    let collection_track_fetch_concurrency =
        app_config.technical.collection_track_fetch_concurrency;
    let version = env!("CARGO_PKG_VERSION");

    // Initialize the Tidal session
    let tidal_session = session::Session::new(app_config.tidal).await?;

    // Set the intents
    let intents = serenity::GatewayIntents::GUILDS
        | serenity::GatewayIntents::GUILD_MESSAGES
        | serenity::GatewayIntents::GUILD_VOICE_STATES
        | serenity::GatewayIntents::MESSAGE_CONTENT;

    // Create a new Poise framework instance
    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                commands::help(),
                commands::ping(),
                commands::join(),
                commands::volume(),
                commands::play(),
                commands::search(),
                commands::playnext(),
                commands::pause(),
                commands::resume(),
                commands::seek(),
                commands::skip(),
                commands::repeat(),
                commands::shuffle(),
                commands::remove(),
                commands::clear(),
                commands::stop(),
                commands::current(),
                commands::leave(),
                commands::queue(),
            ],
            prefix_options: poise::PrefixFrameworkOptions {
                prefix: Some(prefix.clone()),
                ..Default::default()
            },
            event_handler: |ctx, event, framework, data| {
                Box::pin(event_handler(ctx, event, framework, data))
            },
            ..Default::default()
        })
        .setup(move |ctx, ready, framework| {
            Box::pin(async move {
                let current_user = sync_bot_profile(ctx, &ready.user, &bot_profile_config).await?;

                for guild_status in &ready.guilds {
                    poise::builtins::register_in_guild(
                        ctx,
                        &framework.options().commands,
                        guild_status.id,
                    )
                    .await?;
                }
                tracing::info!(user = %current_user.name, version, "Bot connected");
                Ok(commands::Data {
                    session: tokio::sync::Mutex::new(tidal_session),
                    spool_read_ahead_bytes,
                    collection_track_fetch_concurrency,
                    command_prefix: prefix.clone(),
                    repeat_modes: std::sync::Arc::new(tokio::sync::Mutex::new(
                        std::collections::HashMap::new(),
                    )),
                    playback_status: std::sync::Arc::new(tokio::sync::Mutex::new(
                        commands::PlaybackStatusState::default(),
                    )),
                })
            })
        })
        .build();

    // Create new client
    let mut client = serenity::Client::builder(&token, intents)
        .framework(framework)
        .register_songbird()
        .await
        .map_err(|error| format!("Error creating client: {error}"))?;

    let shard_manager = client.shard_manager.clone();

    // Start the client
    let client_task = tokio::spawn(async move {
        if let Err(why) = client.start().await {
            tracing::error!(error = ?why, "Client error");
        }
    });

    // Handle Ctrl+C to gracefully shut down the client
    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                tracing::info!("Ctrl+C received, shutting down");
                shard_manager.shutdown_all().await;
                tracing::info!("Shutdown complete");
            }
            Err(error) => {
                tracing::error!(%error, "Failed to listen for Ctrl+C");
            }
        }
    });

    // Wait for the client task to finish
    client_task
        .await
        .map_err(|error| format!("Client task failed to complete: {error}"))?;

    Ok(())
}
