use chrono::{DateTime, FixedOffset, Utc};
use rand::Rng;
use rand_xoshiro::rand_core::{RngCore, SeedableRng};
use rss::Channel;
use serenity::framework::standard::{macros::command, Args, CommandResult};
use serenity::model::prelude::*;
use serenity::prelude::*;
use serenity::utils::Color;
use std::error::Error;

use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::time::{self, Duration};
use url::Url;

use crate::ChannelsRSS;

#[command]
#[owners_only]
pub async fn rss(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let channels = msg.guild_id.unwrap().channels(&ctx.http).await?;
    let urls = channels
        .into_values()
        .filter(|c| c.topic.is_some() && Url::parse(c.topic.as_ref().unwrap()).is_ok());
    let urls: Vec<GuildChannel> = urls.collect();

    

    // == Data moved in the following closure
    let reqw_client = {
        let data = ctx.data.read().await;
        data.get::<ChannelsRSS>().unwrap().clone()
    };
    let guild_id = msg.guild_id.unwrap();
    let http = ctx.http.clone();
    let minutes = 15;
    // == End of data

    let _ = msg
        .channel_id
        .send_message(&ctx.http, |m| {
            m.content(format!(
                "Started listening for RSS events every {} minutes in {} channels",
                minutes,
                urls.len()
            ))
        })
        .await;

    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(minutes * 60));
        let mut random = rand_xoshiro::Xoshiro256PlusPlus::from_entropy();
        loop {
            interval.tick().await;
            let date = write_new_return_old_date(&guild_id.0.to_string())
                .await
                .unwrap();
            for i in &urls {
                let items = process_rss(i.topic.as_ref().unwrap(), date, &reqw_client).await;
                if let Err(err) = items {
                    let _ = i
                        .send_message(&http, |m| {
                            m.embed(|e| e.title("Error").description(err.to_string()))
                        })
                        .await;
                    continue;
                }
                for item in items.unwrap() {
                    let _ = i
                        .send_message(&http, |m| {
                            m.embed(|e| {
                                e.title(item.title().unwrap_or("No title"))
                                    .description(html2md::parse_html(
                                        item.description().unwrap_or("No description"),
                                    ))
                                    .url(item.link().unwrap_or(""))
                                    .color(Color::from(random.gen::<(u8, u8, u8)>()))
                            })
                        })
                        .await;
                }
            }
        }
    });
    Ok(())
}

async fn process_rss(
    url: &str,
    date: chrono::DateTime<FixedOffset>,
    client: &reqwest::Client,
) -> Result<impl Iterator<Item = rss::Item>, Box<dyn Error + Send + Sync>> {
    let content = client.get(url).send().await?.bytes().await?;
    let channel = Channel::read_from(&content[..]).expect("Not a valid rss url");
    Ok(channel.into_items().into_iter().filter(move |item| {
        DateTime::parse_from_rfc2822(item.pub_date().expect("No date on the item")).unwrap() > date
    }))
}

async fn write_new_return_old_date(
    guild_id: &str,
) -> Result<chrono::DateTime<FixedOffset>, Box<dyn Error>> {
    let path_string = format!("log/{}.txt", guild_id);
    let date = fs::read_to_string(&path_string)
        .await
        .map(|s| DateTime::parse_from_rfc2822(&s).unwrap())
        .unwrap_or_else(|_| DateTime::from(Utc::now() - chrono::Duration::days(1)));

    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path_string)
        .await?;
    let _ = file.write_all(Utc::now().to_rfc2822().as_bytes()).await;
    Ok(date)
}
