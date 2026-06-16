use songbird::tracks::PlayMode;
use std::collections::HashMap;
use std::time::Duration;

pub struct Data {
    pub session: tokio::sync::Mutex<crate::session::Session>,
    pub spool_read_ahead_bytes: u64,
    pub collection_track_fetch_concurrency: usize,
    pub command_prefix: String,
    pub repeat_modes:
        std::sync::Arc<tokio::sync::Mutex<HashMap<serenity::model::id::GuildId, RepeatMode>>>,
}
pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

const QUEUE_PAGE_SIZE: usize = 10;
const TRACK_INFO_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Eq, PartialEq, poise::ChoiceParameter)]
pub enum RepeatMode {
    #[name = "off"]
    Off,
    #[name = "track"]
    Track,
    #[name = "queue"]
    Queue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, poise::ChoiceParameter)]
pub enum RepeatCommandMode {
    #[name = "off"]
    Off,
    #[name = "track"]
    Track,
    #[name = "all"]
    All,
}

struct TrackErrorNotifier;

#[serenity::async_trait]
impl songbird::events::EventHandler for TrackErrorNotifier {
    async fn act(
        &self,
        ctx: &songbird::events::EventContext<'_>,
    ) -> Option<songbird::events::Event> {
        if let songbird::events::EventContext::Track(track_list) = ctx {
            for (state, handle) in *track_list {
                tracing::error!(
                    track_id = ?handle.uuid(),
                    state = ?state.playing,
                    "Track encountered an error"
                );
            }
        }

        None
    }
}

struct RepeatModeNotifier {
    handler_lock: std::sync::Arc<tokio::sync::Mutex<songbird::Call>>,
    repeat_modes:
        std::sync::Arc<tokio::sync::Mutex<HashMap<serenity::model::id::GuildId, RepeatMode>>>,
    guild_id: serenity::model::id::GuildId,
    spool_read_ahead_bytes: u64,
}

#[serenity::async_trait]
impl songbird::events::EventHandler for RepeatModeNotifier {
    async fn act(
        &self,
        ctx: &songbird::events::EventContext<'_>,
    ) -> Option<songbird::events::Event> {
        let repeat_mode = {
            let repeat_modes = self.repeat_modes.lock().await;
            repeat_modes
                .get(&self.guild_id)
                .copied()
                .unwrap_or(RepeatMode::Off)
        };
        if repeat_mode == RepeatMode::Off {
            return None;
        }

        if let songbird::events::EventContext::Track(track_list) = ctx {
            for (state, handle) in *track_list {
                match (repeat_mode, &state.playing) {
                    (RepeatMode::Track, PlayMode::Play) => {
                        if let Err(error) = handle.enable_loop() {
                            tracing::warn!(%error, "Failed to enable track repeat");
                        }
                    }
                    (RepeatMode::Queue, PlayMode::End) => {
                        let track = handle.data::<crate::track::Track>();
                        let mut handler = self.handler_lock.lock().await;
                        if let Err(error) = enqueue_track_with_spool(
                            &mut handler,
                            &track,
                            Duration::ZERO,
                            self.spool_read_ahead_bytes,
                        )
                        .await
                        {
                            tracing::warn!(%error, "Failed to requeue track for queue repeat");
                        }
                    }
                    _ => {}
                }
            }
        }

        None
    }
}

fn get_formatted_track(track: &crate::track::Track) -> String {
    let duration = format_duration_seconds(track.duration as u64);
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

fn help_message(prefix: &str) -> String {
    format!(
        concat!(
            "**Available Commands**\n",
            "`/help` or `{0}help` (`{0}commands`, `{0}cmds`) - Show this help message.\n",
            "`/ping` or `{0}ping` - Check whether the bot is responding.\n",
            "`/join` or `{0}join` (`{0}j`, `{0}summon`, `{0}connect`) - Join your current voice channel.\n",
            "`/volume [0-200]` or `{0}volume [0-200]` (`{0}vol`) - Show or set the playback volume.\n",
            "`/play <query-or-url>` or `{0}play <query-or-url>` (`{0}p`) - Queue a song, album, playlist, Tidal URL, or supported YouTube URL.\n",
            "`/pause` or `{0}pause` (`{0}wait`) - Pause the current track.\n",
            "`/resume` or `{0}resume` (`{0}unpause`, `{0}continue`) - Resume playback.\n",
            "`/seek <position>` or `{0}seek <position>` (`{0}seekto`, `{0}jump`, `{0}jumpto`, `{0}goto`) - Seek the current track to `seconds`, `mm:ss`, or `hh:mm:ss`.\n",
            "`/skip` or `{0}skip` (`{0}s`, `{0}next`) - Skip the current track.\n",
            "`/playnext <query-or-url>` or `{0}playnext <query-or-url>` (`{0}pn`) - Insert a song, album, playlist, Tidal URL, or supported YouTube URL right after the current track.\n",
            "`/repeat [track|all|off]` or `{0}repeat [track|all|off]` (`{0}loop`) - Repeat the current track, all tracks, or turn repeat off.\n",
            "`/shuffle` or `{0}shuffle` - Shuffle the queued tracks.\n",
            "`/remove <position>` or `{0}remove <position>` (`{0}delete <position>`) - Remove a queued track by its position in `queue`. Position `1` is the next track.\n",
            "`/clear` or `{0}clear` - Clear queued tracks without stopping the current track.\n",
            "`/stop` or `{0}stop` - Stop playback and clear the queue.\n",
            "`/current` or `{0}current` (`{0}currentplaying`, `{0}now`, `{0}nowplaying`, `{0}playing`, `{0}np`) - Show the current track.\n",
            "`/leave` or `{0}leave` (`{0}disconnect`) - Disconnect from voice.\n",
            "`/queue` or `{0}queue` (`{0}q`, `{0}list`, `{0}l`) - Show the current queue.\n",
            "`{0}queue page <n>` - Show a later queue page."
        ),
        prefix
    )
}

fn format_duration_seconds(total_seconds: u64) -> String {
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}", minutes, seconds)
    }
}

