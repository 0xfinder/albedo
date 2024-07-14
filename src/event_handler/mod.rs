use color_eyre::Report;
use color_eyre::Result;
use poise::serenity_prelude as serenity;

use crate::Data;

pub async fn event_handler(
    _ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, Data, Report>,
    _data: &Data,
) -> Result<(), Report> {
    match event {
        serenity::FullEvent::Ready { data_about_bot, .. } => {
            println!("Logged in as {}", data_about_bot.user.name);
        }
        _ => {}
    }
    Ok(())
}
