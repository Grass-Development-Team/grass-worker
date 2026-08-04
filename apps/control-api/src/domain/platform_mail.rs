use std::collections::BTreeMap;

use sea_orm::DatabaseConnection;

use crate::{
    domain::{projects, settings, teams, users},
    infra::{
        config::mail::MailConfig,
        database::entity::{DeploymentBuildStatus, UserStatus, deployment, team},
        mail::{PlatformMessage, spawn_delivery},
    },
};

const DEFAULT_SITE_NAME: &str = "Grass Worker";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MailBranding {
    pub site_name: String,
    pub site_url: String,
}

pub async fn branding(db: &DatabaseConnection) -> anyhow::Result<MailBranding> {
    let site_name = setting_string(db, "site.name")
        .await?
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_SITE_NAME.to_owned());
    let site_url = setting_string(db, "site.url")
        .await?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("site.url is not configured"))?;
    Ok(MailBranding {
        site_name,
        site_url: site_url.trim_end_matches('/').to_owned(),
    })
}

async fn setting_string(db: &DatabaseConnection, key: &str) -> anyhow::Result<Option<String>> {
    settings::get_setting(db, key)
        .await
        .map(|setting| setting.and_then(|setting| setting.value.as_str().map(str::to_owned)))
}

pub fn invitation_message(
    branding: &MailBranding,
    recipient: &str,
    team_name: &str,
    role: &str,
    token: &str,
) -> PlatformMessage {
    let link = format!(
        "{}/invitations/accept?token={}",
        branding.site_url,
        url::form_urlencoded::byte_serialize(token.as_bytes()).collect::<String>()
    );
    PlatformMessage {
        recipient_address: recipient.to_owned(),
        recipient_name: None,
        subject: format!("Invitation to join {team_name}"),
        text: format!(
            "You have been invited to join {team_name} as {role} on {}.\n\nAccept the invitation:\n{link}\n",
            branding.site_name
        ),
    }
}

pub async fn send_invitation_best_effort(
    db: &DatabaseConnection,
    config: MailConfig,
    recipient: &str,
    team: &team::Model,
    role: &str,
    token: &str,
) {
    if !config.enabled() {
        return;
    }
    match branding(db).await {
        Ok(branding) => spawn_delivery(
            config,
            invitation_message(&branding, recipient, &team.name, role, token),
            "team.invitation",
        ),
        Err(error) => tracing::warn!(
            operation = "team.invitation",
            %error,
            "mail context could not be loaded"
        ),
    }
}

pub fn deployment_message(
    branding: &MailBranding,
    recipient_address: String,
    recipient_name: Option<String>,
    project_name: &str,
    project_id: uuid::Uuid,
    deployment: &deployment::Model,
) -> PlatformMessage {
    let status = match deployment.build_status {
        DeploymentBuildStatus::Ready => "succeeded",
        DeploymentBuildStatus::Failed => "failed",
        DeploymentBuildStatus::Canceled => "was canceled",
        _ => "finished",
    };
    let link = format!(
        "{}/projects/{project_id}/deployments/{}",
        branding.site_url, deployment.id
    );
    let failure = deployment
        .failure_message
        .as_deref()
        .filter(|message| !message.trim().is_empty())
        .map(|message| format!("\nReason: {message}\n"))
        .unwrap_or_default();
    PlatformMessage {
        recipient_address,
        recipient_name,
        subject: format!("Deployment {status}: {project_name}"),
        text: format!(
            "The {project_name} deployment {status} on {}.{failure}\nView deployment:\n{link}\n",
            branding.site_name
        ),
    }
}

fn account_link_message(
    branding: &MailBranding,
    recipient: &str,
    subject: &str,
    introduction: &str,
    path: &str,
    token: &str,
) -> PlatformMessage {
    let link = format!(
        "{}{path}?token={}",
        branding.site_url,
        url::form_urlencoded::byte_serialize(token.as_bytes()).collect::<String>()
    );
    PlatformMessage {
        recipient_address: recipient.to_owned(),
        recipient_name: None,
        subject: subject.to_owned(),
        text: format!("{introduction}\n\n{link}\n"),
    }
}