fn repeat_mode_name(mode: RepeatMode) -> &'static str {
    match mode {
        RepeatMode::Off => "off",
        RepeatMode::Track => "track",
        RepeatMode::Queue => "all",
    }
}

async fn current_repeat_mode(ctx: Context<'_>) -> Result<Option<RepeatMode>, Error> {
    let Some(guild_id) = guild_id(ctx).await? else {
        return Ok(None);
    };

    let repeat_mode = ctx
        .data()
        .repeat_modes
        .lock()
        .await
        .get(&guild_id)
        .copied()
        .unwrap_or(RepeatMode::Off);

    Ok(Some(repeat_mode))
}

fn parse_seek_position(position: &str) -> Result<Duration, String> {
    let position = position.trim();
    if position.is_empty() {
        return Err("Provide a seek position like `90`, `1:30`, or `1:02:03`.".to_string());
    }

    if position.starts_with('-') {
        return Err("Seek position cannot be negative.".to_string());
    }

    let parts = position.split(':').map(str::trim).collect::<Vec<&str>>();
    if parts.len() > 3 || parts.iter().any(|part| part.is_empty()) {
        return Err("Use `seconds`, `mm:ss`, or `hh:mm:ss`.".to_string());
    }

    let values = parts
        .iter()
        .map(|part| {
            part.parse::<u64>()
                .map_err(|_| "Use `seconds`, `mm:ss`, or `hh:mm:ss`.".to_string())
        })
        .collect::<Result<Vec<u64>, String>>()?;

    let seconds = match values.as_slice() {
        [seconds] => *seconds,
        [minutes, seconds] => {
            if *seconds >= 60 {
                return Err("Seconds must be less than 60 when using `mm:ss`.".to_string());
            }

            minutes
                .checked_mul(60)
                .and_then(|base| base.checked_add(*seconds))
                .ok_or_else(|| "Seek position is too large.".to_string())?
        }
        [hours, minutes, seconds] => {
            if *minutes >= 60 || *seconds >= 60 {
                return Err(
                    "Minutes and seconds must be less than 60 when using `hh:mm:ss`.".to_string(),
                );
            }

            hours
                .checked_mul(3600)
                .and_then(|base| {
                    minutes
                        .checked_mul(60)
                        .and_then(|mins| base.checked_add(mins))
                })
                .and_then(|base| base.checked_add(*seconds))
                .ok_or_else(|| "Seek position is too large.".to_string())?
        }
        _ => unreachable!("empty seek positions are rejected before parsing"),
    };

    Ok(Duration::from_secs(seconds))
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

    #[test]
    fn maps_remove_position_to_queue_index() {
        assert_eq!(queue_remove_index(4, 1), Ok(1));
        assert_eq!(queue_remove_index(4, 3), Ok(3));
    }

    #[test]
    fn rejects_remove_position_when_queue_has_no_up_next_tracks() {
        assert_eq!(
            queue_remove_index(1, 1),
            Err("There are no queued tracks to remove.")
        );
    }

    #[test]
    fn rejects_remove_position_zero_or_out_of_range() {
        assert_eq!(
            queue_remove_index(3, 0),
            Err("Position must be at least 1.")
        );
        assert_eq!(
            queue_remove_index(3, 3),
            Err("That position is outside the current queue.")
        );
    }

    #[test]
    fn shuffle_keeps_current_track_at_front() {
        let mut values = vec![1, 2, 3, 4, 5];
        let mut rng = fastrand::Rng::with_seed(7);

        shuffle_up_next(&mut values, &mut rng);

        assert_eq!(values[0], 1);
    }

    #[test]
    fn shuffle_preserves_all_up_next_tracks() {
        let mut values = vec![1, 2, 3, 4, 5];
        let mut rng = fastrand::Rng::with_seed(7);

        shuffle_up_next(&mut values, &mut rng);

        let mut up_next = values[1..].to_vec();
        up_next.sort_unstable();
        assert_eq!(up_next, vec![2, 3, 4, 5]);
    }

    #[test]
    fn shuffle_availability_requires_two_up_next_tracks() {
        assert!(!can_shuffle_queue(0));
        assert!(!can_shuffle_queue(1));
        assert!(!can_shuffle_queue(2));
        assert!(can_shuffle_queue(3));
    }

    #[test]
    fn move_appended_tracks_next_places_new_tracks_after_current() {
        let mut values = vec![1, 2, 3, 4, 5, 6];

        move_appended_tracks_next(&mut values, 2);

        assert_eq!(values, vec![1, 5, 6, 2, 3, 4]);
    }

    #[test]
    fn move_appended_tracks_next_preserves_order_when_only_new_tracks_follow_current() {
        let mut values = vec![1, 2, 3];

        move_appended_tracks_next(&mut values, 2);

        assert_eq!(values, vec![1, 2, 3]);
    }

    #[test]
    fn parses_queue_page_requests() {
        assert_eq!(parse_queue_page_request(None), Ok(1));
        assert_eq!(parse_queue_page_request(Some("2")), Ok(2));
        assert_eq!(parse_queue_page_request(Some("page 3")), Ok(3));
    }

    #[test]
    fn rejects_invalid_queue_page_requests() {
        assert_eq!(
            parse_queue_page_request(Some("page 0")),
            Err("Page number must be at least 1.".to_string())
        );
        assert_eq!(
            parse_queue_page_request(Some("later please")),
            Err("Use `queue`, `queue <page>`, or `queue page <page>`.".to_string())
        );
    }

    #[test]
    fn parses_seek_positions() {
        assert_eq!(parse_seek_position("90"), Ok(Duration::from_secs(90)));
        assert_eq!(parse_seek_position("1:30"), Ok(Duration::from_secs(90)));
        assert_eq!(
            parse_seek_position("1:02:03"),
            Ok(Duration::from_secs(3723))
        );
    }

    #[test]
    fn rejects_invalid_seek_positions() {
        assert_eq!(
            parse_seek_position(""),
            Err("Provide a seek position like `90`, `1:30`, or `1:02:03`.".to_string())
        );
        assert_eq!(
            parse_seek_position("-1"),
            Err("Seek position cannot be negative.".to_string())
        );
        assert_eq!(
            parse_seek_position("1:60"),
            Err("Seconds must be less than 60 when using `mm:ss`.".to_string())
        );
        assert_eq!(
            parse_seek_position("1:60:00"),
            Err("Minutes and seconds must be less than 60 when using `hh:mm:ss`.".to_string())
        );
        assert_eq!(
            parse_seek_position("later"),
            Err("Use `seconds`, `mm:ss`, or `hh:mm:ss`.".to_string())
        );
    }

    #[test]
    fn computes_queue_page_bounds() {
        assert_eq!(queue_page_bounds(23, 1), Ok((0, 10, 3)));
        assert_eq!(queue_page_bounds(23, 3), Ok((20, 23, 3)));
    }

    #[test]
    fn rejects_queue_page_out_of_range() {
        assert_eq!(
            queue_page_bounds(5, 2),
            Err("That page is outside the queue. There is only 1 page available.".to_string())
        );
    }

    #[test]
    fn formats_empty_queue_with_repeat_mode() {
        assert_eq!(
            format_queue_message(&[], 1, RepeatMode::Queue),
            Ok("**Repeat:** all\n\nThe queue is currently empty.".to_string())
        );
    }
}

