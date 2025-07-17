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
