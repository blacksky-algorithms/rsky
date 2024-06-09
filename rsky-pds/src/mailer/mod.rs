pub mod moderation;

extern crate mailgun_rs;

use anyhow::Result;
use mailgun_rs::{EmailAddress, Mailgun, MailgunRegion, Message};
use std::collections::HashMap;
use std::env;

pub struct MailOpts {
    pub to: String,
    pub subject: String,
    pub template: String,
    pub template_vars: HashMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentifierAndTokenParams {
    pub identifier: String,
    pub token: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TokenParam {
    pub token: String,
}

pub async fn send_template(opts: MailOpts) -> Result<()> {
    let MailOpts {
        to,
        subject,
        template,
        template_vars,
    } = opts;

    let recipient = EmailAddress::address(&to);
    let message = Message {
        to: vec![recipient],
        subject,
        template,
        template_vars,
        ..Default::default()
    };

    let client = Mailgun {
        api_key: env::var("PDS_MAILGUN_API_KEY").unwrap(),
        domain: env::var("PDS_MAILGUN_DOMAIN").unwrap(),
        message,
    };
    let sender = EmailAddress::name_address(
        &env::var("PDS_EMAIL_FROM_NAME").unwrap(),
        &env::var("PDS_EMAIL_FROM_ADDRESS").unwrap(),
    );

    client.async_send(MailgunRegion::US, &sender).await?;
    Ok(())
}

pub async fn send_reset_password(to: String, params: IdentifierAndTokenParams) -> Result<()> {
    let mut template_vars = HashMap::new();
    template_vars.insert("identifier".to_string(), params.identifier);
    template_vars.insert("token".to_string(), params.token);
    send_template(MailOpts {
        to,
        subject: "Password Reset Requested".to_string(),
        template: "reset password".to_string(),
        template_vars,
    })
    .await
}

pub async fn send_account_delete(to: String, params: TokenParam) -> Result<()> {
    let mut template_vars = HashMap::new();
    template_vars.insert("token".to_string(), params.token);
    send_template(MailOpts {
        to,
        subject: "Account Deletion Requested".to_string(),
        template: "delete account".to_string(),
        template_vars,
    })
    .await
}

pub async fn send_confirm_email(to: String, params: TokenParam) -> Result<()> {
    let mut template_vars = HashMap::new();
    template_vars.insert("token".to_string(), params.token);
    send_template(MailOpts {
        to,
        subject: "Email Confirmation".to_string(),
        template: "confirm email".to_string(),
        template_vars,
    })
    .await
}

pub async fn send_update_email(to: String, params: TokenParam) -> Result<()> {
    let mut template_vars = HashMap::new();
    template_vars.insert("token".to_string(), params.token);
    send_template(MailOpts {
        to,
        subject: "Email Update Requested".to_string(),
        template: "email update".to_string(),
        template_vars,
    })
    .await
}

pub async fn send_plc_operation(to: String, params: TokenParam) -> Result<()> {
    let mut template_vars = HashMap::new();
    template_vars.insert("token".to_string(), params.token);
    send_template(MailOpts {
        to,
        subject: "PLC Update Operation Requested".to_string(),
        template: "plc operation".to_string(),
        template_vars,
    })
    .await
}
