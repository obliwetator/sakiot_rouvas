use std::sync::Arc;

use mysql_async::{prelude::Queryable, Row};
use serenity::{
    client::Context,
    model::{
        channel::ReactionType,
        id::{ChannelId, EmojiId},
        interactions::{
            message_component::{ButtonStyle, MessageComponentInteraction},
            InteractionApplicationCommandCallbackDataFlags, InteractionResponseType,
        },
    },
};
use songbird::input::Input;
use tracing::info;

use crate::{
    database::get_conn_from_ctx,
    events::interactions::{
        get_songbird_manager, helpers::get_guild_channel_id_from_interaction_message,
        interactions::ffmpeg_input_from_string,
    },
};

use super::{
    helpers::{add_events_to_handle, misc_handle},
    lavalink::get_lavalink_client,
};

struct JamIt {
    // guild_id: u64,
    audio_name: String,
    ext: String,
}

async fn play_audio_from_string_from_message_component(
    command: &MessageComponentInteraction,
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
                                .style(ButtonStyle::Secondary)
                        })
                    })
                    .create_action_row(|row| {
                        row.create_button(|btn| {
                            btn.custom_id("delete_and_skip")
                                .emoji(ReactionType::Unicode("❌".to_string()))
                                .style(ButtonStyle::Secondary)
                        })
                    })
                })
                .content(format!(
                    "Giga Jamming: {} - TODO: special buttons for this",
                    title
                ))
        })
        .await;

    match a {
        Ok(_) => {}
        Err(err) => {
            println!("Cannot respond to slash command 2: {}", err)
        }
    }
}

pub async fn handle_jam_it(ctx: &Context, command: &MessageComponentInteraction) {
    // Immediatly respond to the interaction which we will edit later
    if send_defered_response(command, ctx).await {
        return;
    }
    // TODO: local state after first query
    let (guild_id, channel_id) = get_guild_channel_id_from_interaction_message(command, ctx).await;
    let connect_to = match not_in_a_voice_channel_message(channel_id, command, ctx).await {
        Some(value) => value,
        None => return,
    };
    let mut conn = get_conn_from_ctx(ctx).await;
    let query = format!(
        "SELECT guild_id, audio_name, ext FROM jam_it WHERE guild_id = {} ORDER BY RAND() LIMIT 1",
        guild_id
    );
    let result: Vec<Row> = match conn.query(query).await {
        Ok(ok) => ok,
        Err(_) => {
            // TODO: handle error
            return;
        }
    };

    if result.is_empty() {
        // No result
        match command
            .edit_original_interaction_response(ctx, |f| f.content("No tracks present to jam"))
            .await
        {
            Ok(_) => {}
            Err(err) => {
                panic!("cannot send response err: {}", err)
            }
        };
    } else {
        let mut jam: Vec<JamIt> = Vec::new();

        for mut data in result {
            let j = JamIt {
                // guild_id: data.take("guild_id").expect("cannot get guild_id"),
                audio_name: data.take("audio_name").expect("cannot get audio name"),
                ext: data.take("ext").expect("cannot get ext"),
            };
            jam.push(j);
        }
        // TODO: Will not work if we plan on playing more than 1 audio
        let ffmpeg = ffmpeg_input_from_string(&jam[0].audio_name, &jam[0].ext).await;
        let manager = get_songbird_manager(ctx).await;

        let mut new_input = songbird::input::Input::from(ffmpeg);
        new_input.metadata.title = Some(jam[0].audio_name.to_owned());

        match manager.get(guild_id) {
            Some(handle_lock) => {
                let mut handle = handle_lock.lock().await;

                handle.enqueue_source(new_input);
                play_audio_from_string_from_message_component(command, ctx, &jam[0].audio_name)
                    .await;
            }
            None => {
                handle_no_handle_from_message_component(
                    manager, guild_id, connect_to, ctx, command, new_input,
                )
                .await;
                play_audio_from_string_from_message_component(command, ctx, &jam[0].audio_name)
                    .await;
            }
        };
    }
}

