//! Email notifications, sent straight over SMTP. Plugins have no sockets, so
//! this one is built in: a notification whose implementation is `email`.

use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::Mailbox,
    transport::smtp::{authentication::Credentials, client::Tls},
};
use omp_plugin_host::Event;
use serde::Deserialize;
use serde_json::Value;

pub const IMPLEMENTATION: &str = "email";

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct EmailSettings {
    pub server: String,
    pub port: u16,
    /// `none`, `starttls` or `tls`.
    pub security: String,
    pub username: String,
    pub password: String,
    pub from: String,
    /// Comma separated.
    pub to: String,
}

impl Default for EmailSettings {
    fn default() -> Self {
        Self {
            server: String::new(),
            port: 587,
            security: "starttls".into(),
            username: String::new(),
            password: String::new(),
            from: String::new(),
            to: String::new(),
        }
    }
}

impl EmailSettings {
    pub fn parse(settings: &Value) -> Result<Self, String> {
        let s: Self = serde_json::from_value(settings.clone())
            .map_err(|e| format!("unreadable email settings: {e}"))?;
        s.check()?;
        Ok(s)
    }

    fn check(&self) -> Result<(), String> {
        if self.server.trim().is_empty() {
            return Err("the mail server is empty".into());
        }
        if !matches!(self.security.as_str(), "none" | "starttls" | "tls") {
            return Err("security must be none, starttls or tls".into());
        }
        self.from
            .parse::<Mailbox>()
            .map_err(|e| format!("the From address is not valid: {e}"))?;
        if self.recipients()?.is_empty() {
            return Err("add at least one recipient".into());
        }
        Ok(())
    }

    fn recipients(&self) -> Result<Vec<Mailbox>, String> {
        self.to
            .split(',')
            .map(str::trim)
            .filter(|a| !a.is_empty())
            .map(|a| {
                a.parse::<Mailbox>()
                    .map_err(|e| format!("'{a}' is not an email address: {e}"))
            })
            .collect()
    }
}

pub async fn send(settings: &Value, event: &Event) -> Result<(), String> {
    let s = EmailSettings::parse(settings)?;
    let mut message = Message::builder()
        .from(s.from.parse::<Mailbox>().map_err(|e| e.to_string())?)
        .subject(format!("OpenMediaProject: {}", event.title));
    for to in s.recipients()? {
        message = message.to(to);
    }
    let body = match &event.movie {
        Some(m) => format!("{}\n\n{} ({})", event.message, m.title, m.year),
        None => event.message.clone(),
    };
    let message = message.body(body).map_err(|e| e.to_string())?;

    let builder = match s.security.as_str() {
        "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(&s.server),
        "starttls" => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&s.server),
        _ => Ok(AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(
            &s.server,
        )),
    }
    .map_err(|e| e.to_string())?
    .port(s.port)
    .timeout(Some(std::time::Duration::from_secs(20)));
    let builder = if s.security == "none" {
        builder.tls(Tls::None)
    } else {
        builder
    };
    let builder = if s.username.is_empty() {
        builder
    } else {
        builder.credentials(Credentials::new(s.username.clone(), s.password.clone()))
    };
    builder
        .build()
        .send(message)
        .await
        .map(|_| ())
        .map_err(|e| format!("sending failed: {e}"))
}
