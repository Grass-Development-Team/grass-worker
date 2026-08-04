use std::{env, fmt};

use grass_config::{ConfigError, overlay_string};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MailMode {
    #[default]
    None,
    Local,
    Smtp,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SmtpSecurity {
    None,
    #[default]
    StartTls,
    Tls,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct MailConfig {
    #[serde(default)]
    pub mode: MailMode,
    #[serde(default = "default_from_address")]
    pub from_address: String,
    #[serde(default = "default_from_name")]
    pub from_name: String,
    #[serde(default = "default_sendmail_command")]
    pub sendmail_command: String,
    #[serde(default)]
    pub smtp_host: String,
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
    #[serde(default)]
    pub smtp_security: SmtpSecurity,
    #[serde(default)]
    pub smtp_username: String,
    #[serde(default)]
    pub smtp_password: String,
}

impl fmt::Debug for MailConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MailConfig")
            .field("mode", &self.mode)
            .field("from_address", &self.from_address)
            .field("from_name", &self.from_name)
            .field("sendmail_command", &self.sendmail_command)
            .field("smtp_host", &self.smtp_host)
            .field("smtp_port", &self.smtp_port)
            .field("smtp_security", &self.smtp_security)
            .field("smtp_username", &self.smtp_username)
            .field(
                "smtp_password",
                &(!self.smtp_password.is_empty()).then_some("[REDACTED]"),
            )
            .finish()
    }
}

impl Default for MailConfig {
    fn default() -> Self {
        Self {
            mode: MailMode::None,
            from_address: default_from_address(),
            from_name: default_from_name(),
            sendmail_command: default_sendmail_command(),
            smtp_host: String::new(),
            smtp_port: default_smtp_port(),
            smtp_security: SmtpSecurity::StartTls,
            smtp_username: String::new(),
            smtp_password: String::new(),
        }
    }
}

fn default_from_address() -> String {
    "noreply@localhost".to_owned()
}

fn default_from_name() -> String {
    "Grass Worker".to_owned()
}

fn default_sendmail_command() -> String {
    "/usr/sbin/sendmail".to_owned()
}

const fn default_smtp_port() -> u16 {
    587
}

impl MailMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Local => "local",
            Self::Smtp => "smtp",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "local" => Some(Self::Local),
            "smtp" => Some(Self::Smtp),
            _ => None,
        }
    }
}

impl SmtpSecurity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::StartTls => "starttls",
            Self::Tls => "tls",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "starttls" => Some(Self::StartTls),
            "tls" => Some(Self::Tls),
            _ => None,
        }
    }
}

impl MailConfig {
    pub fn enabled(&self) -> bool {
        !matches!(self.mode, MailMode::None)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.from_address.trim().is_empty() {
            return Err("mail sender address is required");
        }
        self.from_address
            .parse::<lettre::Address>()
            .map_err(|_| "mail sender address is invalid")?;

        match self.mode {
            MailMode::None => Ok(()),
            MailMode::Local => {
                let command = self.sendmail_command.trim();
                if command.is_empty() || !std::path::Path::new(command).is_absolute() {
                    return Err("local mail command must be an absolute path");
                }
                Ok(())
            }
            MailMode::Smtp => {
                if self.smtp_host.trim().is_empty() {
                    return Err("SMTP host is required");
                }
                if self.smtp_port == 0 {
                    return Err("SMTP port must be greater than zero");
                }
                if self.smtp_username.is_empty() != self.smtp_password.is_empty() {
                    return Err("SMTP username and password must be configured together");
                }
                Ok(())
            }
        }
    }
}

pub(super) fn apply_env(config: &mut MailConfig) -> Result<(), ConfigError> {
    if let Ok(value) = env::var("GWAPI_MAIL_MODE") {
        config.mode = MailMode::parse(&value).ok_or_else(|| {
            ConfigError::Invalid("GWAPI_MAIL_MODE must be none, local or smtp".to_owned())
        })?;
    }
    overlay_string("GWAPI_MAIL_FROM_ADDRESS", &mut config.from_address);
    overlay_string("GWAPI_MAIL_FROM_NAME", &mut config.from_name);
    overlay_string("GWAPI_MAIL_SENDMAIL_COMMAND", &mut config.sendmail_command);
    overlay_string("GWAPI_MAIL_SMTP_HOST", &mut config.smtp_host);
    if let Ok(value) = env::var("GWAPI_MAIL_SMTP_PORT") {
        config.smtp_port = value.parse().map_err(|source| ConfigError::Env {
            name: "GWAPI_MAIL_SMTP_PORT",
            source: Box::new(source),
        })?;
    }
    if let Ok(value) = env::var("GWAPI_MAIL_SMTP_SECURITY") {
        config.smtp_security = SmtpSecurity::parse(&value).ok_or_else(|| {
            ConfigError::Invalid(
                "GWAPI_MAIL_SMTP_SECURITY must be none, starttls or tls".to_owned(),
            )
        })?;
    }
    overlay_string("GWAPI_MAIL_SMTP_USERNAME", &mut config.smtp_username);
    overlay_string("GWAPI_MAIL_SMTP_PASSWORD", &mut config.smtp_password);
    config
        .validate()
        .map_err(|message| ConfigError::Invalid(message.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_mail_is_valid_without_a_transport() {
        let config = MailConfig::default();
        assert_eq!(config.mode, MailMode::None);
        assert!(!config.enabled());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn local_and_smtp_modes_validate_transport_details() {
        let mut local = MailConfig {
            mode: MailMode::Local,
            ..Default::default()
        };
        assert!(local.validate().is_ok());
        local.sendmail_command = "sendmail".to_owned();
        assert_eq!(
            local.validate(),
            Err("local mail command must be an absolute path")
        );

        let mut smtp = MailConfig {
            mode: MailMode::Smtp,
            ..Default::default()
        };
        assert_eq!(smtp.validate(), Err("SMTP host is required"));
        smtp.smtp_host = "smtp.example.com".to_owned();
        smtp.smtp_username = "user".to_owned();
        assert_eq!(
            smtp.validate(),
            Err("SMTP username and password must be configured together")
        );
        smtp.smtp_password = "secret".to_owned();
        assert!(smtp.validate().is_ok());
        assert!(!format!("{smtp:?}").contains("secret"));
    }
}
