use std::{convert::TryInto, sync::Arc, time::Instant};

use mysql_async::{prelude::Queryable, Row};
use serenity::{
    async_trait,
    http::Http,
    model::{
        channel::ReactionType,
        id::{ChannelId, EmojiId, GuildId},
        interactions::{
            application_command::{
                ApplicationCommand, ApplicationCommandInteraction,
                ApplicationCommandInteractionDataOptionValue,
            },
            message_component::{ButtonStyle, MessageComponentInteraction},
            Interaction, InteractionApplicationCommandCallbackDataFlags, InteractionResponseType,
        },
    },
    prelude::*,
};
use songbird::{
    input::{Input, Restartable},
    Event, EventContext, EventHandler as VoiceEventHandler, TrackEvent,
};

use crate::{
    database::get_conn_from_ctx,
    events::interactions::{
        application_command::*, database::add_track_to_db, helpers::ytdl_input_from_string,
    },
    GuildTrack, GuildTrackMap,
};

use super::{
    get_songbird_manager,
    helpers::{
        get_guild_channel_id_from_interaction_application,
        get_guild_channel_id_from_interaction_message,
    },
};

pub struct TrackEndNotifier {
    pub chann_id: ChannelId,
    pub http: Arc<Http>,
}

#[async_trait]
impl VoiceEventHandler for TrackEndNotifier {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        if let EventContext::Track(track_list) = ctx {
            self.chann_id
                .say(&self.http, &format!("track ended. queue len: {}", 0))
                .await
                .expect("cannot send message");
        }
        None
    }
}

pub struct TrackPlayNotifier {
    pub ctx: Context,
    pub guild_id: GuildId,
}
#[async_trait]
impl VoiceEventHandler for TrackPlayNotifier {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        if let EventContext::Track(track_list) = ctx {
            let map_lock = {
                let data_read = self.ctx.data.read().await;
                let data = data_read.get::<GuildTrackMap>().expect("msg").clone();
                data
            };
            let mutex_guard = map_lock.lock().await;
            let guild_track = mutex_guard
                .get(&self.guild_id.0)
                .expect("expected initialized value");
            let vol = guild_track.volume;

            track_list[0]
                .1
                .set_volume(vol)
                .expect("cannot set volue to next track");
        }
        None
    }
}

pub async fn interaction_create(ctx: Context, interaction: Interaction) {
    let now = Instant::now();
    if let Interaction::ApplicationCommand(command) = interaction {
        println!("interaction name: {}", command.data.name);
        match command.data.name.as_str() {
            "j" => handle_j(&ctx, &command).await,
            "que" => display_current_queue(&ctx, &command).await,
            "help" => send_interaction_message_basic(&command, &ctx, ":(").await,
            "vol" => handle_vol(&ctx, &command).await,
            "playlist" => handle_playlist(&ctx, &command).await,
            "join" => handle_join(&ctx, &command).await,
            "ff" => {
                let option = command
                    .data
                    .options
                    .get(0)
                    .expect("Expected required field")
                    .resolved
                    .as_ref()
                    .expect("Expected required value");
                if let ApplicationCommandInteractionDataOptionValue::Integer(value) = option {
                    if *value < 0 {
                        // No queue present. Send approriate message
                        match command
                            .create_interaction_response(&ctx.http, |response| {
                                response
                                    .kind(InteractionResponseType::ChannelMessageWithSource)
                                    .interaction_response_data(|message| {
                                        message.content("Provide a positive number")
										.flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
                                    })
                            })
                            .await
                        {
                            Ok(_) => {}
                            Err(err) => {
                                panic!("cannot send response err 2: {}", err)
                            }
                        };

                        return;
                    }
                    hanle_fast_forward_audio_application_command(&ctx, &command, *value).await
                } else {
                    match command
                        .edit_original_interaction_response(ctx, |f| f.content("Provide a number"))
                        .await
                    {
                        Ok(_) => {}
                        Err(err) => {
                            panic!("cannot send response err 2: {}", err)
                        }
                    };
                }
            }
            _ => {
                send_interaction_message_basic(
                    &command,
                    &ctx,
                    format!(
                        "No command with the name {}. This is probably not your fault",
                        command.data.name
                    )
                    .as_str(),
                )
                .await;
            }
        };
    } else if let Interaction::MessageComponent(command) = interaction {
        println!("Message component command: {}", command.data.custom_id);
        match command.data.custom_id.as_str() {
            "play" => {
                handle_play_pause_audio(&ctx, &command).await;
            }
            "next" => {
                handle_next_audio_in_queue(&ctx, &command).await;
            }
            "stop" => {
                handle_stop_audio(&ctx, &command).await;
            }
            "ff" => {
                hanle_fast_forward_audio(&ctx, &command, 15).await;
            }
            "jam_it" => {
                handle_jam_it(&ctx, &command).await;
            }
            "delete_and_skip" => {
                handle_delete_and_skip_from_jam(&ctx, &command).await;
            }
            _ => {
                if let Err(why) = command
				.create_interaction_response(&ctx, |f| {
					f.kind(InteractionResponseType::ChannelMessageWithSource)
						.interaction_response_data(|message| message.content("Unkown button clicked. This probably not your fault").flags(serenity::model::interactions::InteractionApplicationCommandCallbackDataFlags::EPHEMERAL))
				})
				.await
			{
				println!("Cannot respond to slash command 1: {}", why);
			}
                panic!("Unkown custom id")
            }
        }
    } else if let Interaction::Ping(command) = interaction {
        println!("ping");
    }

    println!("Time elapsed: {} seconds", now.elapsed().as_secs_f64());
    println!("Time elapsed: {} micros", now.elapsed().as_micros());
    println!("Time elapsed: {} nanos", now.elapsed().as_nanos());
}

