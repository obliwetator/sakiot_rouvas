use std::sync::Arc;

use serenity::client::Context;

pub mod application_command;
pub mod database;
pub mod helpers;
pub mod interactions;
pub mod lavalink;
pub mod message_component;

pub async fn get_songbird_manager(ctx: &Context) -> Arc<songbird::Songbird> {
    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.");
    manager
}
