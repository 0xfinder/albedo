pub mod handlers;

use crate::db::Db;
use teloxide::{dptree, prelude::*};

pub async fn start(bot: Bot, db: Db) -> color_eyre::eyre::Result<()> {
    let me = bot.get_me().await?;
    let bot_name = me.user.username.unwrap_or_default();

    let handler = Update::filter_message().endpoint(handlers::handle_message);

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![db, bot_name])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}