async fn handle_delete_and_skip_from_jam(ctx: &Context, command: &MessageComponentInteraction) {
    // TODO: lock is aquired in each function
    handle_next_audio_in_queue(ctx, command).await;

    delete_from_jam(command, ctx).await;
}

async fn delete_from_jam(command: &MessageComponentInteraction, ctx: &Context) {
    let (guild_id, channel_id) = get_guild_channel_id_from_interaction_message(command, ctx).await;

    let connect_to = match not_in_a_voice_channel_message(channel_id, command, ctx).await {
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

struct JamIt {
    // guild_id: u64,
    audio_name: String,
    ext: String,
}

async fn handle_jam_it(ctx: &Context, command: &MessageComponentInteraction) {
    // Immediatly respond to the interaction which we will edit later
    match command
        .create_interaction_response(&ctx.http, |response| {
            response
                .kind(InteractionResponseType::DeferredChannelMessageWithSource)
                .interaction_response_data(|message| {
                    message
                        .components(|comp| {
                            comp.create_action_row(|row| {
                                row.create_button(|btn| {
                                    btn.custom_id("foo")
                                        .label("click me")
                                        .style(ButtonStyle::Primary)
                                })
                            })
                        })
                        .content("Processing")
                })
        })
        .await
    {
        Ok(_) => {}
        Err(err) => {
            println!("Cannot respond to slash command 2: {}", err)
        }
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

// async fn ffrobe_header(audio_name: &str, ext: &str) -> Vec<u8> {
//     let command = Command::new("ffprobe")
//         .args([
//             "-i",
//             format!("/home/projects/sakiot_rouvas/{}.{}", audio_name, ext).as_str(),
//         ])
//         .output()
//         .expect("cannot start ffprobe");

//     command.stdout
// }

async fn hanle_fast_forward_audio_application_command(
    ctx: &Context,
    command: &ApplicationCommandInteraction,
    length: i64,
) {
    let (guild_id, channel_id) =
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

async fn hanle_fast_forward_audio(
    ctx: &Context,
    command: &MessageComponentInteraction,
    length: u64,
) {
    let (guild_id, channel_id) = get_guild_channel_id_from_interaction_message(command, ctx).await;
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

async fn handle_stop_audio(ctx: &Context, command: &MessageComponentInteraction) {
    let (guild_id, channel_id) = get_guild_channel_id_from_interaction_message(command, ctx).await;
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

async fn handle_play_pause_audio(ctx: &Context, command: &MessageComponentInteraction) {
    let (guild_id, channel_id) = get_guild_channel_id_from_interaction_message(command, ctx).await;
    let manager = get_songbird_manager(ctx).await;

    match manager.get(guild_id) {
        Some(handle_lock) => {
            // handle_handle(handle_lock, command, ctx).await;
            let handle = handle_lock.lock().await;
            let queue = handle.queue();

            let track = match queue.current() {
                Some(ok) => ok.get_info().await.expect("cannot get info").playing,
                None => {
                    // TODO: send error message
                    panic!("no track found");
                }
            };
            match track {
                songbird::tracks::PlayMode::Play => {
                    // If we are currently playing, puase
                    match queue.pause() {
                        Ok(_) => {
                            match command
                                .create_interaction_response(ctx, |f| {
                                    f.kind(InteractionResponseType::ChannelMessageWithSource)
                                        .interaction_response_data(|message| {
                                            message.content("paused")
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
                        Err(err) => {
                            match command
                                .create_interaction_response(ctx, |f| {
                                    f.kind(InteractionResponseType::ChannelMessageWithSource)
                                        .interaction_response_data(|message| {
                                            message.content("Cannot pause?").flags(
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
                        }
                    };
                }
                songbird::tracks::PlayMode::Pause => {
                    // If we are currently paused, play
                    match queue.resume() {
                        Ok(_) => {
                            match command
                                .create_interaction_response(ctx, |f| {
                                    f.kind(InteractionResponseType::ChannelMessageWithSource)
                                        .interaction_response_data(|message| {
                                            message.content("resumed")
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
                        Err(err) => {
                            match command
                                .create_interaction_response(ctx, |f| {
                                    f.kind(InteractionResponseType::ChannelMessageWithSource)
                                        .interaction_response_data(|message| {
                                            message.content("There is no queue to skip").flags(
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
                        }
                    };
                }
                songbird::tracks::PlayMode::Stop => {
                    // TODO: handle this
                }
                songbird::tracks::PlayMode::End => {
                    // TODO: handle this
                }
                _ => {
                    panic!("unkown PlayMode")
                }
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

async fn handle_next_audio_in_queue(
    ctx: &Context,
    command: &serenity::model::interactions::message_component::MessageComponentInteraction,
) {
    let (guild_id, channel_id) = get_guild_channel_id_from_interaction_message(command, ctx).await;
    let manager = get_songbird_manager(ctx).await;

    match manager.get(guild_id) {
        Some(handle_lock) => {
            // handle_handle(handle_lock, command, ctx).await;
            let handle = handle_lock.lock().await;
            let queue = handle.queue();
            match queue.skip() {
                Ok(_) => {}
                Err(err) => {
                    match command
                        .create_interaction_response(ctx, |f| {
                            f.kind(InteractionResponseType::ChannelMessageWithSource)
                                .interaction_response_data(|message| {
                                    message.content("There is no queue to skip").flags(
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
                }
            };
            // let a = queue.dequeue(0).expect("msg");

            match command
                .create_interaction_response(ctx, |f| {
                    f.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| message.content("song skipped"))
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

pub async fn download_track_async(ctx: &Context, option: &str, guild_id: GuildId) {
    let ctx1 = ctx.clone();
    let stra = Arc::new(option.to_owned());
    tokio::spawn(async move {
        let (title, ext) = ytdl_input_from_string(&stra).await;
        add_track_to_db(ctx1, guild_id, title, ext).await;
    });
}

// async fn handle_handle(
//     handle_lock: Arc<Mutex<songbird::Call>>,
//     command: &ApplicationCommandInteraction,
//     ctx: &Context,
// ) {
//     // We have a handle. Bot is in a channel in this guild
//     // Audio is:
//     // 1) already playing and we should queue the incoming request
//     // 2) The bot hasn't left the channel after it stopped playing
//     // ?)
//     let mut handle = handle_lock.lock().await;
//     let queue = handle.queue().clone();
//     if queue.is_empty() {
//         println!("queue is empty");
//         // No audio queued
//     } else {
//         // audio queued
//         // println!("audio queued len: {}", queue.len());
//     }
//     let option = command
//         .data
//         .options
//         .get(0)
//         .expect("Expected required field")
//         .resolved
//         .as_ref()
//         .expect("Expected required value");
//     if let ApplicationCommandInteractionDataOptionValue::String(option) = option {
//         // let (ffmpeg_input, title) = ytdl_and_ffmpeg_input_from_string(option).await;

//         let ytdl_search = Restartable::ytdl_search(option, true)
//             .await
//             .expect("cannot");
//         let input = songbird::input::Input::from(ytdl_search);

//         let metadata = input.metadata.clone();
//         let artist = &metadata
//             .artist
//             .unwrap_or_else(|| "Unkown artist".to_string());
//         let title = &metadata.title.unwrap_or_else(|| "Unkown title".to_string());
//         let final_title = format!("{} - {}", artist, title);
//         // TDOD: proper link validation
//         if option.contains("youtube.com") {
//             handle.enqueue_source(input);
//             play_audio_from_string(command, ctx, &queue, final_title).await;
//         } else if option.contains("patrykstyla.com") {
//             match command
//                 .edit_original_interaction_response(ctx, |f| f.content("Not working yet"))
//                 .await
//             {
//                 Ok(_) => {}
//                 Err(err) => {
//                     panic!("cannot send response err 2: {}", err)
//                 }
//             };
//         } else {
//             // process string
//             handle.enqueue_source(input);
//             play_audio_from_string(command, ctx, &queue, final_title).await;
//         }
//         println!("audio queued len: {}", &queue.len());
//     } else {
//         // Not a string. This should not happen?
//         match command
//             .edit_original_interaction_response(ctx, |f| f.content("Provide a string"))
//             .await
//         {
//             Ok(_) => {}
//             Err(err) => {
//                 panic!("cannot send response err 2: {}", err)
//             }
//         };
//     }
// }

pub async fn ffmpeg_input_from_string(title: &str, ext: &str) -> songbird::input::Restartable {
    let ffmpeg_input = Restartable::ffmpeg(
        format!(
            "/home/ubuntu/projects/sakiot_rouvas/temp_video/{}.{}",
            &title, ext
        ),
        true,
    )
    .await
    .expect("error with ffmpeg songibrd");

    ffmpeg_input
}

async fn handle_no_handle_from_message_component(
    manager: Arc<songbird::Songbird>,
    guild_id: serenity::model::id::GuildId,
    connect_to: ChannelId,
    ctx: &Context,
    command: &MessageComponentInteraction,
    new_input: Input,
) {
    let (handle_lock, success) = manager.join(guild_id, connect_to).await;
    match success {
        Ok(_) => {}
        Err(err) => {
            panic!("Cannot join voice channel: {}", err)
        }
    }
    let mut handle = handle_lock.lock().await;
    let queue = handle.queue().clone();
    let send_http = ctx.http.clone();
    let chann_id = command.channel_id;

    {
        let data_write = ctx.data.read().await;
        let data = data_write
            .get::<GuildTrackMap>()
            .expect("cannot get GuildTrackMap")
            .clone();
        let mut mutex_guard = data.lock().await;
        // TODO: Get value from DB
        mutex_guard.insert(guild_id.0, GuildTrack { volume: 0.5 });
    }

    handle.add_global_event(
        Event::Track(TrackEvent::End),
        TrackEndNotifier {
            chann_id,
            http: send_http.clone(),
        },
    );

    handle.add_global_event(
        Event::Track(TrackEvent::Play),
        TrackPlayNotifier {
            ctx: ctx.clone(),
            guild_id,
        },
    );

    handle.enqueue_source(new_input);
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

pub async fn application_command_create(_ctx: Context, _application_command: ApplicationCommand) {
    todo!()
}

pub async fn application_command_update(_ctx: Context, _application_command: ApplicationCommand) {
    todo!()
}

pub async fn application_command_delete(_ctx: Context, _application_command: ApplicationCommand) {
    todo!()
}
