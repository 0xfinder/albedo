mod commands;
mod event_handler;

use dotenv::dotenv;
use std::env;

use color_eyre::eyre::{Report, Result};
use poise::serenity_prelude::{self as serenity, ActivityData, GatewayIntents};

struct Data {} // User data, which is stored and accessible in all command invocations
type Context<'a> = poise::Context<'a, Data, Report>;

#[tokio::main]
async fn main() -> Result<()> {
    // load env vars
    dotenv().expect("failed to load .env file");

    // install color_eyre for error handling
    color_eyre::install()?;

    // client configuration
    let token = env::var("DISCORD_TOKEN").expect("expected discord_token");
    let intents = GatewayIntents::non_privileged()
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    let opts = poise::FrameworkOptions {
        commands: vec![
            commands::age::age(),
            commands::avatar::avatar(),
            commands::help::help(),
            commands::kelly::kelly(),
        ],
        event_handler: |ctx, event, framework, data| {
            Box::pin(async move {
                crate::event_handler::event_handler(ctx, event, framework, data).await?;
                Ok(())
            })
        },
        prefix_options: poise::PrefixFrameworkOptions {
            prefix: Some("~".into()),
            ..Default::default()
        },
        ..Default::default()
    };

    let framework = poise::Framework::builder()
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;

                ctx.set_activity(Some(ActivityData::custom("AAAAAAAAAAAAAAAAAAAa")));

                Ok(Data {})
            })
        })
        .options(opts)
        .build();

    // start client
    let mut client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await
        .expect("error creating client");

    if let Err(why) = client.start().await {
        println!("client error: {:?}", why);
    }

    Ok(())
}
