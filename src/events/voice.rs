use serenity::client::Context;

pub async fn voice_server_update(
    _ctx: Context,
    _update: serenity::model::event::VoiceServerUpdateEvent,
) {
}

pub async fn voice_state_update(
    ctx: Context,
    guild_id: Option<serenity::model::id::GuildId>,
    old_state: Option<serenity::model::prelude::VoiceState>,
    new_state: serenity::model::prelude::VoiceState,
) {
}
