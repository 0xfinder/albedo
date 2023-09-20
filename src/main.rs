mod commands;

use std::collections::HashSet;
use std::env;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use serenity::async_trait;
use serenity::collector::MessageCollectorBuilder;
use serenity::framework::standard::macros::group;
use serenity::framework::StandardFramework;
use serenity::futures::StreamExt;
use serenity::http::Http;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;

use crate::commands::math::*;

#[derive(Deserialize, Serialize)]
struct Accounts<'a> {
    #[serde(borrow)]
    accounts: Box<Account<'a>>,
}

#[derive(Deserialize, Serialize)]
struct Account<'a> {
    userid: u64,
    discriminator: &'a str,
    token: &'a str,
    guild: u64,
}

#[group]
#[commands(multiply)]
struct General;

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        if msg.content == "~ping" {
            if let Err(why) = msg.channel_id.say(&ctx.http, "Pong!").await {
                println!("error sending message: {:?}", why);
            }
        }

        if msg.content == "!messageme" {
            let dm = msg.author.dm(&ctx, |m| m.content("Hello!")).await;

            if let Err(why) = dm {
                println!("Error when direct messaging user: {:?}", why);
            }
        }

        // check gold per account
        if msg.content == ".g" {
            let collector = MessageCollectorBuilder::new(ctx)
                .author_id(571027211407196161 as u64)
                .collect_limit(1u32)
                .timeout(Duration::from_secs(5))
                .build();

            let collected: Vec<_> = collector.collect().await;
            for message in collected {
                println!("{:#?}", message)
            }
        }
    }

    // send when bot is rdy
    async fn ready(&self, _: Context, ready: Ready) {
        println!(
            "{}#{} is connected!",
            ready.user.name, ready.user.discriminator
        );
    }
}

#[tokio::main]
async fn main() {
    // load env vars
    dotenv::dotenv().expect("failed to load .env file");

    let token = env::var("DISCORD_TOKEN").expect("expected a token");

    let http = Http::new(&token);

    // We will fetch your bot's owners and id
    let (owners, _bot_id) = match http.get_current_application_info().await {
        Ok(info) => {
            let mut owners = HashSet::new();
            owners.insert(info.owner.id);

            (owners, info.id)
        }
        Err(why) => panic!("Could not access application info: {:?}", why),
    };

    // Create the framework
    let framework = StandardFramework::new()
        .configure(|c| c.owners(owners).prefix("~"))
        .group(&GENERAL_GROUP);

    // settings
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    // start client
    let mut client = Client::builder(&token, intents)
        .framework(framework)
        .event_handler(Handler)
        .await
        .expect("error creating client");

    if let Err(why) = client.start().await {
        println!("client error: {:?}", why);
    }
}
