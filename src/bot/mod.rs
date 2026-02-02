pub mod commands;
pub mod handlers;

use teloxide::prelude::*;

pub async fn start(bot: Bot) -> color_eyre::eyre::Result<()> {
    let handler = Update::filter_message()
        .filter_command::<commands::Command>()
        .endpoint(handlers::handle_command);

    Dispatcher::builder(bot, handler)
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}
