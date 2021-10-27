use serenity::{
    client::Context,
    model::{
        channel::ReactionType,
        id::{ChannelId, EmojiId, GuildId},
        interactions::{
            application_command::{
                ApplicationCommandInteraction, ApplicationCommandInteractionDataOptionValue,
            },
            message_component::ButtonStyle,
            InteractionApplicationCommandCallbackDataFlags, InteractionResponseType,
        },
    },
    prelude::Mutex,
};
use songbird::{input::Restartable, Call, Event, TrackEvent};

use crate::{
    events::interactions::{get_songbird_manager, interactions::download_track_async},
    GuildTrack, GuildTrackMap, Lavalink,
};

use std::{fmt::Write, sync::Arc};

use super::{
    database::add_track_to_db,
    helpers::{
        get_guild_channel_id_from_interaction_application, join_voice_channel,
        not_in_a_voice_channel_application, ytdl_input_from_string_link,
    },
    interactions::{TrackEndNotifier, TrackPlayNotifier},
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

    let manager = get_songbird_manager(ctx).await;

    match manager.get(guild_id) {
        Some(handle_lock) => {
            process_audio_link(handle_lock, command, ctx, guild_id).await;
            // handle_handle(handle_lock, command, ctx).await;
        }
        None => {
            // No handle bot is not in a channel is this guild. Create a new handle
            handle_no_handle(manager, guild_id, connect_to, ctx, command).await;
        }
    };
    // TODO: Check if we are already playing anything.
}

