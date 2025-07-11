mod commands;
mod session;
mod storage;
mod track;

use poise::serenity_prelude as serenity;
use songbird::SerenityInit;

#[tokio::main]
async fn main() {
    // Load environment variables from .env file if it exists
    dotenvy::dotenv_override().ok();

    // Retrieve the Discord token from the environment
    let token = std::env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    // Retrieve command prefix from the environment, defaulting to "!"
    let prefix = std::env::var("COMMAND_PREFIX").unwrap_or_else(|_| "!".to_string());

    // Retrieve the cache directory from the environment, defaulting to "data/cache"
    let cache_dir = std::env::var("CACHE_DIR").unwrap_or_else(|_| "data/cache".to_string());

    // Retrieve the maximum cache size from the environment, defaulting to 20,000
    let cache_max_size: usize = std::env::var("CACHE_MAX_SIZE")
        .unwrap_or("20000".to_string())
        .parse()
        .expect("Expected a valid number for CACHE_MAX_SIZE");

    // Initialize the Tidal session
    let mut tidal_session = session::Session::new();
    tidal_session
        .start()
        .await
        .expect("Failed to start Tidal session");

    let storage = storage::Storage::new(&cache_dir, cache_max_size)
        .await
        .expect("Failed to create storage");

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
                commands::play(),
                commands::join(),
                commands::skip(),
            ],
            prefix_options: poise::PrefixFrameworkOptions {
                prefix: Some(prefix),
                ..Default::default()
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
