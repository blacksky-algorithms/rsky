use super::{mailbox, provider, send_smtp, Provider};
use anyhow::{Context, Result};
use mailgun_rs::{EmailAddress, Mailgun, MailgunRegion, Message};
use std::env;

pub struct HtmlMailOpts {
    pub to: String,
    pub subject: String,
    pub html: String,
}
pub struct ModerationMailer;

impl ModerationMailer {
    pub async fn send_html(opts: HtmlMailOpts) -> Result<()> {
        match provider("PDS_MODERATION_EMAIL_SMTP_URL")? {
            Provider::Smtp => {
                send_smtp(
                    &opts.to,
                    &opts.subject,
                    mailbox(
                        "PDS_MODERATION_EMAIL_ADDRESS",
                        "PDS_MODERATION_EMAIL_FROM_NAME",
                    )?,
                    "PDS_MODERATION_EMAIL_SMTP_URL",
                    html_to_plain(&opts.html),
                    opts.html,
                )
                .await
            }
            Provider::Mailgun => {
                let message = Message {
                    to: vec![EmailAddress::address(&opts.to)],
                    subject: opts.subject,
                    html: opts.html,
                    ..Default::default()
                };
                let client = Mailgun {
                    api_key: env::var("PDS_MAILGUN_API_KEY")
                        .context("missing required mail configuration PDS_MAILGUN_API_KEY")?,
                    domain: env::var("PDS_MAILGUN_DOMAIN")
                        .context("missing required mail configuration PDS_MAILGUN_DOMAIN")?,
                    message,
                };
                let sender = EmailAddress::name_address(
                    &env::var("PDS_MODERATION_EMAIL_FROM_NAME").unwrap_or_default(),
                    &env::var("PDS_MODERATION_EMAIL_FROM_ADDRESS").context(
                        "missing required mail configuration PDS_MODERATION_EMAIL_FROM_ADDRESS",
                    )?,
                );
                client.async_send(MailgunRegion::US, &sender).await?;
                Ok(())
            }
        }
    }
}

fn html_to_plain(html: &str) -> String {
    let mut plain = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => plain.push(ch),
            _ => {}
        }
    }
    plain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn smtp_delivers_moderation_html_without_mailgun() {
        let _guard = super::super::tests::ENV_LOCK.lock().unwrap();
        let (port, task) = super::super::tests::capture_server().await;
        std::env::set_var(
            "PDS_MODERATION_EMAIL_SMTP_URL",
            format!("smtp://127.0.0.1:{port}"),
        );
        std::env::set_var("PDS_MODERATION_EMAIL_ADDRESS", "moderation@example.com");
        std::env::remove_var("PDS_MAILGUN_API_KEY");
        std::env::remove_var("PDS_MAILGUN_DOMAIN");
        ModerationMailer::send_html(HtmlMailOpts {
            to: "recipient@example.com".to_owned(),
            subject: "Moderation notice".to_owned(),
            html: "<p>Notice <strong>body</strong></p>".to_owned(),
        })
        .await
        .unwrap();
        let message = task.await.unwrap();
        assert!(message.contains("Subject: Moderation notice"));
        assert!(message.contains("Notice <strong>body</strong>"));
        std::env::remove_var("PDS_MODERATION_EMAIL_SMTP_URL");
        std::env::remove_var("PDS_MODERATION_EMAIL_ADDRESS");
    }
}
