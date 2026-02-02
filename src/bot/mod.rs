pub mod commands;
pub mod handlers;

use crate::db::Db;
use teloxide::{dptree, prelude::*};

pub async fn start(bot: Bot, db: Db) -> color_eyre::eyre::Result<()> {
    let handler = Update::filter_message()
        .filter_command::<commands::Command>()
        .endpoint(handlers::handle_command);

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![db])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}