pub async fn display_current_queue(ctx: &Context, command: &ApplicationCommandInteraction) {
    let (guild_id, channel_id) =
        get_guild_channel_id_from_interaction_application(command, ctx).await;
    let manager = get_songbird_manager(ctx).await;

    match manager.get(guild_id) {
        Some(handle_lock) => {
            // handle_handle(handle_lock, command, ctx).await;
            let handle = handle_lock.lock().await;
            let queue = handle.queue();
            if queue.is_empty() {
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

            let current_queue = queue.current_queue();
            let mut output = String::from("Currently queued tracks\n");
            for (i, ele) in current_queue.iter().enumerate() {
                let metadata = ele.metadata().clone();
                let artist = &metadata
                    .artist
                    .unwrap_or_else(|| "Unkown artist".to_string());
                let title = &metadata.title.unwrap_or_else(|| "Unkown title".to_string());

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

    let connect_to = match not_in_a_voice_channel_application(channel_id, command, ctx).await {
        Some(value) => value,
        None => return,
    };

    match command.data.options.get(0) {
        Some(float) => {
            let option = float.resolved.as_ref().expect("cannot get value");
            // we have a value change the volume
            if let ApplicationCommandInteractionDataOptionValue::Number(value) = option {
                let value_percent = (*value / 100.0) as f32;
                if !(0.0..2.01).contains(&value_percent) {
                    send_interaction_message_basic(command, ctx, "vol must be > 0 and < 200").await;
                    return;
                }

                let manager = get_songbird_manager(ctx).await;
                match manager.get(guild_id) {
                    Some(handle_lock) => {
                        let handle = handle_lock.lock().await;
                        let queue = handle.queue().clone();
                        // Try to set the currently playing song volume
                        if let Some(ok) = queue.current() {
                            ok.set_volume(value_percent).expect("cannot set volume");
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
                    guild_track.volume = value_percent;
                }

                send_interaction_message_basic(
                    command,
                    ctx,
                    format!("Volume changed to :{}%", value_percent * 100.0).as_str(),
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
                    format!("volume is set to: {}%", vol * 100.0).as_str(),
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

    let manager = get_songbird_manager(ctx).await;

    match manager.get(guild_id) {
        Some(handle_lock) => {
            let handle = handle_lock.lock().await;
            let queue = handle.queue().clone();
            let option = command
                .data
                .options
                .get(0)
                .expect("Expected required field")
                .resolved
                .as_ref()
                .expect("Expected required value");
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

                    // if let Err(why) = &lava_client
                    //     .play(guild_id.0, query_information.tracks[0].clone())
                    //     // Change this to play() if you want your own custom queue or no queue at all.
                    //     .queue()
                    //     .await
                    // {
                    //     eprintln!("{}", why);
                    // };
                    println!("Tracks added: {:#?}", query_information.tracks);
                    println!("added to queue len: {}", query_information.tracks.len());

                    match command
                        .edit_original_interaction_response(ctx, |f| {
                            f.content(format!(
                                "added {} songs to the queue",
                                query_information.tracks.len()
                            ))
                        })
                        .await
                    {
                        Ok(_) => {}
                        Err(err) => {
                            panic!("cannot send response err 2: {}", err)
                        }
                    };

                    // let count = ytdl_playlist(option).await;
                    // play_audio_from_string(
                    //     command,
                    //     ctx,
                    //     format!("testing playlist: {} song in the playlist", count).as_str(),
                    // )
                    // .await;
                    // let (ffmpeg, title, ext) = result.await;
                    // add_track_to_db(ctx, guild_id, title, ext).await;
                } else {
                    println!("error in play playlist");
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
            // handle_handle(handle_lock, command, ctx).await;
        }
        None => {
            // No handle bot is not in a channel is this guild. Create a new handle
            handle_no_handle(manager, guild_id, connect_to, ctx, command).await;
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
            {
                let data = ctx.data.read().await;
                let lava_client = data.get::<Lavalink>().unwrap().clone();
                lava_client
                    .create_session_with_songbird(&connection_info)
                    .await
                    .expect("cannot create lavalink session");

                let data = data
                    .get::<GuildTrackMap>()
                    .expect("cannot get GuildTrackMap")
                    .clone();
                let mut mutex_guard = data.lock().await;
                // TODO: Get value from DB
                mutex_guard.insert(guild_id.0, GuildTrack { volume: 0.5 });
            }

            let mut handle = handle_lock.lock().await;
            let queue = handle.queue().clone();
            let send_http = ctx.http.clone();
            let chann_id = command.channel_id;

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

            send_interaction_message_basic(command, ctx, "joined").await;
        }
        Err(why) => {
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

pub async fn process_audio_link(
    handle_lock: Arc<Mutex<Call>>,
    command: &ApplicationCommandInteraction,
    ctx: &Context,
    guild_id: GuildId,
) {
    let mut handle = handle_lock.lock().await;
    let queue = handle.queue().clone();
    let option = command
        .data
        .options
        .get(0)
        .expect("Expected required field")
        .resolved
        .as_ref()
        .expect("Expected required value");
    if let ApplicationCommandInteractionDataOptionValue::String(option) = option {
        // TDOD: proper link validation
        if option.contains("youtube.com") {
            let result = ytdl_input_from_string_link(option);
            let ytdl_link = Restartable::ytdl(option.to_owned(), true)
                .await
                .expect("cannot");
            // COnvert to input to access metadata
            let input = songbird::input::Input::from(ytdl_link);

            let metadata = input.metadata.clone();
            let artist = &metadata
                .artist
                .unwrap_or_else(|| "Unkown artist".to_string());
            let title = &metadata.title.unwrap_or_else(|| "Unkown title".to_string());

            handle.enqueue_source(input);

            play_audio_from_string(command, ctx, title).await;
            let (ffmpeg, title, ext) = result.await;
            add_track_to_db(ctx.clone(), guild_id, title, ext).await;
            // play_audio_from_string(command, ctx, &queue, final_title).await;
        } else if option.contains("patrykstyla.com") {
            match command
                .edit_original_interaction_response(ctx, |f| f.content("Not working yet"))
                .await
            {
                Ok(_) => {}
                Err(err) => {
                    panic!("cannot send response err 2: {}", err)
                }
            };
        } else {
            let lava_client = get_lavalink_client(ctx).await;
            let query_information = lava_client
                .search_tracks(option)
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

            if let Err(why) = &lava_client
                .play(guild_id.0, query_information.tracks[0].clone())
                // Change this to play() if you want your own custom queue or no queue at all.
                .queue()
                .await
            {
                eprintln!("{}", why);
            };

            download_track_async(ctx, option, guild_id).await;

            play_audio_from_string(
                command,
                ctx,
                query_information.tracks[0]
                    .info
                    .as_ref()
                    .expect("expected info lavalink")
                    .title
                    .as_str(),
            )
            .await;
            // let (ffmpeg, title, ext) = result.await;
        }
        println!("audio queued len: {}", &queue.len());
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

pub async fn handle_no_handle(
    manager: Arc<songbird::Songbird>,
    guild_id: serenity::model::id::GuildId,
    connect_to: ChannelId,
    ctx: &Context,
    command: &ApplicationCommandInteraction,
) {
    match join_voice_channel(manager, ctx, guild_id, connect_to, command.channel_id).await {
        Ok(handle_lock) => {
            let mut handle = handle_lock.lock().await.clone();
            let queue = handle.queue().clone();

            let option = get_option_at_index_application_command(command, 0).await;
            if let ApplicationCommandInteractionDataOptionValue::String(string_result) = option {
                // TDOD: proper link validation
                if string_result.contains("youtube.com") {
                    handle_youtube(string_result, &mut handle, command, ctx, guild_id).await;
                } else if string_result.contains("patrykstyla.com") {
                    handle_patryk_application_command(command, ctx).await;
                } else {
                    // process string
                    handle_search_string_application_command(
                        string_result,
                        handle,
                        &queue,
                        ctx,
                        guild_id,
                        command,
                    )
                    .await;
                }

                println!("audio queued len: {}", queue.len());
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
        Err(why) => {
            panic!("cannot join channel");
        }
    }
}

pub async fn handle_youtube(
    option: &str,
    handle: &mut Call,
    command: &ApplicationCommandInteraction,
    ctx: &Context,
    guild_id: GuildId,
) {
    let result = ytdl_input_from_string_link(option);
    let ytdl_link = Restartable::ytdl(option.to_owned(), true)
        .await
        .expect("cannot");
    let input = songbird::input::Input::from(ytdl_link);
    let metadata = input.metadata.clone();
    let artist = &metadata
        .artist
        .unwrap_or_else(|| "Unkown artist".to_string());
    let title = &metadata.title.unwrap_or_else(|| "Unkown title".to_string());
    handle.enqueue_source(input);
    play_audio_from_string(command, ctx, title).await;
    let (ffmpeg, title, ext) = result.await;
    add_track_to_db(ctx.clone(), guild_id, title, ext).await;
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
    string_result: &str,
    mut handle: Call,
    queue: &songbird::tracks::TrackQueue,
    ctx: &Context,
    guild_id: GuildId,
    command: &ApplicationCommandInteraction,
) {
    println!("not working");
    // let result = ytdl_input_from_string(string_result);
    // let ytdl_search = Restartable::ytdl_search(string_result, true)
    //     .await
    //     .expect("cannot");
    // let input = songbird::input::Input::from(ytdl_search);
    // let metadata = input.metadata.clone();
    // let artist = &metadata
    //     .artist
    //     .unwrap_or_else(|| "Unkown artist".to_string());
    // let title = &metadata.title.unwrap_or_else(|| "Unkown title".to_string());
    // handle.enqueue_source(input);
    // let curr_queue = queue.current().expect("");
    // {
    //     let map_lock = {
    //         let data_read = ctx.data.read().await;
    //         let data = data_read.get::<GuildTrackMap>().expect("msg").clone();
    //         data
    //     };
    //     let mutex_guard = map_lock.lock().await;
    //     let guild_track = mutex_guard
    //         .get(&guild_id.0)
    //         .expect("expected initialized value");
    //     let vol = guild_track.volume;
    //     // reduce the volume of the first track
    //     if queue.len() == 1 {
    //         // reduce the volume of the first track
    //         queue
    //             .current()
    //             .expect("cannot get current queue")
    //             .set_volume(vol)
    //             .expect("cannot set volume");
    //     }
    // }
    // play_audio_from_string(command, ctx, title).await;
    // let (ffmpeg, title, ext) = result.await;
    // add_track_to_db(ctx, guild_id, title, ext).await;
}
