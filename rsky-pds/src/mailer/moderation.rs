use anyhow::Result;
use mailgun_rs::{EmailAddress, Mailgun, MailgunRegion, Message};
use std::env;

pub struct HtmlMailOpts {
    pub to: String,
    pub subject: String,
    pub html: String,
}

pub struct ModerationMailer {}

impl ModerationMailer {
    pub async fn send_html(opts: HtmlMailOpts) -> Result<()> {
        let HtmlMailOpts { to, subject, html } = opts;

        let recipient = EmailAddress::address(&to);
        let message = Message {
            to: vec![recipient],
            subject,
            html,
            ..Default::default()
        };

        let client = Mailgun {
            api_key: env::var("PDS_MAILGUN_API_KEY").unwrap(),
            domain: env::var("PDS_MAILGUN_DOMAIN").unwrap(),
            message,
        };
        let sender = EmailAddress::name_address(
            &env::var("PDS_MODERATION_EMAIL_FROM_NAME").unwrap(),
            &env::var("PDS_MODERATION_EMAIL_FROM_ADDRESS").unwrap(),
        );

        client.async_send(MailgunRegion::US, &sender).await?;
        Ok(())
    }
}
