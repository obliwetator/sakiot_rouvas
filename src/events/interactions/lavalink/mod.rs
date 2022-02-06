use serenity::client::Context;

use crate::Lavalink;

pub(crate) async fn get_lavalink_client(ctx: &Context) -> lavalink_rs::LavalinkClient {
    let data = ctx.data.read().await;
    let lavalink = data.get::<Lavalink>().unwrap().clone();

    lavalink
}
