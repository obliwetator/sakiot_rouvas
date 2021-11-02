use serenity::{
    client::Context,
    model::{
        channel::ReactionType,
        id::{EmojiId, GuildId},
        interactions::{
            application_command::{
                ApplicationCommandInteraction, ApplicationCommandInteractionDataOptionValue,
            },
            message_component::ButtonStyle,
            InteractionApplicationCommandCallbackDataFlags, InteractionResponseType,
        },
    },
};

use crate::{
    events::interactions::{get_songbird_manager, interactions::download_track_async},
    GuildTrackMap,
};

use std::{convert::TryInto, fmt::Write};

use super::{
    helpers::{
        add_events_to_handle, get_guild_channel_id_from_interaction_application,
        join_or_get_voice_channel, misc_handle, not_in_a_voice_channel_application,
    },
    lavalink::get_lavalink_client,
};

pub async fn handle_j(ctx: &Context, command: &ApplicationCommandInteraction) {
    // Immediatly respond to the interaction which we will edit later
    if send_defered_response(command, ctx).await {
        return;
    }

    let (guild_id, channel_id) =
        get_guild_channel_id_from_interaction_application(command, ctx).await;

    let connect_to = match not_in_a_voice_channel_application(channel_id, command, ctx).await {
        Some(value) => value,
        None => return,
    };

    let _ = join_or_get_voice_channel(ctx, guild_id, connect_to, command.channel_id).await;

    fun_name(command, ctx, guild_id).await;
}

async fn fun_name(command: &ApplicationCommandInteraction, ctx: &Context, guild_id: GuildId) {
    let option = get_option_at_index_application_command(command, 0).await;
    if let ApplicationCommandInteractionDataOptionValue::String(string_result) = option {
        // TDOD: proper link validation
        if string_result.contains("youtube.com") {
            if string_result.contains("playlist") {
                // handle_playlist(ctx, command)
            } else {
                handle_youtube_link(string_result, command, ctx, guild_id).await;
            }
        } else if string_result.contains("patrykstyla.com") {
            handle_patryk_application_command(command, ctx).await;
        } else {
            // process string
            handle_search_string_application_command(string_result, ctx, guild_id, command).await;
        }
    } else {
        // Not a string. This should not happen?
        match command
            .edit_original_interaction_response(ctx, |f| f.content("Provide a string"))
            .await
        {
            Ok(_) => {}
            Err(err) => {
                panic!("cannot send response err 2: {}", err)
            }
        };
    }
}

pub async fn display_current_queue(ctx: &Context, command: &ApplicationCommandInteraction) {
    let (guild_id, _channel_id) =
        get_guild_channel_id_from_interaction_application(command, ctx).await;
    let manager = get_songbird_manager(ctx).await;
    let lavalink = get_lavalink_client(ctx).await;

    match manager.get(guild_id) {
        Some(_handle_lock) => {
            if let Some(ok) = lavalink
                .nodes()
                .await
                .get(&command.guild_id.expect("expected guild_id").0)
            {
                let mut output = String::from("Currently queued tracks\n");
                for (i, ele) in ok.queue.iter().enumerate() {
                    let title = match &ele.track.info {
                        Some(ok) => &ok.title,
                        None => "Unkown title",
                    };

                    writeln!(&mut output, "{}) {}", i + 1, title).expect("cannot write to buffer");
                }

                match command
                    .create_interaction_response(ctx, |f| {
                        f.kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| message.content(output))
                    })
                    .await
                {
                    Ok(_) => {}
                    Err(err) => {
                        panic!("cannot send response err: {}", err)
                    }
                };
            } else {
                match command
                    .create_interaction_response(ctx, |f| {
                        f.kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message.content("No audio queued").flags(
                                    InteractionApplicationCommandCallbackDataFlags::EPHEMERAL,
                                )
                            })
                    })
                    .await
                {
                    Ok(_) => {}
                    Err(err) => {
                        panic!("cannot send response err: {}", err)
                    }
                };
            };
        }
        None => {
            match command
                .create_interaction_response(ctx, |f| {
                    f.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message
                                .content("bot is not present in a voice channel")
                                .flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
                        })
                })
                .await
            {
                Ok(_) => {}
                Err(err) => {
                    panic!("cannot send response err: {}", err)
                }
            };
        }
    };
}