async fn not_in_a_voice_channel_message(
    channel_id: Option<ChannelId>,
    command: &MessageComponentInteraction,
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

pub async fn handle_delete_and_skip_from_jam(ctx: &Context, command: &MessageComponentInteraction) {
    // TODO: lock is aquired in each function
    handle_next_audio_in_queue(ctx, command).await;

    delete_from_jam(command, ctx).await;
}

async fn delete_from_jam(command: &MessageComponentInteraction, ctx: &Context) {
    let (guild_id, channel_id) = get_guild_channel_id_from_interaction_message(command, ctx).await;

    let _connect_to = match not_in_a_voice_channel_message(channel_id, command, ctx).await {
        Some(value) => value,
        None => return,
    };
    let manager = get_songbird_manager(ctx).await;

    match manager.get(guild_id) {
        Some(handle_lock) => {
            let handle = handle_lock.lock().await;
            let queue = handle.queue().clone();
            // Try to set the currently playing song volume
            // TODO: Proper error handling
            let current = queue.current().expect("expected a current track");
            let title = current
                .metadata()
                .clone()
                .title
                .expect("expected a title from metedata");

            let mut conn = get_conn_from_ctx(ctx).await;

            let query = format!(
                "DELETE FROM jam_it WHERE jam_it.audio_name = '{}' AND jam_it.guild_id = {}",
                title, guild_id.0
            );

            let _: Vec<usize> = match conn.query(query).await {
                Ok(ok) => ok,
                Err(err) => {
                    panic!("error when trying to delete: {}", err)
                }
            };
        }
        None => {
            send_interaction_message_basic_from_message(
                ctx,
                command,
                "bot is not present in the channel",
            )
            .await;
        }
    };
}

pub async fn handle_next_audio_in_queue(
    ctx: &Context,
    command: &serenity::model::interactions::message_component::MessageComponentInteraction,
) {
    let (guild_id, _channel_id) = get_guild_channel_id_from_interaction_message(command, ctx).await;
    let manager = get_songbird_manager(ctx).await;
    let lavalink = get_lavalink_client(ctx).await;

    match manager.get(guild_id) {
        Some(_handle_lock) => {
            // if let Some(node) = lavalink.nodes().await.get(&guild_id.0) {
            // if node.now_playing.is_some() {
            if let Some(_track) = lavalink.skip(guild_id.0).await {
                if let Some(node) = lavalink.nodes().await.get(&guild_id.0) {
                    if node.queue.is_empty() {
                        // IF we skip and the queue is empty that was the last song that did not "skip" properly
                        let _ = lavalink.stop(guild_id.0).await;
                        send_interaction_message_basic(command, ctx, "skipped 2").await;
                    } else {
                        send_interaction_message_basic(command, ctx, "skipped 1").await;
                    }
                }
            } else {
                if let Some(node) = lavalink.nodes().await.get(&guild_id.0) {}
                // nothing is playing atm OR only 1 track is playing

                // match lavalink.stop(guild_id.0).await {
                //     Ok(_) => {
                //         send_interaction_message_basic(command, ctx, "skipped 2").await;
                //     }
                //     Err(err) => {
                //         panic!("error when stopping: {}", err)
                //     }
                // }

                match command
                    .create_interaction_response(ctx, |f| {
                        f.kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message.content("No audio is playing")
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
            // }

            // }
        }

        None => {
            info!("No handle");
            // No handle bot is not in a channel is this guild. Create a new handle
            // handle_no_handle(manager, guild_id, connect_to, ctx, command).await;

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

async fn handle_no_handle_from_message_component(
    manager: Arc<songbird::Songbird>,
    guild_id: serenity::model::id::GuildId,
    connect_to: ChannelId,
    ctx: &Context,
    command: &MessageComponentInteraction,
    new_input: Input,
) {
    let (handle_lock, handler) = manager.join_gateway(guild_id, connect_to).await;
    match handler {
        Ok(connection_info) => {
            let mut handle = handle_lock.lock().await;

            misc_handle(ctx, connection_info, guild_id).await;
            add_events_to_handle(&handle_lock, ctx, command.channel_id, guild_id).await;

            handle.enqueue_source(new_input);
        }
        Err(why) => {
            panic!("Cannot join voice channel: {}", why)
        }
    }
}

async fn send_interaction_message_basic_from_message(
    ctx: &Context,
    command: &MessageComponentInteraction,
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

pub async fn hanle_fast_forward_audio(
    ctx: &Context,
    command: &MessageComponentInteraction,
    length: u64,
) {
    let (guild_id, _channel_id) = get_guild_channel_id_from_interaction_message(command, ctx).await;
    let manager = get_songbird_manager(ctx).await;

    match manager.get(guild_id) {
        Some(handle_lock) => {
            // handle_handle(handle_lock, command, ctx).await;
            let handle = handle_lock.lock().await;
            let queue = handle.queue();
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
            current_time += std::time::Duration::from_secs(length);

            match current.seek_time(current_time) {
                Ok(_) => {}
                Err(err) => {
                    panic!("Cannot ff: {}", err);
                }
            };
            match command
                .create_interaction_response(ctx, |f| {
                    f.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|f| {
                            f.content("ff")
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

pub async fn handle_stop_audio(ctx: &Context, command: &MessageComponentInteraction) {
    let (guild_id, _channel_id) = get_guild_channel_id_from_interaction_message(command, ctx).await;
    let manager = get_songbird_manager(ctx).await;

    match manager.get(guild_id) {
        Some(handle_lock) => {
            // handle_handle(handle_lock, command, ctx).await;
            let handle = handle_lock.lock().await;
            let queue = handle.queue();
            // TODO:
            if queue.is_empty() {
                // queue empty send appropriate message
                println!("queue is empty on stop");
            } else {
            }

            queue.stop();

            match command
                .create_interaction_response(ctx, |f| {
                    f.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message.content("Song stoped and queue cleared")
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

pub async fn handle_play_pause_audio(ctx: &Context, command: &MessageComponentInteraction) {
    let (guild_id, _channel_id) = get_guild_channel_id_from_interaction_message(command, ctx).await;
    let manager = get_songbird_manager(ctx).await;
    let lavalink = get_lavalink_client(ctx).await;
    info!("here");
    match manager.get(guild_id) {
        Some(_handle_lock) => {
            info!("got handle");

            if let Some(ok) = lavalink
                .nodes()
                .await
                .get(&command.guild_id.expect("expected guild_id").0)
            {
                info!("is paused: {}", ok.is_paused);

                match lavalink.set_pause(guild_id.0, !ok.is_paused).await {
                    Ok(_) => {}
                    Err(err) => {
                        panic!("error when setting paused: {}", err);
                    }
                };
            } else {
                info!("No node???");
            }
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

pub async fn send_defered_response(command: &MessageComponentInteraction, ctx: &Context) -> bool {
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

pub async fn send_interaction_message_basic(
    command: &MessageComponentInteraction,
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
