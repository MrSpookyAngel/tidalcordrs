use songbird::input::Compose;

pub struct Data {
    pub session: tokio::sync::Mutex<crate::session::Session>,
    pub storage: tokio::sync::Mutex<crate::storage::Storage>,
}
pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

struct TrackErrorNotifier;

#[serenity::async_trait]
impl songbird::events::EventHandler for TrackErrorNotifier {
    async fn act(
        &self,
        ctx: &songbird::events::EventContext<'_>,
    ) -> Option<songbird::events::Event> {
        if let songbird::events::EventContext::Track(track_list) = ctx {
            for (state, handle) in *track_list {
                println!(
                    "Track {:?} encountered an error: {:?}",
                    handle.uuid(),
                    state.playing
                );
            }
        }

        None
    }
}

fn get_formatted_track(track: &crate::track::Track) -> String {
    let hours = track.duration / 3600;
    let minutes = (track.duration % 3600) / 60;
    let seconds = track.duration % 60;
    let duration = if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}", minutes, seconds)
    };
    let featured = if track.featured_artists.is_empty() {
        String::new()
    } else {
        format!(" ft. {}", track.featured_artists.join(", "))
    };
    let need_featured = !featured.is_empty()
        && !track.title.to_lowercase().contains("feat.")
        && !track.title.to_lowercase().contains("ft.");
    format!(
        "{} - {}{} ({})",
        track.artist,
        track.title,
        if need_featured {
            featured
        } else {
            String::new()
        },
        duration
    )
}

#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("Pong!").await?;
    Ok(())
}

async fn try_join_voice_channel(ctx: Context<'_>) -> Result<(), Error> {
    // Get the songbird voice manager
    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("Songbird voice manager not found");

    // Get the guild ID
    let guild_id = match ctx.guild_id() {
        Some(guild_id) => guild_id,
        None => {
            ctx.say("This command can only be used in a guild.").await?;
            return Ok(());
        }
    };

    // Check if already connected to a voice channel
    if manager.get(guild_id).is_some() {
        println!(
            "Already connected to a voice channel. Guild ID: {}",
            guild_id
        );
        return Ok(());
    }

    // Get the voice states from the guild
    let voice_states = if let Some(guild) = ctx.guild() {
        guild.voice_states.clone()
    } else {
        ctx.say("Voice states not available.").await?;
        return Ok(());
    };

    // Get the current voice channel ID of the user
    let channel_id = match voice_states
        .get(&ctx.author().id)
        .and_then(|vs| vs.channel_id)
    {
        Some(channel_id) => channel_id,
        None => {
            ctx.say("You must be in a voice channel to use this command.")
                .await?;
            return Ok(());
        }
    };

    // Join the voice channel
    if let Ok(handler_lock) = manager.join(guild_id, channel_id).await {
        let mut handler = handler_lock.lock().await;
        handler.add_global_event(
            songbird::events::TrackEvent::Error.into(),
            TrackErrorNotifier,
        );
        println!(
            "Joined the voice channel! Guild ID: {}, Channel ID: {}",
            guild_id, channel_id
        );
    } else {
        ctx.say("Failed to join the voice channel.").await?;
        return Ok(());
    }

    Ok(())
}

#[poise::command(slash_command, prefix_command, aliases("join", "j"), guild_only)]
pub async fn join(ctx: Context<'_>) -> Result<(), Error> {
    // Attempt to join the voice channel if not already connected
    try_join_voice_channel(ctx).await
}

#[poise::command(slash_command, prefix_command, aliases("volume", "vol"), guild_only)]
pub async fn volume(ctx: Context<'_>, volume: u8) -> Result<(), Error> {
    // Get the guild ID
    let guild_id = match ctx.guild_id() {
        Some(guild_id) => guild_id,
        None => {
            ctx.say("This command can only be used in a guild.").await?;
            return Ok(());
        }
    };

    // Get the songbird voice manager
    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("Songbird voice manager not found");

    // Get the voice channel handler
    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;

        // Set the volume
        handler.queue().current().as_mut().map(|track_handle| {
            let _ = track_handle.set_volume(volume as f32 / 100.0);
        });

        ctx.say(format!("Volume set to {}.", volume)).await?;
    } else {
        ctx.say("Not connected to a voice channel.").await?;
    }

    Ok(())
}