pub async fn send_email_verification_best_effort(
    db: &DatabaseConnection,
    config: MailConfig,
    recipient: &str,
    token: &str,
) {
    if !config.enabled() {
        return;
    }
    match branding(db).await {
        Ok(branding) => {
            let subject = format!("Verify your {} email", branding.site_name);
            let introduction = format!(
                "Verify this email address to finish creating your {} account.",
                branding.site_name
            );
            spawn_delivery(
                config,
                account_link_message(
                    &branding,
                    recipient,
                    &subject,
                    &introduction,
                    "/verify-email",
                    token,
                ),
                "auth.email_verification",
            );
        }
        Err(error) => tracing::warn!(
            operation = "auth.email_verification",
            %error,
            "mail context could not be loaded"
        ),
    }
}

pub async fn send_password_reset_best_effort(
    db: &DatabaseConnection,
    config: MailConfig,
    recipient: &str,
    token: &str,
) {
    if !config.enabled() {
        return;
    }
    match branding(db).await {
        Ok(branding) => {
            let subject = format!("Reset your {} password", branding.site_name);
            let introduction = format!(
                "A password reset was requested for your {} account. Ignore this message if you did not request it.",
                branding.site_name
            );
            spawn_delivery(
                config,
                account_link_message(
                    &branding,
                    recipient,
                    &subject,
                    &introduction,
                    "/reset-password",
                    token,
                ),
                "auth.password_reset",
            );
        }
        Err(error) => tracing::warn!(
            operation = "auth.password_reset",
            %error,
            "mail context could not be loaded"
        ),
    }
}

pub async fn send_mfa_code_best_effort(
    db: &DatabaseConnection,
    config: MailConfig,
    recipient: &str,
    code: &str,
) {
    if !config.enabled() {
        return;
    }
    match branding(db).await {
        Ok(branding) => spawn_delivery(
            config,
            PlatformMessage {
                recipient_address: recipient.to_owned(),
                recipient_name: None,
                subject: format!("{} verification code", branding.site_name),
                text: format!(
                    "Your {} verification code is:\n\n{code}\n\nThis code expires shortly.",
                    branding.site_name
                ),
            },
            "auth.mfa_email",
        ),
        Err(error) => tracing::warn!(
            operation = "auth.mfa_email",
            %error,
            "mail context could not be loaded"
        ),
    }
}

pub async fn send_deployment_result_best_effort(
    db: &DatabaseConnection,
    config: MailConfig,
    deployment: &deployment::Model,
) {
    if !config.enabled() {
        return;
    }
    let result = async {
        let branding = branding(db).await?;
        let project = projects::get_by_id(db, deployment.project_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("project not found"))?;
        let mut recipients = BTreeMap::<String, (String, Option<String>)>::new();

        if let Some(user_id) = deployment.triggered_by_user_id
            && let Some(user) = users::get_user_by_id(db, user_id).await?
            && matches!(user.status, UserStatus::Active)
        {
            recipients.insert(
                user.email.to_ascii_lowercase(),
                (user.email, user.display_name),
            );
        }
        for (_, user) in teams::list_members(db, deployment.team_id).await? {
            if matches!(user.status, UserStatus::Active) {
                recipients
                    .entry(user.email.to_ascii_lowercase())
                    .or_insert((user.email, user.display_name));
            }
        }

        for (_, (address, name)) in recipients {
            spawn_delivery(
                config.clone(),
                deployment_message(
                    &branding,
                    address,
                    name,
                    &project.name,
                    project.id,
                    deployment,
                ),
                "deployment.result",
            );
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;

    if let Err(error) = result {
        tracing::warn!(
            operation = "deployment.result",
            deployment_id = %deployment.id,
            %error,
            "deployment mail recipients could not be prepared"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invitation_links_encode_tokens() {
        let message = invitation_message(
            &MailBranding {
                site_name: "Acme".to_owned(),
                site_url: "https://console.example.com".to_owned(),
            },
            "user@example.com",
            "Platform",
            "member",
            "secret/value",
        );
        assert!(message.text.contains("token=secret%2Fvalue"));
        assert!(!message.text.contains("token=secret/value"));
    }
}