/// Check whether the bot is responding.
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("Pong!").await?;
    Ok(())
}

/// Show the list of available commands and how to use them.
#[poise::command(slash_command, prefix_command, aliases("commands", "cmds"), guild_only)]
pub async fn help(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say(help_message(&ctx.data().command_prefix)).await?;
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

enum JoinVoiceChannelState {
    Joined,
    AlreadyConnected,
}

async fn try_join_voice_channel(ctx: Context<'_>) -> Result<Option<JoinVoiceChannelState>, Error> {
    let Some((guild_id, manager)) = guild_voice_manager(ctx).await? else {
        return Ok(None);
    };

    // Get the voice states from the guild
    let voice_states = if let Some(guild) = ctx.guild() {
        guild.voice_states.clone()
    } else {
        ctx.say("Voice states not available.").await?;
        return Ok(None);
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
            return Ok(None);
        }
    };

    // Check if the bot is already in the same channel
    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;
        if handler.current_channel() == Some(channel_id.into()) {
            return Ok(Some(JoinVoiceChannelState::AlreadyConnected));
        }
    }

    // Join (or move to) the voice channel
    if let Ok(handler_lock) = manager.join(guild_id, channel_id).await {
        let repeat_handler_lock = handler_lock.clone();
        let mut handler = handler_lock.lock().await;
        handler.add_global_event(
            songbird::events::TrackEvent::Error.into(),
            TrackErrorNotifier,
        );
        handler.add_global_event(
            songbird::events::TrackEvent::End.into(),
            RepeatModeNotifier {
                handler_lock: repeat_handler_lock.clone(),
                repeat_modes: ctx.data().repeat_modes.clone(),
                guild_id,
                spool_read_ahead_bytes: ctx.data().spool_read_ahead_bytes,
            },
        );
        handler.add_global_event(
            songbird::events::TrackEvent::Play.into(),
            RepeatModeNotifier {
                handler_lock: repeat_handler_lock,
                repeat_modes: ctx.data().repeat_modes.clone(),
                guild_id,
                spool_read_ahead_bytes: ctx.data().spool_read_ahead_bytes,
            },
        );
        Ok(Some(JoinVoiceChannelState::Joined))
    } else {
        ctx.say("Failed to join the voice channel.").await?;
        Ok(None)
    }
}

