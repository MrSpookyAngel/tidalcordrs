use songbird::tracks::PlayMode;

pub struct Data {
    pub session: tokio::sync::Mutex<crate::session::Session>,
    pub storage: tokio::sync::Mutex<crate::storage::LRUStorage>,
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

async fn try_join_voice_channel(ctx: Context<'_>) -> Result<bool, Error> {
    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("Songbird voice manager not found");

    let guild_id = match ctx.guild_id() {
        Some(guild_id) => guild_id,
        None => {
            ctx.say("This command can only be used in a guild.").await?;
            return Ok(false);
        }
    };

    // Get the voice states from the guild
    let voice_states = if let Some(guild) = ctx.guild() {
        guild.voice_states.clone()
    } else {
        ctx.say("Voice states not available.").await?;
        return Ok(false);
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
            return Ok(false);
        }
    };

    // Check if the bot is already in the same channel
    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;
        if handler.current_channel() == Some(channel_id.into()) {
            return Ok(true);
        }
    }

    // Join (or move to) the voice channel
    if let Ok(handler_lock) = manager.join(guild_id, channel_id).await {
        let mut handler = handler_lock.lock().await;
        handler.add_global_event(
            songbird::events::TrackEvent::Error.into(),
            TrackErrorNotifier,
        );
        Ok(true)
    } else {
        ctx.say("Failed to join the voice channel.").await?;
        Ok(false)
    }
}

#[poise::command(slash_command, prefix_command, aliases("join", "j"), guild_only)]
pub async fn join(ctx: Context<'_>) -> Result<(), Error> {
    // Attempt to join the voice channel if not already connected
    try_join_voice_channel(ctx).await?;

    Ok(())
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

async fn download_to_bytes(url: &str) -> Result<Vec<u8>, std::io::Error> {
    // Verify ffmpeg is installed
    if std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_err()
    {
        println!("FFmpeg is not installed or not found in the system PATH");

        return Ok(vec![]);
    }

    // Use ffmpeg to download and convert the audio stream to opus format
    let output = std::process::Command::new("ffmpeg")
        .args([
            "-i",
            &url,
            "-c:a",
            "libopus",
            "-f",
            "opus",
            "pipe:1",
            "-loglevel",
            "error",
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()?
        .wait_with_output()
        .expect("Failed to start ffmpeg process");

    if !output.status.success() {
        println!("Failed to download and/or convert audio stream with ffmpeg.");
    }

    Ok(output.stdout)
}

async fn enqueue_track(
    ctx: &Context<'_>,
    handler: &mut songbird::Call,
    track: &crate::track::Track,
) -> Result<(), Error> {
    let storage = ctx.data().storage.lock().await;
    let file_name = format!("{}.opus", track.id);
    let file_path = match storage.exists(&file_name).await {
        true => {
            println!("Track already exists in storage: {}", file_name);
            storage.storage_dir.join(file_name)
        }
        false => {
            let file_bytes = download_to_bytes(&track.stream_url).await?;
            let file_path = storage.storage_dir.join(&file_name);
            storage.insert(file_name, file_bytes).await?;
            file_path
        }
    };
    drop(storage);

    let stream = songbird::input::File::new(file_path);
    let data = std::sync::Arc::new(track.clone());
    let songbird_track = songbird::tracks::Track::new_with_data(stream.into(), data);

    let _ = handler.enqueue(songbird_track).await;

    Ok(())
}

#[poise::command(slash_command, prefix_command, aliases("play", "p"), guild_only)]
pub async fn play(
    ctx: Context<'_>,
    #[description = "Provide the query or url of a song"]
    #[rest]
    query_or_url: Option<String>,
) -> Result<(), Error> {
    // Attempt to join the voice channel if not already connected
    if !try_join_voice_channel(ctx.clone()).await? {
        return Ok(());
    }

    let guild_id = ctx.guild_id().ok_or("Must be in a guild")?;
    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("Songbird voice manager not found")
        .clone();

    if query_or_url.is_none() {
        if let Some(handler_lock) = manager.get(guild_id) {
            let handler = handler_lock.lock().await;
            let current = handler.queue().current();
            if let Some(track_handle) = current {
                let track_info = track_handle.get_info().await?;
                match track_info.playing {
                    PlayMode::Pause => {
                        if handler.queue().resume().is_ok() {
                            ctx.say("Resumed the playback.").await?;
                        } else {
                            ctx.say("Failed to resume the playback.").await?;
                        }
                    }
                    PlayMode::Play => {
                        ctx.say("Track already playing.").await?;
                    }
                    _ => {
                        // Do nothing
                    }
                }
            }
        } else {
            // Probably shouldn't happen since the bot would join in try_join_voice_channel
            ctx.say("I'm not in a voice channel.").await?;
        }
        return Ok(());
    }

    let query = query_or_url.unwrap();

    let _ = ctx.defer().await;

    try_join_voice_channel(ctx.clone()).await?;

    let mut session = ctx.data().session.lock().await;

    let mut tracks = crate::url_handler::handle_url(&mut *session, &query).await?;

    if tracks.is_empty() {
        tracks = {
            let tracks = session
                .find_tracks(&query, 1)
                .await
                .map_err(|e| Error::from(e.to_string()))?;

            if tracks.is_empty() {
                ctx.say("No track was found on Tidal.").await?;
                return Ok(());
            }

            tracks
        };
    }
    drop(session);

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;

        for track in &tracks {
            enqueue_track(&ctx, &mut handler, track).await?;
        }

        if tracks.len() == 1 {
            ctx.say(format!(
                "{} added **{}** to the queue.",
                ctx.author().name,
                get_formatted_track(&tracks[0])
            ))
            .await?;
        } else {
            ctx.say(format!(
                "{} added **{} tracks** to the queue.",
                ctx.author().name,
                tracks.len()
            ))
            .await?;
        }
    } else {
        ctx.say("Not connected to a voice channel.").await?;
        return Ok(());
    }

    Ok(())
}

#[poise::command(slash_command, prefix_command, aliases("pause", "wait"), guild_only)]
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

#[poise::command(
    slash_command,
    prefix_command,
    aliases("resume", "unpause", "continue"),
    guild_only
)]
pub async fn resume(ctx: Context<'_>) -> Result<(), Error> {
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
        let current = handler.queue().current();
        if let Some(track_handle) = current {
            let track_info = track_handle.get_info().await?;
            match track_info.playing {
                PlayMode::Pause => {
                    if handler.queue().resume().is_ok() {
                        ctx.say("Resumed the playback.").await?;
                    } else {
                        ctx.say("Failed to resume the playback.").await?;
                    }
                }
                PlayMode::Play => {
                    ctx.say("Track already playing.").await?;
                }
                _ => {
                    // Do nothing
                }
            }
        }
    } else {
        ctx.say("I'm not in a voice channel.").await?;
    }

    Ok(())
}

