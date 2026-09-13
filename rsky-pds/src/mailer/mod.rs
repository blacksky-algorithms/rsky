pub mod moderation;

extern crate mailgun_rs;

use anyhow::{anyhow, bail, Context, Result};
use lettre::message::{Mailbox, MultiPart};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use mailgun_rs::{EmailAddress, Mailgun, MailgunRegion};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Provider {
    Smtp,
    Mailgun,
}

pub(crate) fn provider(smtp_var: &str) -> Result<Provider> {
    if env::var(smtp_var).is_ok() {
        return Ok(Provider::Smtp);
    }
    let api_key = env::var("PDS_MAILGUN_API_KEY").is_ok();
    let domain = env::var("PDS_MAILGUN_DOMAIN").is_ok();
    if api_key || domain {
        if !api_key || !domain {
            bail!("Mailgun configuration requires both PDS_MAILGUN_API_KEY and PDS_MAILGUN_DOMAIN");
        }
        return Ok(Provider::Mailgun);
    }
    bail!("No mail provider configured: set {smtp_var} or Mailgun credentials")
}

fn required(name: &str) -> Result<String> {
    env::var(name).with_context(|| format!("missing required mail configuration {name}"))
}

pub(crate) fn mailbox(address_var: &str, name_var: &str) -> Result<Mailbox> {
    let address = required(address_var)?
        .parse()
        .with_context(|| format!("invalid email address in {address_var}"))?;
    Ok(Mailbox::new(env::var(name_var).ok(), address))
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub(crate) fn render(template: &str, vars: &HashMap<String, String>) -> (String, String) {
    let token = vars.get("token").map(String::as_str).unwrap_or_default();
    let (title, intro) = match template {
        "reset password" => (
            "Password Reset Requested",
            format!(
                "We received a request to reset the password for @{}.",
                html_escape(
                    vars.get("identifier")
                        .map(String::as_str)
                        .unwrap_or("your account")
                )
            ),
        ),
        "delete account" => (
            "Account Deletion Requested",
            "Use this code to confirm account deletion.".to_owned(),
        ),
        "confirm email" => (
            "Email Confirmation",
            "Use this code to confirm your email address.".to_owned(),
        ),
        "email update" => (
            "Email Update Requested",
            "Use this code to confirm your email update.".to_owned(),
        ),
        "plc operation" => (
            "PLC Update Operation Requested",
            "Use this code to authorize the requested PLC operation.".to_owned(),
        ),
        _ => (
            "Message from your PDS",
            "Use the code below to continue.".to_owned(),
        ),
    };
    (format!("{title}\n\n{intro}\n\n{token}"), format!("<!doctype html><html><body><h1>{title}</h1><p>{intro}</p><p><code>{}</code></p></body></html>", html_escape(token)))
}

pub(crate) async fn send_smtp(
    to: &str,
    subject: &str,
    from: Mailbox,
    smtp_var: &str,
    plain: String,
    html: String,
) -> Result<()> {
    let message = Message::builder()
        .from(from)
        .to(to.parse().context("invalid recipient email address")?)
        .subject(subject)
        .multipart(MultiPart::alternative_plain_html(plain, html))
        .context("failed to build SMTP message")?;
    let smtp_url = required(smtp_var)?;
    let transport = AsyncSmtpTransport::<Tokio1Executor>::from_url(&smtp_url)
        .map_err(|_| anyhow!("invalid SMTP URL in {smtp_var}"))?
        .build();
    transport
        .send(message)
        .await
        .context("SMTP delivery failed")?;
    Ok(())
}

pub async fn send_template(opts: MailOpts) -> Result<()> {
    match provider("PDS_EMAIL_SMTP_URL")? {
        Provider::Smtp => {
            let (plain, html) = render(&opts.template, &opts.template_vars);
            send_smtp(
                &opts.to,
                &opts.subject,
                mailbox("PDS_EMAIL_FROM_ADDRESS", "PDS_EMAIL_FROM_NAME")?,
                "PDS_EMAIL_SMTP_URL",
                plain,
                html,
            )
            .await
        }
        Provider::Mailgun => {
            let message = mailgun_rs::Message {
                to: vec![EmailAddress::address(&opts.to)],
                subject: opts.subject,
                template: opts.template,
                template_vars: opts.template_vars,
                ..Default::default()
            };
            let client = Mailgun {
                api_key: required("PDS_MAILGUN_API_KEY")?,
                domain: required("PDS_MAILGUN_DOMAIN")?,
                message,
            };
            let sender = EmailAddress::name_address(
                &env::var("PDS_EMAIL_FROM_NAME").unwrap_or_default(),
                &required("PDS_EMAIL_FROM_ADDRESS")?,
            );
            client.async_send(MailgunRegion::US, &sender).await?;
            Ok(())
        }
    }
}

pub async fn send_reset_password(to: String, params: IdentifierAndTokenParams) -> Result<()> {
    let mut template_vars = HashMap::new();
    template_vars.insert("identifier".to_owned(), params.identifier);
    template_vars.insert("token".to_owned(), params.token);
    send_template(MailOpts {
        to,
        subject: "Password Reset Requested".to_owned(),
        template: "reset password".to_owned(),
        template_vars,
    })
    .await
}
async fn send_token_template(
    to: String,
    token: String,
    subject: &str,
    template: &str,
) -> Result<()> {
    let mut template_vars = HashMap::new();
    template_vars.insert("token".to_owned(), token);
    send_template(MailOpts {
        to,
        subject: subject.to_owned(),
        template: template.to_owned(),
        template_vars,
    })
    .await
}
pub async fn send_account_delete(to: String, params: TokenParam) -> Result<()> {
    send_token_template(
        to,
        params.token,
        "Account Deletion Requested",
        "delete account",
    )
    .await
}
pub async fn send_confirm_email(to: String, params: TokenParam) -> Result<()> {
    send_token_template(to, params.token, "Email Confirmation", "confirm email").await
}
pub async fn send_update_email(to: String, params: TokenParam) -> Result<()> {
    send_token_template(to, params.token, "Email Update Requested", "email update").await
}
pub async fn send_plc_operation(to: String, params: TokenParam) -> Result<()> {
    send_token_template(
        to,
        params.token,
        "PLC Update Operation Requested",
        "plc operation",
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;

    pub(super) static ENV_LOCK: Mutex<()> = Mutex::new(());
    pub(super) async fn capture_server() -> (u16, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (read, mut write) = socket.into_split();
            write.write_all(b"220 localhost\r\n").await.unwrap();
            let mut reader = BufReader::new(read);
            let mut line = String::new();
            let mut message = String::new();
            loop {
                line.clear();
                reader.read_line(&mut line).await.unwrap();
                if line.starts_with("DATA") {
                    write.write_all(b"354 go\r\n").await.unwrap();
                    loop {
                        line.clear();
                        reader.read_line(&mut line).await.unwrap();
                        if line == ".\r\n" {
                            break;
                        }
                        message.push_str(&line);
                    }
                    write.write_all(b"250 accepted\r\n").await.unwrap();
                } else if line.starts_with("QUIT") {
                    write.write_all(b"221 bye\r\n").await.unwrap();
                    break;
                } else {
                    write.write_all(b"250 localhost\r\n").await.unwrap();
                }
            }
            message
        });
        (port, task)
    }
    #[tokio::test]
    async fn smtp_renders_and_delivers_transactional_template() {
        let _guard = ENV_LOCK.lock().unwrap();
        let (port, task) = capture_server().await;
        std::env::set_var("PDS_EMAIL_SMTP_URL", format!("smtp://127.0.0.1:{port}"));
        std::env::set_var("PDS_EMAIL_FROM_ADDRESS", "noreply@example.com");
        std::env::remove_var("PDS_MAILGUN_API_KEY");
        std::env::remove_var("PDS_MAILGUN_DOMAIN");
        send_confirm_email(
            "recipient@example.com".to_owned(),
            TokenParam {
                token: "a<&token".to_owned(),
            },
        )
        .await
        .unwrap();
        let message = task.await.unwrap();
        assert!(message.contains("Subject: Email Confirmation"));
        assert!(message.contains("a&lt;&amp;token"));
        std::env::remove_var("PDS_EMAIL_SMTP_URL");
        std::env::remove_var("PDS_EMAIL_FROM_ADDRESS");
    }
    #[test]
    fn smtp_takes_precedence_and_missing_provider_is_actionable() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("PDS_EMAIL_SMTP_URL", "smtp://localhost:2525");
        assert_eq!(provider("PDS_EMAIL_SMTP_URL").unwrap(), Provider::Smtp);
        std::env::remove_var("PDS_EMAIL_SMTP_URL");
        std::env::remove_var("PDS_MAILGUN_API_KEY");
        std::env::remove_var("PDS_MAILGUN_DOMAIN");
        let error = provider("PDS_EMAIL_SMTP_URL").unwrap_err().to_string();
        assert!(error.contains("PDS_EMAIL_SMTP_URL"));
        assert!(!error.contains("2525"));
    }

    #[test]
    fn smtp_url_supports_tls_and_url_credentials() {
        let transport: AsyncSmtpTransport<Tokio1Executor> =
            AsyncSmtpTransport::<Tokio1Executor>::from_url(
                "smtps://smtp-user:smtp-password@example.com:465",
            )
            .unwrap()
            .build();
        let _ = transport;
    }
}
