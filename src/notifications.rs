use crate::config::Config;
use log::{error, info};
use serde::Serialize;

#[derive(Serialize)]
struct DiscordPayload {
    content: String,
}

#[derive(Serialize)]
struct SlackPayload {
    text: String,
}

pub fn send_notifications(summary: &str, target_path: &str) {
    let message = format!(
        "🚨 **Infected items found on {}** 🚨\n\n```\n{}\n```",
        target_path, summary
    );
    let client = reqwest::blocking::Client::new();
    let config = Config::load();

    let discord_urls = config.discord_webhooks;
    if !discord_urls.is_empty() {
        for url in discord_urls.split(',') {
            let url = url.trim();
            if !url.is_empty() {
                let payload = DiscordPayload {
                    content: message.clone(),
                };
                match client.post(url).json(&payload).send() {
                    Ok(_) => info!("Sent Discord notification"),
                    Err(e) => error!("Failed to send Discord notification: {}", e),
                }
            }
        }
    }

    let slack_urls = config.slack_webhooks;
    if !slack_urls.is_empty() {
        for url in slack_urls.split(',') {
            let url = url.trim();
            if !url.is_empty() {
                let payload = SlackPayload {
                    text: message.clone(),
                };
                match client.post(url).json(&payload).send() {
                    Ok(_) => info!("Sent Slack notification"),
                    Err(e) => error!("Failed to send Slack notification: {}", e),
                }
            }
        }
    }
}
