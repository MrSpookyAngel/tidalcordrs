mod commands;
mod ffmpeg_spool;
mod session;
mod track;
mod url_handler;

use poise::serenity_prelude as serenity;
use songbird::SerenityInit;
use std::process::ExitCode;

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

fn required_env(name: &str) -> Result<String, commands::Error> {
    match std::env::var(name) {
        Ok(value) => Ok(value),
        Err(std::env::VarError::NotPresent) => Err(format!("{name} must be set").into()),
        Err(std::env::VarError::NotUnicode(_)) => Err(format!("{name} must be valid UTF-8").into()),
    }
}

fn optional_env_parse<T>(name: &str, default: T) -> Result<T, commands::Error>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match std::env::var(name) {
        Ok(value) => value
            .parse::<T>()
            .map_err(|error| format!("Failed to parse {name}: {error}").into()),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(std::env::VarError::NotUnicode(_)) => Err(format!("{name} must be valid UTF-8").into()),
    }
}

async fn run() -> Result<(), commands::Error> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Load environment variables from .env file if it exists
    dotenvy::dotenv_override().ok();
    let token = required_env("DISCORD_TOKEN")?;
    let prefix = required_env("COMMAND_PREFIX")?;
    let spool_read_ahead_bytes = optional_env_parse::<u64>("SPOOL_READ_AHEAD_MIB", 16)?
        .checked_mul(1024 * 1024)
        .ok_or("SPOOL_READ_AHEAD_MIB is too large")?;
    let collection_track_fetch_concurrency = optional_env_parse::<usize>(
        "COLLECTION_TRACK_FETCH_CONCURRENCY",
        session::DEFAULT_COLLECTION_TRACK_FETCH_CONCURRENCY,
    )?;
    if collection_track_fetch_concurrency == 0 {
        return Err("COLLECTION_TRACK_FETCH_CONCURRENCY must be greater than 0".into());
    }
    let version = env!("CARGO_PKG_VERSION");

    // Initialize the Tidal session
    let tidal_session = session::Session::new().await?;

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
                for guild_status in &ready.guilds {
                    poise::builtins::register_in_guild(
                        ctx,
                        &framework.options().commands,
                        guild_status.id,
                    )
                    .await?;
                }
                tracing::info!(user = %ready.user.name, version, "Bot connected");
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