async fn pause_playback_message(handler: &songbird::Call) -> Result<&'static str, Error> {
    let Some(track_handle) = handler.queue().current() else {
        return Ok("No track is currently playing.");
    };

    let track_info = track_handle.get_info().await?;

    Ok(match track_info.playing {
        PlayMode::Play => {
            let _ = handler.queue().pause();
            "Paused the playback."
        }
        PlayMode::Pause => "Playback is already paused.",
        _ => "No track is currently playing.",
    })
}

async fn resume_playback_message(handler: &songbird::Call) -> Result<&'static str, Error> {
    let Some(track_handle) = handler.queue().current() else {
        return Ok("No track is currently playing.");
    };

    let track_info = track_handle.get_info().await?;

    Ok(match track_info.playing {
        PlayMode::Pause => {
            if handler.queue().resume().is_ok() {
                "Resumed the playback."
            } else {
                "Failed to resume the playback."
            }
        }
        PlayMode::Play => "Track already playing.",
        _ => "No track is currently playing.",
    })
}

fn queue_remove_index(queue_len: usize, position: usize) -> Result<usize, &'static str> {
    if position == 0 {
        return Err("Position must be at least 1.");
    }

    if queue_len <= 1 {
        return Err("There are no queued tracks to remove.");
    }

    if position >= queue_len {
        return Err("That position is outside the current queue.");
    }

    Ok(position)
}

fn can_shuffle_queue(queue_len: usize) -> bool {
    queue_len >= 3
}

fn shuffle_up_next<T>(queue: &mut [T], rng: &mut fastrand::Rng) {
    if queue.len() <= 2 {
        return;
    }

    let up_next = &mut queue[1..];

    for i in (1..up_next.len()).rev() {
        let j = rng.usize(..=i);
        up_next.swap(i, j);
    }
}

fn move_appended_tracks_next<T>(queue: &mut [T], inserted_count: usize) {
    if inserted_count == 0 || queue.len() <= 1 {
        return;
    }

    let up_next = &mut queue[1..];
    if inserted_count > up_next.len() {
        return;
    }

    up_next.rotate_right(inserted_count);
}

fn disable_track_loops(queue: &[songbird::tracks::TrackHandle]) {
    for track_handle in queue {
        let _ = track_handle.disable_loop();
    }
}

async fn set_repeat_mode(ctx: Context<'_>, mode: RepeatMode) -> Result<(), Error> {
    let Some(guild_id) = guild_id(ctx).await? else {
        return Ok(());
    };
    let Some(handler_lock) = voice_call(ctx, "Not connected to a voice channel.").await? else {
        return Ok(());
    };

    let queue = {
        let handler = handler_lock.lock().await;
        handler.queue().current_queue()
    };

    match mode {
        RepeatMode::Off => {
            disable_track_loops(&queue);
            ctx.data().repeat_modes.lock().await.remove(&guild_id);
            ctx.say("Repeat mode turned off.").await?;
        }
        RepeatMode::Track => {
            let Some(current) = queue.first() else {
                ctx.say("No track is currently playing.").await?;
                return Ok(());
            };

            disable_track_loops(&queue);
            current.enable_loop()?;
            ctx.data()
                .repeat_modes
                .lock()
                .await
                .insert(guild_id, RepeatMode::Track);
            ctx.say("Repeating the current track.").await?;
        }
        RepeatMode::Queue => {
            disable_track_loops(&queue);
            ctx.data()
                .repeat_modes
                .lock()
                .await
                .insert(guild_id, RepeatMode::Queue);
            ctx.say("Repeating the queue.").await?;
        }
    }

    Ok(())
}