#[poise::command(
    slash_command,
    prefix_command,
    aliases("skip", "s", "next"),
    guild_only
)]
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

        if let Some(_) = handler.queue().current() {
            // Skip the current song
            let _ = handler.queue().skip();
            ctx.say("Skipped the current track.").await?;
        } else {
            ctx.say("No track in the queue.").await?;
        }
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
    if let Some(manager) = songbird::get(&ctx.serenity_context()).await {
        // Leave the voice channel
        let _ = manager.remove(guild_id).await;
        println!("Left the voice channel. Guild ID: {}", guild_id);
    } else {
        ctx.say("Not connected to a voice channel.").await?;
    }

    Ok(())
}

#[poise::command(
    slash_command,
    prefix_command,
    aliases("queue", "q", "list", "l"),
    guild_only
)]
pub async fn queue(ctx: Context<'_>) -> Result<(), Error> {
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

        let queue = handler.queue().current_queue();

        if queue.is_empty() {
            ctx.say("The queue is currently empty.").await?;
            return Ok(());
        }

        let mut message = String::new();

        // Current track
        if let Some(current) = queue.first() {
            let track_data = current.data::<crate::track::Track>();
            message.push_str(&format!(
                "**Now Playing:**\n> {}\n\n",
                get_formatted_track(&track_data)
            ));
        }

        // Next {tracks_to_show} tracks
        let tracks_to_show = 10;

        if queue.len() > 1 {
            message.push_str("**Up Next:**\n");

            // Retrieve the next {tracks_to_show} number of tracks
            for (i, track_handle) in queue.iter().skip(1).take(tracks_to_show).enumerate() {
                let track_data = track_handle.data::<crate::track::Track>();
                message.push_str(&format!(
                    "{}. {}\n",
                    i + 1,
                    get_formatted_track(&track_data)
                ));
            }

            // Show queue length if greater than {tracks_to_show} + 1 (current song)
            if queue.len() > (tracks_to_show + 1) {
                message.push_str(&format!(
                    "\n*...and {} more tracks in the queue*",
                    queue.len() - (tracks_to_show + 1)
                ));
            }
        } else {
            message.push_str("_No more tracks in the queue._");
        }

        ctx.say(message).await?;
    } else {
        ctx.say("Not connected to a voice channel.").await?;
    }

    Ok(())
}
