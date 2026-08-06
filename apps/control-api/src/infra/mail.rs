use lettre::{
    AsyncSendmailTransport, AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::Mailbox, transport::smtp::authentication::Credentials,
};

use crate::infra::config::mail::{MailConfig, MailMode, SmtpSecurity};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformMessage {
    pub recipient_address: String,
    pub recipient_name: Option<String>,
    pub subject: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryOutcome {
    Disabled,
    Sent,
}

#[derive(Debug, thiserror::Error)]
pub enum MailError {
    #[error("mail configuration is invalid: {0}")]
    Configuration(String),
    #[error("mail message is invalid: {0}")]
    Message(String),
    #[error("local mail delivery failed")]
    Local,
    #[error("SMTP delivery failed")]
    Smtp,
}

fn mailbox(name: Option<&str>, address: &str) -> Result<Mailbox, MailError> {
    let address = address
        .parse()
        .map_err(|_| MailError::Message("mailbox address is invalid".to_owned()))?;
    Ok(Mailbox::new(
        name.map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned),
        address,
    ))
}

pub fn build_message(config: &MailConfig, message: &PlatformMessage) -> Result<Message, MailError> {
    config
        .validate()
        .map_err(|message| MailError::Configuration(message.to_owned()))?;
    if message.subject.trim().is_empty() || message.text.trim().is_empty() {
        return Err(MailError::Message(
            "mail subject and text body are required".to_owned(),
        ));
    }

    Message::builder()
        .from(mailbox(Some(&config.from_name), &config.from_address)?)
        .to(mailbox(
            message.recipient_name.as_deref(),
            &message.recipient_address,
        )?)
        .subject(message.subject.trim())
        .body(message.text.clone())
        .map_err(|error| MailError::Message(error.to_string()))
}

pub async fn deliver(
    config: &MailConfig,
    message: &PlatformMessage,
) -> Result<DeliveryOutcome, MailError> {
    if matches!(config.mode, MailMode::None) {
        return Ok(DeliveryOutcome::Disabled);
    }

    let message = build_message(config, message)?;
    match config.mode {
        MailMode::None => Ok(DeliveryOutcome::Disabled),
        MailMode::Local => {
            AsyncSendmailTransport::<Tokio1Executor>::new_with_command(
                config.sendmail_command.clone(),
            )
            .send(message)
            .await
            .map_err(|_| MailError::Local)?;
            Ok(DeliveryOutcome::Sent)
        }
        MailMode::Smtp => {
            let builder = match config.smtp_security {
                SmtpSecurity::None => {
                    AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(config.smtp_host.trim())
                }
                SmtpSecurity::StartTls => {
                    AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(config.smtp_host.trim())
                        .map_err(|error| MailError::Configuration(error.to_string()))?
                }
                SmtpSecurity::Tls => {
                    AsyncSmtpTransport::<Tokio1Executor>::relay(config.smtp_host.trim())
                        .map_err(|error| MailError::Configuration(error.to_string()))?
                }
            };
            let mut builder = builder.port(config.smtp_port);
            if !config.smtp_username.is_empty() {
                builder = builder.credentials(Credentials::new(
                    config.smtp_username.clone(),
                    config.smtp_password.clone(),
                ));
            }
            builder
                .build()
                .send(message)
                .await
                .map_err(|_| MailError::Smtp)?;
            Ok(DeliveryOutcome::Sent)
        }
    }
}

/// Delivers mail after the caller's core transaction has completed. The
/// recipient is represented only by a stable hash in diagnostics.
pub fn spawn_delivery(config: MailConfig, message: PlatformMessage, operation: &'static str) {
    tokio::spawn(async move {
        let recipient_hash = grass_token::hash_token(&message.recipient_address);
        match deliver(&config, &message).await {
            Ok(DeliveryOutcome::Disabled) => {
                tracing::debug!(operation, %recipient_hash, "mail delivery is disabled");
            }
            Ok(DeliveryOutcome::Sent) => {
                tracing::info!(operation, %recipient_hash, "mail delivered");
            }
            Err(error) => {
                tracing::warn!(operation, %recipient_hash, %error, "mail delivery failed");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message() -> PlatformMessage {
        PlatformMessage {
            recipient_address: "user@example.com".to_owned(),
            recipient_name: Some("Example User".to_owned()),
            subject: "Deployment ready".to_owned(),
            text: "The deployment is ready.".to_owned(),
        }
    }

    #[tokio::test]
    async fn disabled_transport_is_an_explicit_success_without_building_a_message() {
        let config = MailConfig::default();
        let mut invalid = message();
        invalid.recipient_address = "not an address".to_owned();
        assert_eq!(
            deliver(&config, &invalid).await.unwrap(),
            DeliveryOutcome::Disabled
        );
    }

    #[test]
    fn message_builder_validates_sender_recipient_and_content() {
        let mut config = MailConfig {
            mode: MailMode::Local,
            ..Default::default()
        };
        assert!(build_message(&config, &message()).is_ok());

        config.from_address = "invalid".to_owned();
        assert!(matches!(
            build_message(&config, &message()),
            Err(MailError::Configuration(_))
        ));

        let mut invalid = message();
        invalid.subject.clear();
        assert!(matches!(
            build_message(
                &MailConfig {
                    mode: MailMode::Local,
                    ..Default::default()
                },
                &invalid
            ),
            Err(MailError::Message(_))
        ));
    }
}