fn parse_queue_page_request(page: Option<&str>) -> Result<usize, String> {
    let Some(page) = page.map(str::trim).filter(|page| !page.is_empty()) else {
        return Ok(1);
    };

    let raw_page = if let Some(page) = page.strip_prefix("page ") {
        page.trim()
    } else {
        page
    };

    let page_number = raw_page
        .parse::<usize>()
        .map_err(|_| "Use `queue`, `queue <page>`, or `queue page <page>`.".to_string())?;

    if page_number == 0 {
        return Err("Page number must be at least 1.".to_string());
    }

    Ok(page_number)
}

fn queue_page_bounds(up_next_count: usize, page: usize) -> Result<(usize, usize, usize), String> {
    let total_pages = up_next_count.div_ceil(QUEUE_PAGE_SIZE);
    if total_pages == 0 {
        return Ok((0, 0, 0));
    }

    if page > total_pages {
        return Err(format!(
            "That page is outside the queue. There {} only {} page{} available.",
            if total_pages == 1 { "is" } else { "are" },
            total_pages,
            if total_pages == 1 { "" } else { "s" }
        ));
    }

    let start = (page - 1) * QUEUE_PAGE_SIZE;
    let end = std::cmp::min(start + QUEUE_PAGE_SIZE, up_next_count);

    Ok((start, end, total_pages))
}

fn format_queue_message(
    queue: &[songbird::tracks::TrackHandle],
    page: usize,
    repeat_mode: RepeatMode,
) -> Result<String, String> {
    if queue.is_empty() {
        return Ok(format!(
            "**Repeat:** {}\n\nThe queue is currently empty.",
            repeat_mode_name(repeat_mode)
        ));
    }

    let mut message = format!("**Repeat:** {}\n\n", repeat_mode_name(repeat_mode));

    if let Some(current) = queue.first() {
        let track_data = current.data::<crate::track::Track>();
        message.push_str(&format!(
            "**Now Playing:**\n> {}\n\n",
            get_formatted_track(&track_data)
        ));
    }

    let up_next = &queue[1..];
    if up_next.is_empty() {
        message.push_str("_No more tracks in the queue._");
        return Ok(message);
    }

    let (start, end, total_pages) = queue_page_bounds(up_next.len(), page)?;
    message.push_str(&format!("**Up Next (Page {page}/{total_pages}):**\n"));

    for (offset, track_handle) in up_next.iter().skip(start).take(end - start).enumerate() {
        let track_data = track_handle.data::<crate::track::Track>();
        message.push_str(&format!(
            "{}. {}\n",
            start + offset + 1,
            get_formatted_track(&track_data)
        ));
    }

    if total_pages > 1 {
        message.push_str(&format!(
            "\n*Showing tracks {}-{} of {} queued tracks.*",
            start + 1,
            end,
            up_next.len()
        ));
    }

    Ok(message)
}

/// Join the voice channel you are currently in.
#[poise::command(
    slash_command,
    prefix_command,
    aliases("j", "summon", "connect"),
    guild_only
)]
pub async fn join(ctx: Context<'_>) -> Result<(), Error> {
    // Attempt to join the voice channel if not already connected
    if let Some(state) = try_join_voice_channel(ctx).await? {
        let message = match state {
            JoinVoiceChannelState::Joined => "Joined your voice channel.",
            JoinVoiceChannelState::AlreadyConnected => "Already connected to your voice channel.",
        };
        ctx.say(message).await?;
    }

    Ok(())
}

/// Show the current volume or set it between 0 and 200.
#[poise::command(slash_command, prefix_command, aliases("vol"), guild_only)]
pub async fn volume(
    ctx: Context<'_>,
    #[description = "Volume percentage from 0 to 200"] volume: Option<u8>,
) -> Result<(), Error> {
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
) -> Result<songbird::tracks::TrackHandle, Error> {
    enqueue_track_at(ctx, handler, track, Duration::ZERO).await
}

async fn enqueue_track_at(
    ctx: &Context<'_>,
    handler: &mut songbird::Call,
    track: &crate::track::Track,
    start_position: Duration,
) -> Result<songbird::tracks::TrackHandle, Error> {
    enqueue_track_with_spool(
        handler,
        track,
        start_position,
        ctx.data().spool_read_ahead_bytes,
    )
    .await
}

async fn enqueue_track_with_spool(
    handler: &mut songbird::Call,
    track: &crate::track::Track,
    start_position: Duration,
    spool_read_ahead_bytes: u64,
) -> Result<songbird::tracks::TrackHandle, Error> {
    let ffmpeg_stream = if start_position.is_zero() {
        crate::ffmpeg_spool::FfmpegStream::new(&track.stream_url, spool_read_ahead_bytes)
    } else {
        crate::ffmpeg_spool::FfmpegStream::new_at(
            &track.stream_url,
            spool_read_ahead_bytes,
            start_position,
        )
    };
    let stream = songbird::input::Input::Lazy(Box::new(ffmpeg_stream));
    let data = std::sync::Arc::new(track.clone());
    let songbird_track = songbird::tracks::Track::new_with_data(stream, data);

    let handle = handler.enqueue(songbird_track).await;

    Ok(handle)
}

