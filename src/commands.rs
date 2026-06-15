use songbird::tracks::PlayMode;

pub struct Data {
    pub session: tokio::sync::Mutex<crate::session::Session>,
    pub spool_read_ahead_bytes: u64,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn track(title: &str, featured_artists: Vec<&str>, duration: u32) -> crate::track::Track {
        crate::track::Track {
            title: title.to_string(),
            artist: "Main Artist".to_string(),
            featured_artists: featured_artists.into_iter().map(String::from).collect(),
            duration,
            stream_url: "https://example.com/stream".to_string(),
        }
    }

    #[test]
    fn formats_track_without_featured_artists() {
        assert_eq!(
            get_formatted_track(&track("Song Title", Vec::new(), 185)),
            "Main Artist - Song Title (03:05)"
        );
    }

    #[test]
    fn appends_featured_artists_when_title_does_not_include_them() {
        assert_eq!(
            get_formatted_track(&track("Song Title", vec!["Guest One", "Guest Two"], 65)),
            "Main Artist - Song Title ft. Guest One, Guest Two (01:05)"
        );
    }

    #[test]
    fn avoids_duplicate_featured_artists_when_title_already_mentions_them() {
        assert_eq!(
            get_formatted_track(&track(
                "Song Title feat. Guest One",
                vec!["Guest One"],
                3661
            )),
            "Main Artist - Song Title feat. Guest One (01:01:01)"
        );
    }
}

#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("Pong!").await?;
    Ok(())
}

async fn guild_id(ctx: Context<'_>) -> Result<Option<serenity::model::id::GuildId>, Error> {
    match ctx.guild_id() {
        Some(guild_id) => Ok(Some(guild_id)),
        None => {
            ctx.say("This command can only be used in a guild.").await?;
            Ok(None)
        }
    }
}

async fn voice_manager(ctx: Context<'_>) -> std::sync::Arc<songbird::Songbird> {
    songbird::get(ctx.serenity_context())
        .await
        .expect("Songbird voice manager not found")
}

async fn guild_voice_manager(
    ctx: Context<'_>,
) -> Result<
    Option<(
        serenity::model::id::GuildId,
        std::sync::Arc<songbird::Songbird>,
    )>,
    Error,
> {
    let Some(guild_id) = guild_id(ctx).await? else {
        return Ok(None);
    };

    Ok(Some((guild_id, voice_manager(ctx).await)))
}

async fn voice_call(
    ctx: Context<'_>,
    not_connected_message: &str,
) -> Result<Option<std::sync::Arc<tokio::sync::Mutex<songbird::Call>>>, Error> {
    let Some((guild_id, manager)) = guild_voice_manager(ctx).await? else {
        return Ok(None);
    };

    match manager.get(guild_id) {
        Some(handler_lock) => Ok(Some(handler_lock)),
        None => {
            ctx.say(not_connected_message).await?;
            Ok(None)
        }
    }
}

async fn try_join_voice_channel(ctx: Context<'_>) -> Result<bool, Error> {
    let Some((guild_id, manager)) = guild_voice_manager(ctx).await? else {
        return Ok(false);
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
pub async fn volume(ctx: Context<'_>, volume: Option<u8>) -> Result<(), Error> {
    if volume.is_some_and(|volume| volume > 200) {
        ctx.say("Volume must be between 0 and 200.").await?;
        return Ok(());
    }

    let Some(handler_lock) = voice_call(ctx, "Not connected to a voice channel.").await? else {
        return Ok(());
    };
    let handler = handler_lock.lock().await;

    // Set the volume
    let mut current = handler.queue().current();
    let Some(track_handle) = current.as_mut() else {
        ctx.say("No track is currently playing.").await?;
        return Ok(());
    };

    if let Some(volume) = volume {
        let _ = track_handle.set_volume(volume as f32 / 100.0);
        ctx.say(format!("Volume set to {}%.", volume)).await?;
    } else {
        let track_info = track_handle.get_info().await?;
        let volume = (track_info.volume * 100.0).round() as u32;
        ctx.say(format!("Current volume: {}%.", volume)).await?;
    }

    Ok(())
}

async fn enqueue_track(
    ctx: &Context<'_>,
    handler: &mut songbird::Call,
    track: &crate::track::Track,
) -> Result<(), Error> {
    let stream = songbird::input::Input::Lazy(Box::new(crate::ffmpeg_spool::FfmpegStream::new(
        &track.stream_url,
        ctx.data().spool_read_ahead_bytes,
    )));
    let data = std::sync::Arc::new(track.clone());
    let songbird_track = songbird::tracks::Track::new_with_data(stream, data);

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
    if !try_join_voice_channel(ctx).await? {
        return Ok(());
    }

    let Some((guild_id, manager)) = guild_voice_manager(ctx).await? else {
        return Ok(());
    };

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

    let mut session = ctx.data().session.lock().await;

    let mut tracks = crate::url_handler::handle_url(&mut session, &query).await?;

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
    let Some(handler_lock) = voice_call(ctx, "Not connected to a voice channel.").await? else {
        return Ok(());
    };
    let handler = handler_lock.lock().await;

    // Pause the playback
    let _ = handler.queue().pause();

    ctx.say("Paused the playback.").await?;

    Ok(())
}

#[poise::command(
    slash_command,
    prefix_command,
    aliases("resume", "unpause", "continue"),
    guild_only
)]
pub async fn resume(ctx: Context<'_>) -> Result<(), Error> {
    let Some(handler_lock) = voice_call(ctx, "I'm not in a voice channel.").await? else {
        return Ok(());
    };
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

    Ok(())
}

#[poise::command(
    slash_command,
    prefix_command,
    aliases("skip", "s", "next"),
    guild_only
)]
pub async fn skip(ctx: Context<'_>) -> Result<(), Error> {
    let Some(handler_lock) = voice_call(ctx, "Not connected to a voice channel.").await? else {
        return Ok(());
    };
    let handler = handler_lock.lock().await;

    if handler.queue().current().is_some() {
        // Skip the current song
        let _ = handler.queue().skip();
        ctx.say("Skipped the current track.").await?;
    } else {
        ctx.say("No track in the queue.").await?;
    }

    Ok(())
}

#[poise::command(slash_command, prefix_command, aliases("stop"), guild_only)]
pub async fn stop(ctx: Context<'_>) -> Result<(), Error> {
    let Some(handler_lock) = voice_call(ctx, "Not connected to a voice channel.").await? else {
        return Ok(());
    };
    let handler = handler_lock.lock().await;

    // Stop the playback and clear the queue
    handler.queue().stop();

    ctx.say("Stopped the playback and cleared the queue.")
        .await?;

    Ok(())
}

#[poise::command(
    slash_command,
    prefix_command,
    aliases("current", "currentplaying", "now", "nowplaying", "playing"),
    guild_only
)]
pub async fn current(ctx: Context<'_>) -> Result<(), Error> {
    let Some(handler_lock) = voice_call(ctx, "Not connected to a voice channel.").await? else {
        return Ok(());
    };
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

    Ok(())
}

#[poise::command(
    slash_command,
    prefix_command,
    aliases("leave", "disconnect"),
    guild_only
)]
pub async fn leave(ctx: Context<'_>) -> Result<(), Error> {
    let Some(guild_id) = guild_id(ctx).await? else {
        return Ok(());
    };

    // Get the songbird voice manager
    if let Some(manager) = songbird::get(ctx.serenity_context()).await {
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
    let Some(handler_lock) = voice_call(ctx, "Not connected to a voice channel.").await? else {
        return Ok(());
    };
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

    Ok(())
}