pub async fn send_interaction_message_basic(
    command: &ApplicationCommandInteraction,
    ctx: &Context,
    content: &str,
) {
    match command
        .create_interaction_response(ctx, |f| {
            f.kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|message| message.content(content))
        })
        .await
    {
        Ok(_) => {}
        Err(err) => {
            panic!("cannot send response err: {}", err)
        }
    };
}

// This command will try to set both the current and global volume
pub async fn handle_vol(ctx: &Context, command: &ApplicationCommandInteraction) {
    let (guild_id, channel_id) =
        get_guild_channel_id_from_interaction_application(command, ctx).await;

    let _ = match not_in_a_voice_channel_application(channel_id, command, ctx).await {
        Some(value) => value,
        None => return,
    };

    match command.data.options.get(0) {
        Some(float) => {
            let option = float.resolved.as_ref().expect("cannot get value");
            // we have a value change the volume
            if let ApplicationCommandInteractionDataOptionValue::Integer(value) = option {
                if !(0..2).contains(value) {
                    send_interaction_message_basic(command, ctx, "vol must be > 0 and < 200").await;
                    return;
                }

                let manager = get_songbird_manager(ctx).await;
                let lavalink = get_lavalink_client(ctx).await;
                match manager.get(guild_id) {
                    Some(_) => {
                        match lavalink.volume(guild_id.0, *value as u16).await {
                            Ok(_) => {}
                            Err(err) => {
                                panic!("cannot set volume: {}", err);
                            }
                        };
                    }
                    None => {
                        send_interaction_message_basic(
                            command,
                            ctx,
                            "bot is not present in the channel",
                        )
                        .await;

                        return;
                    }
                };
                // Set the global volume as well
                {
                    let map_lock = {
                        let data_read = ctx.data.read().await;
                        let data = data_read.get::<GuildTrackMap>().expect("msg").clone();
                        data
                    };
                    let mut mutex_guard = map_lock.lock().await;
                    let guild_track = mutex_guard
                        .get_mut(&command.guild_id.unwrap().0)
                        .expect("expected initialized value");
                    guild_track.volume = *value as u16;
                }

                send_interaction_message_basic(
                    command,
                    ctx,
                    format!("Volume changed to :{}%", *value).as_str(),
                )
                .await;
            } else {
                // TODO: error message not a float. This should not happen?
                println!("not a float");
            }
        }
        None => {
            {
                let map_lock = {
                    let data_read = ctx.data.read().await;
                    let data = data_read.get::<GuildTrackMap>().expect("msg").clone();
                    data
                };
                let mutex_guard = map_lock.lock().await;
                let guild_track = mutex_guard
                    .get(&command.guild_id.unwrap().0)
                    .expect("expected initialized value");
                let vol = guild_track.volume;
                // no value send the qurrent volume
                send_interaction_message_basic(
                    command,
                    ctx,
                    format!("volume is set to: {}%", vol).as_str(),
                )
                .await;
            }
        }
    }
}

pub async fn handle_playlist(ctx: &Context, command: &ApplicationCommandInteraction) {
    // Immediatly respond to the interaction which we will edit later
    if send_defered_response(command, ctx).await {
        return;
    }

    let (guild_id, channel_id) =
        get_guild_channel_id_from_interaction_application(command, ctx).await;

    let connect_to = match not_in_a_voice_channel_application(channel_id, command, ctx).await {
        Some(value) => value,
        None => return,
    };

    let lava_client = get_lavalink_client(ctx).await;
    let _handle_lock =
        join_or_get_voice_channel(ctx, guild_id, connect_to, command.channel_id).await;

    let option = get_option_at_index_application_command(command, 0).await;
    if let ApplicationCommandInteractionDataOptionValue::String(option) = option {
        // TDOD: proper link validation
        if option.contains("youtube.com") {
            let query_information = lava_client
                .auto_search_tracks(option)
                .await
                .expect("cannot get query info");

            if query_information.tracks.is_empty() {
                // check_msg(
                // 	msg.channel_id
                // 		.say(&ctx, "Could not find any video of the search query.")
                // 		.await,
                // );
                // return Ok(());
                println!("empty track");
            }

            // Queue all tracks
            for ele in &query_information.tracks {
                if let Err(why) = &lava_client
                    .play(guild_id.0, ele.clone())
                    // Change this to play() if you want your own custom queue or no queue at all.
                    .queue()
                    .await
                {
                    eprintln!("{}", why);
                };
            }

            println!("added to queue len: {}", query_information.tracks.len());
            play_audio_from_string(
                command,
                ctx,
                format!(
                    "added {} songs to the queue",
                    query_information.tracks.len()
                )
                .as_str(),
            )
            .await;
        } else {
            println!("error in play playlist");
        }
    };
}

