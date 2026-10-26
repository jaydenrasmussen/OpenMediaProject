//! Sends every event as a message from a Telegram bot.
//! API: <https://core.telegram.org/bots/api#sendmessage>

wit_bindgen::generate!({
    path: "../../wit",
    world: "notification-plugin",
});

use exports::omp::plugin::notification::{Event, Guest};
use omp::plugin::host::{self, HttpRequest};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct Settings {
    bot_token: String,
    chat_id: String,
}

struct Telegram;

impl Guest for Telegram {
    fn settings_schema() -> String {
        r#"{
            "type": "object",
            "required": ["bot_token", "chat_id"],
            "properties": {
                "bot_token": {
                    "type": "string",
                    "title": "Bot token",
                    "format": "password",
                    "description": "From @BotFather."
                },
                "chat_id": {
                    "type": "string",
                    "title": "Chat ID",
                    "description": "Your user, group or channel ID (use @username for a public channel)."
                }
            }
        }"#
        .to_owned()
    }

    fn send(event: Event, settings_json: String) -> Result<(), String> {
        let s: Settings =
            serde_json::from_str(&settings_json).map_err(|e| format!("bad settings: {e}"))?;
        let token = s.bot_token.trim();
        if token.is_empty() {
            return Err("the bot token is empty".into());
        }
        if s.chat_id.trim().is_empty() {
            return Err("the chat ID is empty".into());
        }

        let mut text = format!(
            "*{}*\n{}",
            escape_markdown_v2(&event.title),
            escape_markdown_v2(&event.message)
        );
        if let Some(movie) = &event.movie {
            text.push_str(&format!(
                "\n{} \\({}\\)",
                escape_markdown_v2(&movie.title),
                escape_markdown_v2(&movie.year.to_string())
            ));
        }

        let body = json!({
            "chat_id": s.chat_id,
            "text": text,
            "parse_mode": "MarkdownV2",
            "disable_notification": event.kind == "test",
        })
        .to_string();

        let response = host::http(&HttpRequest {
            method: "POST".to_owned(),
            url: format!("https://api.telegram.org/bot{token}/sendMessage"),
            headers: vec![("content-type".to_owned(), "application/json".to_owned())],
            body: Some(body),
        })?;
        if (200..300).contains(&response.status) {
            Ok(())
        } else {
            Err(format!(
                "Telegram answered {}: {}",
                response.status, response.body
            ))
        }
    }
}

/// Escapes the characters MarkdownV2 treats specially, so an event title or message
/// containing e.g. a release name with dots and dashes does not break formatting or
/// make sendMessage reject the whole request.
/// <https://core.telegram.org/bots/api#markdownv2-style>
fn escape_markdown_v2(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(
            c,
            '_' | '*'
                | '['
                | ']'
                | '('
                | ')'
                | '~'
                | '`'
                | '>'
                | '#'
                | '+'
                | '-'
                | '='
                | '|'
                | '{'
                | '}'
                | '.'
                | '!'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

export!(Telegram);
