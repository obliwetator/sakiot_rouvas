use std::{
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
};

use serde_json::Value;
use serenity::{
    client::Context,
    model::{
        id::ChannelId,
        interactions::{
            application_command::ApplicationCommandInteraction,
            message_component::MessageComponentInteraction,
        },
    },
};
use songbird::input::Restartable;
use tokio::task;

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

pub async fn ytdl_input_from_string(
    option: &str,
) -> (songbird::input::Restartable, String, String) {
    let stra = format!("ytsearch:{}", option);
    let command = Command::new("yt-dlp")
        .args([
            "--print-json",
            "--embed-metadata",
            "-f",
            "webm[abr>0]/bestaudio/best",
            "-R",
            "infinite",
            "--no-playlist",
            "--no-warnings",
            stra.as_str(),
            "-o",
            "/home/ubuntu/projects/sakiot_rouvas/temp_video/%(title)s.%(ext)s",
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

    let ffmpeg_input = Restartable::ffmpeg(
        format!(
            "/home/ubuntu/projects/sakiot_rouvas/temp_video/{}.{}",
            &title, ext
        ),
        true,
    )
    .await
    .expect("error with ffmpeg songibrd");

    (ffmpeg_input, title, ext)
}

pub async fn ytdl_input_from_string_link(
    option: &str,
) -> (songbird::input::Restartable, String, String) {
    let command = Command::new("yt-dlp")
        .args([
            "--print-json",
            "--embed-metadata",
            "-f",
            "webm[abr>0]/bestaudio/best",
            "-R",
            "infinite",
            "--no-playlist",
            "--no-warnings",
            option,
            "-o",
            "/home/ubuntu/projects/sakiot_rouvas/temp_video/%(title)s.%(ext)s",
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

    let ffmpeg_input = Restartable::ffmpeg(
        format!(
            "/home/ubuntu/projects/sakiot_rouvas/temp_video/{}.{}",
            &title, ext
        ),
        true,
    )
    .await
    .expect("error with ffmpeg songibrd");

    (ffmpeg_input, title, ext)
}

pub async fn ytdl_playlist(option: &str) -> u64 {
    let mut yt_dlp = Command::new("yt-dlp")
        .args([
            "--print-json",
            "--embed-metadata",
            "-f",
            "webm[abr>0]/bestaudio/best",
            "-R",
            "infinite",
            "--no-warnings",
            option,
            "-o",
            "-",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("cannot get yt-dlp playlist");

    // This rigmarole is required due to the inner synchronous reading context.
    let stderr = yt_dlp.stderr.take();

    let (returned_stderr, value) = task::spawn_blocking(move || {
        let mut s = stderr.unwrap();
        let out: songbird::input::error::Result<Value> = {
            let mut o_vec = vec![];
            let mut serde_read = BufReader::new(s.by_ref());
            // Newline...
            if let Ok(len) = serde_read.read_until(0xA, &mut o_vec) {
                serde_json::from_slice(&o_vec[..len]).map_err(|err| {
                    songbird::input::error::Error::Json {
                        error: err,
                        parsed_text: std::str::from_utf8(&o_vec).unwrap_or_default().to_string(),
                    }
                })
            } else {
                Result::Err(songbird::input::error::Error::Metadata)
            }
        };

        (s, out)
    })
    .await
    .unwrap();

    yt_dlp.stderr = Some(returned_stderr);

    let taken_stdout = yt_dlp.stdout.take().expect("cannot get stdout palylist");

    let obj = value.expect("expected json");

    let playlist_count = obj["n_entries"].as_u64().expect("playlist len no found");

    println!("entries: {}", playlist_count);

    // process will wait indefinetly since the pipe is not consumed(?)
    // let output = yt_dlp.wait_with_output().expect("cannot wait with output");
    // let stdout_o = std::str::from_utf8(&output.stdout).expect("expected output in stdout");
    // let stderr_o = std::str::from_utf8(&output.stderr).expect("expected output in stderr");
    // println!("stdout :{}", stdout_o);
    // println!("stderr: {}", stderr_o);

    // let err_result = std::str::from_utf8(v)

    playlist_count
}
