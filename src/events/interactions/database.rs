use mysql_async::prelude::Queryable;
use serenity::client::Context;
use tracing::info;

use crate::database::get_conn_from_ctx;

pub async fn add_track_to_db(
    ctx: Context,
    guild_id: serenity::model::id::GuildId,
    title: String,
    ext: String,
) {
    let mut conn = get_conn_from_ctx(&ctx).await;
    let query = format!(
        "INSERT IGNORE INTO jam_it (id, guild_id, audio_name, ext) VALUES (NULL, '{}', '{}', '{}')",
        guild_id, title, ext
    );

    match conn.query_drop(query).await {
        Ok(_) => {}
        Err(err) => {
            panic!("error when trying to insert: {}", err)
        }
    }

    info!("Downloaded track: {}.{}", title, ext);
}