pub async fn handle_join(ctx: &Context, command: &ApplicationCommandInteraction) {
    let manager = get_songbird_manager(ctx).await;

    let (guild_id, channel_id) =
        get_guild_channel_id_from_interaction_application(command, ctx).await;

    let connect_to = match channel_id {
        Some(channel) => channel,
        None => {
            send_interaction_message_basic(command, ctx, "Not in a voice channel").await;
            return;
        }
    };

    let (handle_lock, handler) = manager.join_gateway(guild_id, connect_to).await;
    match handler {
        Ok(connection_info) => {
            misc_handle(ctx, connection_info, guild_id).await;
            add_events_to_handle(&handle_lock, ctx, command.channel_id, guild_id).await;
            send_interaction_message_basic(command, ctx, "joined").await;
        }
        Err(_why) => {
            panic!("cannot join channel");
        }
    }
}

pub async fn send_defered_response(command: &ApplicationCommandInteraction, ctx: &Context) -> bool {
    match command
        .create_interaction_response(&ctx.http, |response| {
            response.kind(InteractionResponseType::DeferredChannelMessageWithSource)
        })
        .await
    {
        Ok(_) => {}
        Err(err) => {
            println!("Cannot respond to slash command 2: {}", err);
            return true;
        }
    }
    false
}

pub async fn handle_youtube_link(
    option: &str,
    command: &ApplicationCommandInteraction,
    ctx: &Context,
    guild_id: GuildId,
) {
    let lavalink = get_lavalink_client(ctx).await;
    let tracks = lavalink
        .auto_search_tracks(option)
        .await
        .expect("cannot find track");

    if tracks.tracks.is_empty() {
        edit_original_response_simple_content(command, ctx, "Search returned no results").await;
        return;
    }

    // We should only need to play 1 track. MAYBE: IF result is ambiguous let user choose(?)

    match lavalink
        .play(guild_id.0, tracks.tracks[0].clone())
        .queue()
        .await
    {
        Ok(_) => {}
        Err(err) => {
            // TODO: send response
            panic!("cannot play track: {}", err);
        }
    };

    download_track_async(ctx, option, guild_id).await;
    play_audio_from_string(
        command,
        ctx,
        tracks.tracks[0]
            .info
            .as_ref()
            .expect("cannot get info from track")
            .title
            .as_str(),
    )
    .await;
}

async fn edit_original_response_simple_content(
    command: &ApplicationCommandInteraction,
    ctx: &Context,
    content: &str,
) {
    match command
        .edit_original_interaction_response(ctx, |response| response.content(content))
        .await
    {
        Ok(_) => {}
        Err(err) => {
            println!("Cannot respond to application command {}", err)
        }
    };
}

pub async fn play_audio_from_string(
    command: &ApplicationCommandInteraction,
    ctx: &Context,
    title: &str,
) {
    let a = command
        .edit_original_interaction_response(&ctx.http, |response| {
            response
                .components(|comp| {
                    comp.create_action_row(|row| {
                        row.create_button(|btn| {
                            btn.custom_id("play")
                                .emoji(ReactionType::Unicode("⏯️".to_string()))
                                .style(ButtonStyle::Secondary)
                        })
                        .create_button(|btn| {
                            btn.custom_id("next")
                                .emoji(ReactionType::Custom {
                                    animated: false,
                                    id: EmojiId(365591266269855746),
                                    name: Some("residentsleeper".to_string()),
                                })
                                .style(ButtonStyle::Secondary)
                        })
                        .create_button(|btn| {
                            btn.custom_id("stop")
                                .emoji(ReactionType::Unicode("⏹️".to_string()))
                                .style(ButtonStyle::Secondary)
                        })
                        .create_button(|btn| {
                            btn.custom_id("ff")
                                .emoji(ReactionType::Unicode("⏭️".to_string()))
                                .style(ButtonStyle::Secondary)
                        })
                        .create_button(|btn| {
                            btn.custom_id("jam_it")
                                .emoji(ReactionType::Custom {
                                    animated: false,
                                    id: EmojiId(882285453661835364),
                                    name: Some("reggie".to_string()),
                                })
                                .label("")
                                .style(ButtonStyle::Secondary)
                        })
                    })
                })
                .content(format!("Playing a jammer: {}", title))
        })
        .await;

    match a {
        Ok(_) => {}
        Err(err) => {
            println!("Cannot respond to slash command 2: {}", err)
        }
    }
}

