pub struct Data {
    pub session: tokio::sync::Mutex<crate::session::Session>,
}
pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

#[poise::command(slash_command, prefix_command)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("Pong!").await?;
    Ok(())
}

#[poise::command(slash_command, prefix_command, aliases("join", "j"))]
pub async fn join(ctx: Context<'_>) -> Result<(), Error> {
    // Get the voice states from the guild
    let voice_states = if let Some(guild) = ctx.guild() {
        guild.voice_states.clone()
    } else {
        ctx.say("Voice states not available").await?;
        return Ok(());
    };

    // Get the guild ID
    let guild_id = match ctx.guild_id() {
        Some(guild_id) => guild_id,
        None => {
            ctx.say("This command can only be used in a guild").await?;
            return Ok(());
        }
    };

    // Get the current voice channel ID of the user
    let channel_id = match voice_states
        .get(&ctx.author().id)
        .and_then(|vs| vs.channel_id)
    {
        Some(channel_id) => channel_id,
        None => {
            ctx.say("You must be in a voice channel to use this command")
                .await?;
            return Ok(());
        }
    };

    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("Songbird voice manager not found");

    let call = manager.join(guild_id, channel_id).await;

    match call {
        Ok(_) => {
            ctx.say("Joined the voice channel!").await?;
        }
        Err(e) => {
            ctx.say(e.to_string()).await?;
        }
    }

    Ok(())
}

#[poise::command(slash_command, prefix_command, aliases("play", "p"))]
pub async fn play(
    ctx: Context<'_>,
    #[description = "Provide the query or url of a song"]
    #[rest]
    query_or_url: String,
) -> Result<(), Error> {
    let mut session = ctx.data().session.lock().await;

    let tracks = session
        .find_tracks(&query_or_url, 1)
        .await
        .map_err(|e| Error::from(e.to_string()))?;

    ctx.say(format!("Found {} tracks", tracks.len())).await?;

    if !(tracks.is_empty()) {
        let track = tracks.first().unwrap();
        ctx.say(format!("Playing track: {}", track.title)).await?;
        
        println!("Track details: {:?}", track);
    } else {
        ctx.say("No tracks found for the given query").await?;
    }

    Ok(())
}