async fn find_tracks_for_query(
    ctx: &Context<'_>,
    query: &str,
) -> Result<Vec<crate::track::Track>, Error> {
    let mut session = ctx.data().session.lock().await;

    let mut tracks = crate::url_handler::handle_url(
        &mut session,
        query,
        ctx.data().collection_track_fetch_concurrency,
    )
    .await?;

    if tracks.is_empty() {
        tracks = {
            let tracks = session
                .find_tracks(query, 1)
                .await
                .map_err(|e| Error::from(e.to_string()))?;

            if tracks.is_empty() {
                return Ok(Vec::new());
            }

            tracks
        };
    }

    Ok(tracks)
}

/// Queue a track from a search query or supported URL.
#[poise::command(slash_command, prefix_command, aliases("p"), guild_only)]
pub async fn play(
    ctx: Context<'_>,
    #[description = "Provide the query or url of a song"]
    #[rest]
    query_or_url: Option<String>,
) -> Result<(), Error> {
    // Attempt to join the voice channel if not already connected
    if try_join_voice_channel(ctx).await?.is_none() {
        return Ok(());
    }

    let Some((guild_id, manager)) = guild_voice_manager(ctx).await? else {
        return Ok(());
    };

    if query_or_url.is_none() {
        if let Some(handler_lock) = manager.get(guild_id) {
            let handler = handler_lock.lock().await;
            ctx.say(resume_playback_message(&handler).await?).await?;
        } else {
            // Probably shouldn't happen since the bot would join in try_join_voice_channel
            ctx.say("I'm not in a voice channel.").await?;
        }
        return Ok(());
    }

    let query = query_or_url.unwrap();
    tracing::info!(
        guild_id = %guild_id,
        user_id = %ctx.author().id,
        user = %ctx.author().name,
        query = %query,
        "User search"
    );

    let _ = ctx.defer().await;

    let tracks = find_tracks_for_query(&ctx, &query).await?;
    if tracks.is_empty() {
        ctx.say("No track was found on Tidal.").await?;
        return Ok(());
    }

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;

        for track in &tracks {
            let _ = enqueue_track(&ctx, &mut handler, track).await?;
        }

        if tracks.len() == 1 {
            tracing::info!(
                guild_id = %guild_id,
                user_id = %ctx.author().id,
                user = %ctx.author().name,
                artist = %tracks[0].artist,
                title = %tracks[0].title,
                duration_seconds = tracks[0].duration,
                "Queued track"
            );
            ctx.say(format!(
                "{} added **{}** to the queue.",
                ctx.author().name,
                get_formatted_track(&tracks[0])
            ))
            .await?;
        } else {
            tracing::info!(
                guild_id = %guild_id,
                user_id = %ctx.author().id,
                user = %ctx.author().name,
                track_count = tracks.len(),
                query = %query,
                "Queued multiple tracks"
            );
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

/// Insert a track from a search query or supported URL right after the current track.
#[poise::command(slash_command, prefix_command, aliases("pn"), guild_only)]
pub async fn playnext(
    ctx: Context<'_>,
    #[description = "Provide the query or url of a song"]
    #[rest]
    query_or_url: String,
) -> Result<(), Error> {
    if try_join_voice_channel(ctx).await?.is_none() {
        return Ok(());
    }

    let Some((guild_id, manager)) = guild_voice_manager(ctx).await? else {
        return Ok(());
    };

    let query = query_or_url;
    tracing::info!(
        guild_id = %guild_id,
        user_id = %ctx.author().id,
        user = %ctx.author().name,
        query = %query,
        "User playnext search"
    );

    let _ = ctx.defer().await;

    let tracks = find_tracks_for_query(&ctx, &query).await?;
    if tracks.is_empty() {
        ctx.say("No track was found on Tidal.").await?;
        return Ok(());
    }

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;
        let had_existing_queue = !handler.queue().is_empty();

        for track in &tracks {
            let _ = enqueue_track(&ctx, &mut handler, track).await?;
        }

        if had_existing_queue {
            handler.queue().modify_queue(|queue| {
                move_appended_tracks_next(queue.make_contiguous(), tracks.len());
            });
        }

        if had_existing_queue {
            if tracks.len() == 1 {
                tracing::info!(
                    guild_id = %guild_id,
                    user_id = %ctx.author().id,
                    user = %ctx.author().name,
                    artist = %tracks[0].artist,
                    title = %tracks[0].title,
                    duration_seconds = tracks[0].duration,
                    "Queued track to play next"
                );
                ctx.say(format!(
                    "{} added **{}** to play next.",
                    ctx.author().name,
                    get_formatted_track(&tracks[0])
                ))
                .await?;
            } else {
                tracing::info!(
                    guild_id = %guild_id,
                    user_id = %ctx.author().id,
                    user = %ctx.author().name,
                    track_count = tracks.len(),
                    query = %query,
                    "Queued multiple tracks to play next"
                );
                ctx.say(format!(
                    "{} added **{} tracks** to play next.",
                    ctx.author().name,
                    tracks.len()
                ))
                .await?;
            }
        } else if tracks.len() == 1 {
            ctx.say(format!(
                "{} started playing **{}**.",
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

/// Set repeat mode to the current track, all tracks, or off.
#[poise::command(slash_command, prefix_command, aliases("loop"), guild_only)]
pub async fn repeat(
    ctx: Context<'_>,
    #[description = "Repeat mode: track, all, or off"] mode: Option<RepeatCommandMode>,
) -> Result<(), Error> {
    let mode = match mode {
        Some(RepeatCommandMode::Off) => RepeatMode::Off,
        Some(RepeatCommandMode::Track) => RepeatMode::Track,
        Some(RepeatCommandMode::All) => RepeatMode::Queue,
        None => RepeatMode::Track,
    };

    set_repeat_mode(ctx, mode).await
}

/// Pause the current playback.
#[poise::command(slash_command, prefix_command, aliases("wait"), guild_only)]
pub async fn pause(ctx: Context<'_>) -> Result<(), Error> {
    let Some(handler_lock) = voice_call(ctx, "Not connected to a voice channel.").await? else {
        return Ok(());
    };
    let handler = handler_lock.lock().await;

    ctx.say(pause_playback_message(&handler).await?).await?;

    Ok(())
}

/// Resume playback if it is currently paused.
#[poise::command(
    slash_command,
    prefix_command,
    aliases("unpause", "continue"),
    guild_only
)]
pub async fn resume(ctx: Context<'_>) -> Result<(), Error> {
    let Some(handler_lock) = voice_call(ctx, "I'm not in a voice channel.").await? else {
        return Ok(());
    };
    let handler = handler_lock.lock().await;
    ctx.say(resume_playback_message(&handler).await?).await?;

    Ok(())
}

/// Seek the current track to a given position.
#[poise::command(
    slash_command,
    prefix_command,
    guild_only,
    aliases("seekto", "jump", "jumpto", "goto")
)]
pub async fn seek(
    ctx: Context<'_>,
    #[description = "Position as seconds, mm:ss, or hh:mm:ss"]
    #[rest]
    position: String,
) -> Result<(), Error> {
    let position = match parse_seek_position(&position) {
        Ok(position) => position,
        Err(message) => {
            ctx.say(message).await?;
            return Ok(());
        }
    };

    let Some(handler_lock) = voice_call(ctx, "Not connected to a voice channel.").await? else {
        return Ok(());
    };

    let queue = {
        let handler = handler_lock.lock().await;
        handler.queue().current_queue()
    };

    let Some(current_handle) = queue.first() else {
        ctx.say("No track is currently playing.").await?;
        return Ok(());
    };

    let current_uuid = current_handle.uuid();
    let track = current_handle.data::<crate::track::Track>().clone();
    if position.as_secs() > track.duration as u64 {
        ctx.say(format!(
            "Seek position is past the end of the current track ({}).",
            format_duration_seconds(track.duration as u64)
        ))
        .await?;
        return Ok(());
    }

    let track_info = match tokio::time::timeout(TRACK_INFO_TIMEOUT, current_handle.get_info()).await
    {
        Ok(Ok(track_info)) => Some(track_info),
        Ok(Err(error)) => {
            tracing::warn!(%error, "Failed to get current track info before seek");
            None
        }
        Err(_) => {
            tracing::warn!("Timed out getting current track info before seek");
            None
        }
    };
    let volume = track_info
        .as_ref()
        .map_or(1.0, |track_info| track_info.volume);
    let was_paused = track_info
        .as_ref()
        .is_some_and(|track_info| track_info.playing == PlayMode::Pause);
    let up_next = queue[1..]
        .iter()
        .map(|track_handle| track_handle.data::<crate::track::Track>().clone())
        .collect::<Vec<_>>();

    let new_handle = {
        let mut handler = handler_lock.lock().await;
        if handler
            .queue()
            .current()
            .is_none_or(|track_handle| track_handle.uuid() != current_uuid)
        {
            None
        } else {
            handler.queue().stop();
            let new_handle = enqueue_track_at(&ctx, &mut handler, &track, position).await?;
            for track in &up_next {
                let _ = enqueue_track(&ctx, &mut handler, track).await?;
            }

            Some(new_handle)
        }
    };

    let Some(new_handle) = new_handle else {
        ctx.say("The current track changed before seek could be applied.")
            .await?;
        return Ok(());
    };

    let _ = new_handle.set_volume(volume);
    if was_paused {
        let _ = new_handle.pause();
    }

    ctx.say(format!(
        "Seeked to {}.",
        format_duration_seconds(position.as_secs())
    ))
    .await?;

    Ok(())
}

/// Skip the current track.
#[poise::command(slash_command, prefix_command, aliases("s", "next"), guild_only)]
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

/// Shuffle the queued tracks.
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn shuffle(ctx: Context<'_>) -> Result<(), Error> {
    let Some(handler_lock) = voice_call(ctx, "Not connected to a voice channel.").await? else {
        return Ok(());
    };
    let handler = handler_lock.lock().await;

    let queue_len = handler.queue().len();
    if !can_shuffle_queue(queue_len) {
        ctx.say("Need at least 2 queued tracks to shuffle.").await?;
        return Ok(());
    }

    {
        let mut rng = fastrand::Rng::new();
        handler.queue().modify_queue(|queue| {
            shuffle_up_next(queue.make_contiguous(), &mut rng);
        });
    }

    ctx.say("Shuffled the queue.").await?;

    Ok(())
}

/// Remove a queued track by its position in the queue display.
#[poise::command(slash_command, prefix_command, aliases("delete"), guild_only)]
pub async fn remove(
    ctx: Context<'_>,
    #[description = "Queue position to remove, where 1 is the next track"] position: usize,
) -> Result<(), Error> {
    let Some(handler_lock) = voice_call(ctx, "Not connected to a voice channel.").await? else {
        return Ok(());
    };
    let handler = handler_lock.lock().await;

    let queue = handler.queue().current_queue();
    let queue_index = match queue_remove_index(queue.len(), position) {
        Ok(queue_index) => queue_index,
        Err(message) => {
            ctx.say(message).await?;
            return Ok(());
        }
    };

    let Some(removed_track) = handler.queue().dequeue(queue_index) else {
        ctx.say("Failed to remove that track from the queue.")
            .await?;
        return Ok(());
    };

    let removed_track_data = removed_track.data::<crate::track::Track>().clone();
    let _ = removed_track.stop();

    ctx.say(format!(
        "Removed **{}** from the queue.",
        get_formatted_track(&removed_track_data)
    ))
    .await?;

    Ok(())
}

/// Clear queued tracks without stopping the current track.
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn clear(ctx: Context<'_>) -> Result<(), Error> {
    let Some(handler_lock) = voice_call(ctx, "Not connected to a voice channel.").await? else {
        return Ok(());
    };
    let handler = handler_lock.lock().await;

    let queue_len = handler.queue().len();
    if queue_len <= 1 {
        ctx.say("There are no queued tracks to clear.").await?;
        return Ok(());
    }

    let mut removed_count = 0;
    for _ in 1..queue_len {
        if let Some(track) = handler.queue().dequeue(1) {
            let _ = track.stop();
            removed_count += 1;
        }
    }

    ctx.say(format!(
        "Cleared {} queued track{}.",
        removed_count,
        if removed_count == 1 { "" } else { "s" }
    ))
    .await?;

    Ok(())
}

/// Stop playback and clear the queue.
#[poise::command(slash_command, prefix_command, guild_only)]
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

/// Show the track that is currently playing.
#[poise::command(
    slash_command,
    prefix_command,
    aliases("currentplaying", "now", "nowplaying", "playing", "np"),
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

/// Disconnect the bot from the voice channel.
#[poise::command(slash_command, prefix_command, aliases("disconnect"), guild_only)]
pub async fn leave(ctx: Context<'_>) -> Result<(), Error> {
    let Some(guild_id) = guild_id(ctx).await? else {
        return Ok(());
    };

    // Get the songbird voice manager
    if let Some(manager) = songbird::get(ctx.serenity_context()).await {
        if manager.get(guild_id).is_none() {
            ctx.say("Not connected to a voice channel.").await?;
            return Ok(());
        }

        let _ = manager.remove(guild_id).await;
        tracing::info!(guild_id = %guild_id, "Left voice channel");
        ctx.say("Disconnected from the voice channel.").await?;
    } else {
        ctx.say("Not connected to a voice channel.").await?;
    }

    Ok(())
}

/// Show the current queue and what is up next.
#[poise::command(slash_command, prefix_command, aliases("q", "list", "l"), guild_only)]
pub async fn queue(
    ctx: Context<'_>,
    #[description = "Optional page number, or use `page 2` in prefix commands"]
    #[rest]
    page: Option<String>,
) -> Result<(), Error> {
    let Some(handler_lock) = voice_call(ctx, "Not connected to a voice channel.").await? else {
        return Ok(());
    };
    let handler = handler_lock.lock().await;

    let queue = handler.queue().current_queue();
    let repeat_mode = current_repeat_mode(ctx).await?.unwrap_or(RepeatMode::Off);
    let page = match parse_queue_page_request(page.as_deref()) {
        Ok(page) => page,
        Err(message) => {
            ctx.say(message).await?;
            return Ok(());
        }
    };

    match format_queue_message(&queue, page, repeat_mode) {
        Ok(message) => ctx.say(message).await?,
        Err(message) => ctx.say(message).await?,
    };

    Ok(())
}
