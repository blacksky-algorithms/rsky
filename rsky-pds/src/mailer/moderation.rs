//! Mail the moderation service sends through `com.atproto.admin.sendEmail`:
//! over SMTP at `PDS_MODERATION_EMAIL_SMTP_URL` from
//! `PDS_MODERATION_EMAIL_ADDRESS`, as the reference does, else through the
//! account mailer's transport with the moderation sender when one is set.

use super::{mailer, Mailer};
use anyhow::{anyhow, Result};
use lettre::message::Mailbox;
use mailgun_rs::{EmailAddress, Mailgun, MailgunRegion, Message};
use std::env;

pub struct HtmlMailOpts {
    pub to: String,
    pub subject: String,
    pub html: String,
}

pub struct ModerationMailer {}

fn setting(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

/// The moderation sender: the reference's single address setting, or the
/// name and address pair.
pub fn moderation_sender() -> Result<Option<Mailbox>> {
    let from = match (
        setting("PDS_MODERATION_EMAIL_ADDRESS"),
        setting("PDS_MODERATION_EMAIL_FROM_ADDRESS"),
    ) {
        (Some(address), _) => address,
        (None, Some(address)) => match setting("PDS_MODERATION_EMAIL_FROM_NAME") {
            Some(name) => format!("{name} <{address}>"),
            None => address,
        },
        (None, None) => return Ok(None),
    };
    from.parse()
        .map(Some)
        .map_err(|err| anyhow!("moderation sender {from:?} is not a mailbox: {err}"))
}

impl ModerationMailer {
    pub async fn send_html(opts: HtmlMailOpts) -> Result<()> {
        let HtmlMailOpts { to, subject, html } = opts;
        let sender = moderation_sender()?;
        if let Some(url) = setting("PDS_MODERATION_EMAIL_SMTP_URL") {
            let from = sender.ok_or_else(|| {
                anyhow!(
                    "PDS_MODERATION_EMAIL_ADDRESS is required with PDS_MODERATION_EMAIL_SMTP_URL"
                )
            })?;
            return Mailer::smtp(&url, &from.to_string())?
                .send_html(&to, &subject, &html, None)
                .await;
        }
        mailer()
            .send_html(&to, &subject, &html, sender.as_ref())
            .await
    }
}

pub(super) async fn send_through_mailgun(to: &str, subject: &str, html: &str) -> Result<()> {
    let recipient = EmailAddress::address(to);
    let message = Message {
        to: vec![recipient],
        subject: subject.to_owned(),
        html: html.to_owned(),
        ..Default::default()
    };
    let client = Mailgun {
        api_key: env::var("PDS_MAILGUN_API_KEY").unwrap_or_default(),
        domain: env::var("PDS_MAILGUN_DOMAIN").unwrap_or_default(),
        message,
    };
    let sender = EmailAddress::name_address(
        &env::var("PDS_MODERATION_EMAIL_FROM_NAME").unwrap_or_default(),
        &env::var("PDS_MODERATION_EMAIL_FROM_ADDRESS").unwrap_or_default(),
    );
    client.async_send(MailgunRegion::US, &sender).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::tests::{smtp_server, ENV_LOCK};
    use super::*;

    #[tokio::test]
    async fn moderation_mail_uses_its_own_smtp_settings() {
        let _env = ENV_LOCK.lock().await;
        for name in [
            "PDS_MODERATION_EMAIL_SMTP_URL",
            "PDS_MODERATION_EMAIL_ADDRESS",
            "PDS_MODERATION_EMAIL_FROM_ADDRESS",
            "PDS_MODERATION_EMAIL_FROM_NAME",
            "PDS_EMAIL_SMTP_URL",
        ] {
            std::env::remove_var(name);
        }
        std::env::set_var("PDS_MAILGUN_API_KEY", "");
        assert!(moderation_sender().unwrap().is_none());
        std::env::set_var("PDS_MODERATION_EMAIL_FROM_ADDRESS", "mod@example.test");
        assert_eq!(
            moderation_sender().unwrap().unwrap().to_string(),
            "mod@example.test"
        );
        std::env::set_var("PDS_MODERATION_EMAIL_FROM_NAME", "Moderation");
        assert_eq!(
            moderation_sender().unwrap().unwrap().to_string(),
            "Moderation <mod@example.test>"
        );
        std::env::set_var("PDS_MODERATION_EMAIL_ADDRESS", "not a mailbox");
        assert!(moderation_sender().is_err());
        // an SMTP URL without a sender is a configuration error
        std::env::set_var(
            "PDS_MODERATION_EMAIL_SMTP_URL",
            "smtp://127.0.0.1:1?ignoreTLS=true",
        );
        std::env::remove_var("PDS_MODERATION_EMAIL_ADDRESS");
        std::env::remove_var("PDS_MODERATION_EMAIL_FROM_ADDRESS");
        let err = ModerationMailer::send_html(HtmlMailOpts {
            to: "a@example.test".into(),
            subject: "s".into(),
            html: "<p>x</p>".into(),
        })
        .await
        .unwrap_err();
        assert!(err.to_string().contains("PDS_MODERATION_EMAIL_ADDRESS"));
        // with its own server the notice goes out from the moderation sender
        let (port, captured) = smtp_server(false);
        std::env::set_var(
            "PDS_MODERATION_EMAIL_SMTP_URL",
            format!("smtp://127.0.0.1:{port}?ignoreTLS=true"),
        );
        std::env::set_var("PDS_MODERATION_EMAIL_ADDRESS", "mod@example.test");
        ModerationMailer::send_html(HtmlMailOpts {
            to: "bob@example.test".into(),
            subject: "Notice".into(),
            html: "<p>Hello</p>".into(),
        })
        .await
        .unwrap();
        let mut captured_message = None;
        for _ in 0..200 {
            if let Some(message) = captured.lock().unwrap().take() {
                captured_message = Some(message);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let message = captured_message.expect("the moderation mail was delivered");
        assert!(message
            .commands
            .iter()
            .any(|c| c == "MAIL FROM:<mod@example.test>"));
        assert!(message.data.contains("Subject: Notice"));
        // without its own server the account mailer carries it (logged here)
        std::env::remove_var("PDS_MODERATION_EMAIL_SMTP_URL");
        ModerationMailer::send_html(HtmlMailOpts {
            to: "bob@example.test".into(),
            subject: "Notice".into(),
            html: "<p>Hello</p>".into(),
        })
        .await
        .unwrap();
        std::env::remove_var("PDS_MODERATION_EMAIL_ADDRESS");
        std::env::remove_var("PDS_MODERATION_EMAIL_FROM_NAME");
    }
}
