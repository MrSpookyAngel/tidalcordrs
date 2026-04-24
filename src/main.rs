mod commands;
mod session;
mod storage;
mod track;
mod url_handler;

use poise::serenity_prelude as serenity;
use songbird::SerenityInit;

async fn event_handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, commands::Data, commands::Error>,
    _data: &commands::Data,
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
                if should_disconnect {
                    if let Some(manager) = songbird::get(&ctx_clone).await {
                        let _ = manager.remove(guild_id).await;
                        println!(
                            "Left voice channel in guild {} due to 5 minutes of inactivity",
                            guild_id
                        );
                    }
                }
            });
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Load environment variables from .env file if it exists
    dotenvy::dotenv_override().ok();
    let token = std::env::var("DISCORD_TOKEN").expect("Expected a token in the environment");
    let prefix =
        std::env::var("COMMAND_PREFIX").expect("Expected a command prefix in the environment");
    let storage_dir =
        std::env::var("STORAGE_DIR").expect("Expected a storage directory in the environment");
    let storage_max_size = std::env::var("STORAGE_MAX_SIZE_BYTES")
        .expect("Expected a max size in the environment")
        .parse()
        .expect("Failed to parse STORAGE_MAX_SIZE_BYTES");

    let storage = storage::LRUStorage::new(&storage_dir, storage_max_size);

    // Initialize the Tidal session
    let tidal_session = session::Session::new().await;

    // Set the intents
    let intents = serenity::GatewayIntents::GUILDS
        | serenity::GatewayIntents::GUILD_MESSAGES
        | serenity::GatewayIntents::GUILD_VOICE_STATES
        | serenity::GatewayIntents::MESSAGE_CONTENT;

    // Create a new Poise framework instance
    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                commands::ping(),
                commands::join(),
                commands::volume(),
                commands::play(),
                commands::pause(),
                commands::resume(),
                commands::skip(),
                commands::stop(),
                commands::current(),
                commands::leave(),
                commands::queue(),
            ],
            prefix_options: poise::PrefixFrameworkOptions {
                prefix: Some(prefix),
                ..Default::default()
            },
            event_handler: |ctx, event, framework, data| {
                Box::pin(event_handler(ctx, event, framework, data))
            },
            ..Default::default()
        })
        .setup(|_ctx, ready, _framework| {
            Box::pin(async move {
                println!("{} is connected!", ready.user.name);
                Ok(commands::Data {
                    session: tokio::sync::Mutex::new(tidal_session),
                    storage: tokio::sync::Mutex::new(storage),
                })
            })
        })
        .build();

    // Create new client
    let mut client = serenity::Client::builder(&token, intents)
        .framework(framework)
        .register_songbird()
        .await
        .expect("Error creating client");

    let shard_manager = client.shard_manager.clone();

    // Start the client
    let client_task = tokio::spawn(async move {
        if let Err(why) = client.start().await {
            println!("Client error: {why:?}");
        }
    });

    // Handle Ctrl+C to gracefully shut down the client
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for Ctrl+C");
        println!("Ctrl+C received, shutting down...");
        shard_manager.shutdown_all().await;
        println!("Shutdown complete.");
        std::process::exit(0);
    });

    // Wait for the client task to finish
    client_task.await.expect("Client task failed to complete");
}