pub async fn get_option_at_index_application_command(
    command: &ApplicationCommandInteraction,
    index: usize,
) -> &ApplicationCommandInteractionDataOptionValue {
    let option = command
        .data
        .options
        .get(index)
        .expect("Expected required field")
        .resolved
        .as_ref()
        .expect("Expected required value");
    option
}

pub async fn handle_patryk_application_command(
    command: &ApplicationCommandInteraction,
    ctx: &Context,
) {
    match command
        .edit_original_interaction_response(ctx, |f| f.content("Not working yet"))
        .await
    {
        Ok(_) => {}
        Err(err) => {
            panic!("cannot send response err 2: {}", err)
        }
    };
}

pub async fn handle_search_string_application_command(
    option: &str,
    ctx: &Context,
    guild_id: GuildId,
    command: &ApplicationCommandInteraction,
) {
    // We search youtube for the string
    let lavalink = get_lavalink_client(ctx).await;
    let tracks = lavalink
        .search_tracks(option)
        .await
        .expect("cannot find track");

    if tracks.tracks.is_empty() {
        edit_original_response_simple_content(command, ctx, "Search returned no results").await;
        return;
    }

    match lavalink
        .play(guild_id.0, tracks.tracks[0].clone())
        .queue()
        .await
    {
        Ok(_) => {}
        Err(err) => {
            // TODO: send response
            panic!("cannot play track: {}", err);
        }
    };

    download_track_async(ctx, option, guild_id).await;
    play_audio_from_string(
        command,
        ctx,
        tracks.tracks[0]
            .info
            .as_ref()
            .expect("cannot get info from track")
            .title
            .as_str(),
    )
    .await;
}

pub async fn hanle_fast_forward_audio_application_command(
    ctx: &Context,
    command: &ApplicationCommandInteraction,
    length: i64,
) {
    let (guild_id, _channel_id) =
        get_guild_channel_id_from_interaction_application(command, ctx).await;
    let manager = get_songbird_manager(ctx).await;

    match manager.get(guild_id) {
        Some(handle_lock) => {
            // handle_handle(handle_lock, command, ctx).await;
            let handle = handle_lock.lock().await;
            let queue = handle.queue();
            // TODO:
            if queue.is_empty() {
                // queue empty send appropriate message

                match command
                    .create_interaction_response(ctx, |f| {
                        f.kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message.content("No audio queued").flags(
                                    InteractionApplicationCommandCallbackDataFlags::EPHEMERAL,
                                )
                            })
                    })
                    .await
                {
                    Ok(_) => {}
                    Err(err) => {
                        panic!("cannot send response err: {}", err)
                    }
                };

                return;
            }

            // get the track handle
            let current = queue.current().expect("cannot get current");
            // get the current track position in (secs)
            let mut current_time = current.get_info().await.expect("cannot get info").position;
            // Move x amount of secs to
            current_time += std::time::Duration::from_secs(length.try_into().unwrap());

            match current.seek_time(current_time) {
                Ok(_) => {}
                Err(err) => {
                    panic!("Cannot ff: {}", err);
                }
            };

            match command
                .create_interaction_response(ctx, |f| {
                    f.interaction_response_data(|data| {
                        data.content("")
                            .flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
                    })
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                })
                .await
            {
                Ok(_) => {}
                Err(err) => {
                    panic!("cannot send response err: {}", err)
                }
            };
        }
        None => {
            match command
                .create_interaction_response(ctx, |f| {
                    f.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message
                                .content("bot is not present in a voice channel")
                                .flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
                        })
                })
                .await
            {
                Ok(_) => {}
                Err(err) => {
                    panic!("cannot send response err: {}", err)
                }
            };
        }
    };
}
