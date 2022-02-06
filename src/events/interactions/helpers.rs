use std::{process::Command, sync::Arc};

use super::{get_songbird_manager, interactions::TrackEndNotifier};
use crate::{GuildTrack, GuildTrackMap, Lavalink};
use serenity::{
    client::Context,
    model::{
        id::ChannelId,
        interactions::{
            application_command::ApplicationCommandInteraction,
            message_component::MessageComponentInteraction,
        },
    },
    prelude::Mutex,
};
use songbird::error::JoinError;
use songbird::Call;
use songbird::{Event, TrackEvent};

pub async fn get_guild_channel_id_from_interaction_application(
    command: &ApplicationCommandInteraction,
    ctx: &Context,
) -> (serenity::model::id::GuildId, Option<ChannelId>) {
    // Get all neccessary info
    let guild_id = command.guild_id.expect("cannot get guild id from command");
    let guild = ctx
        .cache
        .guild(guild_id)
        .await
        .expect("cannot get guild from cache");
    let channel_id = guild
        .voice_states
        .get(&command.user.id)
        .and_then(|voice_state| voice_state.channel_id);
    (guild_id, channel_id)
}

pub async fn get_guild_channel_id_from_interaction_message(
    command: &MessageComponentInteraction,
    ctx: &Context,
) -> (serenity::model::id::GuildId, Option<ChannelId>) {
    // Get all neccessary info
    let guild_id = command.guild_id.expect("cannot get guild id from command");
    let guild = ctx
        .cache
        .guild(guild_id)
        .await
        .expect("cannot get guild from cache");
    let channel_id = guild
        .voice_states
        .get(&command.user.id)
        .and_then(|voice_state| voice_state.channel_id);
    (guild_id, channel_id)
}

pub async fn ytdl_input_from_string(option: &str) -> (String, String) {
    let stra = format!("ytsearch:{}", option);
    let command = Command::new("yt-dlp")
        .args([
            "-j",
            "--embed-metadata",
            "-f",
            "webm[abr>0]/bestaudio/best",
            "-R",
            "infinite",
            "--no-playlist",
            "--ignore-config",
            "--no-warnings",
            stra.as_str(),
            "-o",
            "/home/ubuntu/projects/sakiot_rouvas/t/%(title)s.%(ext)s",
        ])
        .output();

    let child = command.expect("cannot start yt-dlp");
    // Convert the output to a string
    let output = std::str::from_utf8(&child.stdout).expect("expected output in stdout");
    // Convert the string to a dynamic json (--print-json)
    let json: serde_json::Value = serde_json::from_str(output).expect("cannot serialize json");
    // Find the title in the json
    let title = json["title"].as_str().expect("title not found").to_owned();
    let ext = json["ext"].as_str().expect("ext not found").to_owned();

    (title, ext)
}

pub async fn not_in_a_voice_channel_application(
    channel_id: Option<ChannelId>,
    command: &ApplicationCommandInteraction,
    ctx: &Context,
) -> Option<ChannelId> {
    let connect_to = match channel_id {
        Some(channel) => channel,
        None => {
            let _ = command
                .edit_original_interaction_response(&ctx, |f| {
                    f.content("Not in a voice channel".to_string())
                })
                .await;
            return None;
        }
    };
    Some(connect_to)
}

pub async fn join_voice_channel(
    manager: Arc<songbird::Songbird>,
    ctx: &Context,
    guild_id: serenity::model::id::GuildId,
    connect_to: ChannelId,
    text_channel_id: ChannelId,
) -> Result<Arc<Mutex<Call>>, JoinError> {
    let (handle_lock, handler) = manager.join_gateway(guild_id, connect_to).await;

    match handler {
        Ok(connection_info) => {
            misc_handle(ctx, connection_info, guild_id).await;
            add_events_to_handle(&handle_lock, ctx, text_channel_id, guild_id).await;

            Ok(handle_lock)
        }
        Err(why) => Err(why),
    }
}

pub async fn join_or_get_voice_channel(
    ctx: &Context,
    guild_id: serenity::model::id::GuildId,
    connect_to: ChannelId,
    text_channel_id: ChannelId,
) -> Arc<Mutex<Call>> {
    let manager = get_songbird_manager(ctx).await;

    match manager.get(guild_id) {
        Some(handle_lock) => {
            // we have a handle. Re-use it
            // TODO: re-connect to a different channel if user is in a different channel
            handle_lock
        }
        None => {
            // No handle get a new one
            let (handle_lock, handler) = manager.join_gateway(guild_id, connect_to).await;
            match handler {
                Ok(connection_info) => {
                    misc_handle(ctx, connection_info, guild_id).await;
                    add_events_to_handle(&handle_lock, ctx, text_channel_id, guild_id).await;

                    handle_lock
                }
                Err(_why) => {
                    // TODO: send response
                    panic!("cannot join voice channel");
                }
            }
        }
    }
}

pub async fn misc_handle(
    ctx: &Context,
    connection_info: songbird::ConnectionInfo,
    guild_id: serenity::model::id::GuildId,
) {
    let data = ctx.data.read().await;
    let lavalink = data.get::<Lavalink>().unwrap().clone();
    lavalink
        .create_session_with_songbird(&connection_info)
        .await
        .expect("cannot create lavalink session");
    let _ = lavalink.volume(guild_id.0, 100).await;
    let guild_track = data
        .get::<GuildTrackMap>()
        .expect("cannot get GuildTrackMap")
        .clone();
    let mut mutex_guard = guild_track.lock().await;
    mutex_guard.insert(
        guild_id.0,
        GuildTrack {
            volume: 100,
            position: 0,
            how_long: std::time::Instant::now(),
        },
    );
}

pub async fn add_events_to_handle(
    handle_lock: &Arc<Mutex<Call>>,
    ctx: &Context,
    text_channel_id: ChannelId,
    _guild_id: serenity::model::id::GuildId,
) {
    // add events
    let mut handle = handle_lock.lock().await;
    let send_http = ctx.http.clone();

    handle.add_global_event(
        Event::Track(TrackEvent::End),
        TrackEndNotifier {
            chann_id: text_channel_id,
            http: send_http,
        },
    );

    // handle.add_global_event(
    //     Event::Track(TrackEvent::Play),
    //     TrackPlayNotifier {
    //         ctx: ctx.clone(),
    //         guild_id,
    //     },
    // );
}
