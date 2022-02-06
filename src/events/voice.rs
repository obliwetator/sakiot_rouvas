use serenity::client::Context;
use tracing::info;

use crate::config::APPLICATION_ID;

use super::interactions::{get_songbird_manager, lavalink::get_lavalink_client};

pub async fn voice_server_update(
    _ctx: Context,
    _update: serenity::model::event::VoiceServerUpdateEvent,
) {
}

pub async fn voice_state_update(
    ctx: Context,
    guild_id: Option<serenity::model::id::GuildId>,
    _old_state: Option<serenity::model::prelude::VoiceState>,
    new_state: serenity::model::prelude::VoiceState,
) {
    // if new_state.user_id.0 == APPLICATION_ID && new_state.channel_id.is_none() {
    // let manager = get_songbird_manager(&ctx).await;
    // let lavalink = get_lavalink_client(&ctx).await;
    // match lavalink.destroy(guild_id.unwrap().0).await {
    //     Ok(_) => {
    //         let nodes = lavalink.nodes().await;
    //         nodes.remove(&guild_id.unwrap().0);

    //         let loops = lavalink.loops().await;
    //         loops.remove(&guild_id.unwrap().0);
    //     }
    //     Err(err) => {
    //         panic!("cannot remove lavalink: {}", err);
    //     }
    // }
    // match manager.remove(guild_id.expect("expected guild_id")).await {
    //     Ok(_) => {
    //         info!("Call removed")
    //     }
    //     Err(err) => {
    //         panic!("cannot leave channel: {}", err);
    //     }
    // };
    // match manager.get(guild_id.unwrap()) {
    //     Some(handle_lock) => handle_lock.lock().await.le,
    //     None => {}
    // };
    // }
    // info!("voice state old update: {:#?}", old_state);
    // info!("voice state new update: {:#?}", new_state);
}
