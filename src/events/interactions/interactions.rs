use std::{sync::Arc, time::Instant};

use serenity::{
    async_trait,
    http::Http,
    model::{
        id::{ChannelId, GuildId},
        interactions::{
            application_command::{
                ApplicationCommand, ApplicationCommandInteractionDataOptionValue,
            },
            Interaction, InteractionApplicationCommandCallbackDataFlags, InteractionResponseType,
        },
    },
    prelude::*,
};
use songbird::{input::Restartable, Event, EventContext, EventHandler as VoiceEventHandler};

use crate::events::interactions::message_component::{
    handle_delete_and_skip_from_jam, handle_jam_it, handle_next_audio_in_queue,
    handle_play_pause_audio, handle_stop_audio, hanle_fast_forward_audio,
};
use crate::events::interactions::{
    application_command::*, database::add_track_to_db, helpers::ytdl_input_from_string,
};

pub struct TrackEndNotifier {
    pub chann_id: ChannelId,
    pub http: Arc<Http>,
}

#[async_trait]
impl VoiceEventHandler for TrackEndNotifier {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        if let EventContext::Track(_track_list) = ctx {
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
// #[async_trait]
// impl VoiceEventHandler for TrackPlayNotifier {
//     async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
//         if let EventContext::Track(track_list) = ctx {
//             let map_lock = {
//                 let data_read = self.ctx.data.read().await;
//                 let data = data_read.get::<GuildTrackMap>().expect("msg").clone();
//                 data
//             };
//             let mutex_guard = map_lock.lock().await;
//             let guild_track = mutex_guard
//                 .get(&self.guild_id.0)
//                 .expect("expected initialized value");
//             let vol = guild_track.volume;

//             track_list[0]
//                 .1
//                 .set_volume(vol)
//                 .expect("cannot set volue to next track");
//         }
//         None
//     }
// }

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
    } else if let Interaction::Ping(_command) = interaction {
        println!("ping");
    }

    println!("Time elapsed: {} seconds", now.elapsed().as_secs_f64());
    println!("Time elapsed: {} micros", now.elapsed().as_micros());
    println!("Time elapsed: {} nanos", now.elapsed().as_nanos());
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

pub async fn application_command_create(_ctx: Context, _application_command: ApplicationCommand) {
    todo!()
}

pub async fn application_command_update(_ctx: Context, _application_command: ApplicationCommand) {
    todo!()
}

pub async fn application_command_delete(_ctx: Context, _application_command: ApplicationCommand) {
    todo!()
}