#[poise::command(slash_command, prefix_command, aliases("play", "p"), guild_only)]
pub async fn play(
    ctx: Context<'_>,
    #[description = "Provide the query or url of a song"]
    #[rest]
    query_or_url: String,
) -> Result<(), Error> {
    // Attempt to join the voice channel if not already connected
    try_join_voice_channel(ctx.clone()).await?;

    let mut session = ctx.data().session.lock().await;

    // Find tracks using the Tidal session
    let tracks = session
        .find_tracks(&query_or_url, 1)
        .await
        .map_err(|e| Error::from(e.to_string()))?;

    // Get the first track from the search results
    let first_track = match tracks.first() {
        Some(track) => track,
        None => {
            ctx.say("No track was found.").await?;
            return Ok(());
        }
    };

    // Get the guild ID
    let guild_id = match ctx.guild_id() {
        Some(guild_id) => guild_id,
        None => {
            ctx.say("This command can only be used in a guild.").await?;
            return Ok(());
        }
    };

    // Get the songbird voice manager
    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("Songbird voice manager not found")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;

        // Verify ffmpeg is installed
        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_err()
        {
            println!("FFmpeg is not installed or not found in the system PATH");
            ctx.say("Error: check logs for details.").await?;

            return Ok(());
        }

        // Create the directory for storing tracks if it doesn't exist
        std::fs::create_dir_all("data/tracks").expect("Failed to create tracks directory");

        // Use ffmpeg to download and convert the audio stream to opus format
        let file_path = format!("data/tracks/{}.opus", first_track.id);
        let output = std::process::Command::new("ffmpeg")
            .args([
                "-i",
                &first_track.stream_url,
                "-c:a",
                "libopus",
                "-f",
                "opus",
                &file_path,
            ])
            .output()
            .expect("Failed to start ffmpeg process");
        if !output.status.success() {
            ctx.say("Failed to download and/or convert audio stream with ffmpeg.")
                .await?;
            return Ok(());
        }

        // Create a songbird stream from the downloaded file
        let stream = songbird::input::File::new(file_path);
        let data = std::sync::Arc::new(first_track.clone());
        let track = songbird::tracks::Track::new_with_data(stream.into(), data);

        // Add the track to the queue
        let _ = handler.enqueue(track).await;

        ctx.say(format!(
            "{} added **{}** to the queue.",
            ctx.author().name,
            get_formatted_track(first_track)
        ))
        .await?;
    } else {
        ctx.say("Not connected to a voice channel.").await?;
        return Ok(());
    }

    Ok(())
}

#[poise::command(slash_command, prefix_command, aliases("pause"), guild_only)]
pub async fn pause(ctx: Context<'_>) -> Result<(), Error> {
    // Get the guild ID
    let guild_id = match ctx.guild_id() {
        Some(guild_id) => guild_id,
        None => {
            ctx.say("This command can only be used in a guild.").await?;
            return Ok(());
        }
    };

    // Get the songbird voice manager
    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("Songbird voice manager not found")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;

        // Pause the playback
        let _ = handler.queue().pause();

        ctx.say("Paused the playback.").await?;
    } else {
        ctx.say("Not connected to a voice channel.").await?;
    }

    Ok(())
}

#[poise::command(slash_command, prefix_command, aliases("skip", "s"), guild_only)]
pub async fn skip(ctx: Context<'_>) -> Result<(), Error> {
    // Get the guild ID
    let guild_id = match ctx.guild_id() {
        Some(guild_id) => guild_id,
        None => {
            ctx.say("This command can only be used in a guild.").await?;
            return Ok(());
        }
    };

    // Get the songbird voice manager
    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("Songbird voice manager not found")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;

        // Skip the current track
        let _ = handler.queue().skip();

        ctx.say("Skipped the current track.").await?;
    } else {
        ctx.say("Not connected to a voice channel.").await?;
    }

    Ok(())
}

#[poise::command(slash_command, prefix_command, aliases("stop"), guild_only)]
pub async fn stop(ctx: Context<'_>) -> Result<(), Error> {
    // Get the guild ID
    let guild_id = match ctx.guild_id() {
        Some(guild_id) => guild_id,
        None => {
            ctx.say("This command can only be used in a guild.").await?;
            return Ok(());
        }
    };

    // Get the songbird voice manager
    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("Songbird voice manager not found")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;

        // Stop the playback and clear the queue
        let _ = handler.queue().stop();

        ctx.say("Stopped the playback and cleared the queue.")
            .await?;
    } else {
        ctx.say("Not connected to a voice channel.").await?;
    }

    Ok(())
}

#[poise::command(
    slash_command,
    prefix_command,
    aliases("current", "currentplaying", "now", "nowplaying", "playing"),
    guild_only
)]
pub async fn current(ctx: Context<'_>) -> Result<(), Error> {
    // Get the guild ID
    let guild_id = match ctx.guild_id() {
        Some(guild_id) => guild_id,
        None => {
            ctx.say("This command can only be used in a guild.").await?;
            return Ok(());
        }
    };

    // Get the songbird voice manager
    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("Songbird voice manager not found")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;

        // Get the current playing track
        if let Some(track_handle) = handler.queue().current() {
            let track = track_handle.data::<crate::track::Track>().clone();
            ctx.say(format!(
                "Current track: **{}**",
                get_formatted_track(&track)
            ))
            .await?;
        } else {
            ctx.say("No track is currently playing.").await?;
        }
    } else {
        ctx.say("Not connected to a voice channel.").await?;
    }

    Ok(())
}

#[poise::command(
    slash_command,
    prefix_command,
    aliases("leave", "disconnect"),
    guild_only
)]
pub async fn leave(ctx: Context<'_>) -> Result<(), Error> {
    // Get the guild ID
    let guild_id = match ctx.guild_id() {
        Some(guild_id) => guild_id,
        None => {
            ctx.say("This command can only be used in a guild.").await?;
            return Ok(());
        }
    };

    // Get the songbird voice manager
    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("Songbird voice manager not found")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;

        // Leave the voice channel
        let _ = handler.leave().await;

        println!("Left the voice channel. Guild ID: {}", guild_id);
    } else {
        ctx.say("Not connected to a voice channel.").await?;
    }

    Ok(())
}
