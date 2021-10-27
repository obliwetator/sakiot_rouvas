use serenity::client::Context;

use crate::Lavalink;

pub(crate) async fn get_lavalink_client(ctx: &Context) -> lavalink_rs::LavalinkClient {
    let data = ctx.data.read().await;
    let lava_client = data.get::<Lavalink>().unwrap().clone();

    lava_client
}
