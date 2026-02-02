pub mod handlers;

use crate::db::Db;
use teloxide::{dptree, prelude::*};

pub async fn start(bot: Bot, db: Db) -> color_eyre::eyre::Result<()> {
    let me = bot.get_me().await?;
    let bot_name = me.user.username.unwrap_or_default();

    let handler = dptree::entry()
        .branch(Update::filter_message().endpoint(handlers::handle_message))
        .branch(Update::filter_callback_query().endpoint(handlers::handle_callback));

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![db, bot_name])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}
