use teloxide::macros::BotCommands;

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase", description = "Available commands:")]
pub enum Command {
    #[command(description = "Start the bot")]
    Start,
    #[command(description = "Show help message")]
    Help,
    #[command(description = "Switch to track mode")]
    Track,
    #[command(description = "Switch to manage mode")]
    Manage,
}
