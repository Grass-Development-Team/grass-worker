use super::{certificates, settings};
use anyhow::{Context, ensure};
use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use uuid::Uuid;

const SETTINGS_KEY: &str = "domain_https";
const SECRET_KEY: &str = "domain-https-eab-v1";

pub struct CertificateSettings {
    pub issuer: String,
    pub eab: Value,
}
impl CertificateSettings {
    pub fn view(&self) -> Value {
        json!({"issuer":self.issuer,"zerossl_eab_configured": self.eab.get("eab_kid").is_some() && self.eab.get("eab_hmac_key").is_some()})
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            matches!(self.issuer.as_str(), "letsencrypt" | "zerossl"),
            "Certificate authority must be letsencrypt or zerossl"
        );
        if self.issuer == "zerossl" {
            ensure!(
                super::acme::external_account_key(&self.eab)?.is_some(),
                "ZeroSSL EAB credentials are required"
            );
        }
        Ok(())
    }
}
async fn stored<C: ConnectionTrait>(db: &C) -> anyhow::Result<Value> {
    Ok(settings::get_setting(db, SETTINGS_KEY)
        .await?
        .map(|s| s.value)
        .unwrap_or_else(|| json!({"issuer":"letsencrypt"})))
}
pub async fn issuer<C: ConnectionTrait>(db: &C) -> anyhow::Result<String> {
    let value = stored(db).await?;
    let issuer = value["issuer"]
        .as_str()
        .context("Invalid certificate authority setting")?;
    ensure!(
        matches!(issuer, "letsencrypt" | "zerossl"),
        "Invalid certificate authority setting"
    );
    Ok(issuer.to_owned())
}
pub async fn load<C: ConnectionTrait>(db: &C, secret: &str) -> anyhow::Result<CertificateSettings> {
    let value = stored(db).await?;
    let eab = match value.get("eab") {
        Some(value) if !value.is_null() => {
            certificates::decrypt(secret, Uuid::nil(), SECRET_KEY, value)
                .context("Certificate authority credentials could not be read")?
        }
        _ => json!({}),
    };
    let settings = CertificateSettings {
        issuer: value["issuer"]
            .as_str()
            .context("Invalid certificate authority setting")?
            .to_owned(),
        eab,
    };
    settings.validate()?;
    Ok(settings)
}
pub async fn save<C: ConnectionTrait>(
    db: &C,
    settings: &CertificateSettings,
    secret: &str,
) -> anyhow::Result<()> {
    settings.validate()?;
    let eab = certificates::encrypt(secret, Uuid::nil(), SECRET_KEY, &settings.eab)?;
    super::settings::set_json(
        db,
        SETTINGS_KEY,
        json!({"issuer":settings.issuer,"eab":eab}),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lets_encrypt_needs_no_provider_credentials_and_zerossl_needs_eab() {
        let plain = CertificateSettings {
            issuer: "letsencrypt".into(),
            eab: json!({}),
        };
        assert!(plain.validate().is_ok());
        assert!(!plain.view().to_string().contains("dns"));
        assert!(
            CertificateSettings {
                issuer: "zerossl".into(),
                eab: json!({})
            }
            .validate()
            .is_err()
        );
        let zero = CertificateSettings {
            issuer: "zerossl".into(),
            eab: json!({"eab_kid":"test-id","eab_hmac_key":"c2VjcmV0"}),
        };
        assert!(zero.validate().is_ok());
        assert_eq!(zero.view()["zerossl_eab_configured"], true);
        assert!(!zero.view().to_string().contains("test-id"));
    }
    #[test]
    fn authority_credentials_are_encrypted_and_bound_to_the_setting() {
        let secret = json!({"eab_kid":"test-id","eab_hmac_key":"c2VjcmV0"});
        let encrypted = certificates::encrypt("key", Uuid::nil(), SECRET_KEY, &secret).unwrap();
        assert!(!encrypted.to_string().contains("test-id"));
        assert_eq!(
            certificates::decrypt("key", Uuid::nil(), SECRET_KEY, &encrypted).unwrap(),
            secret
        );
        assert!(certificates::decrypt("other-key", Uuid::nil(), SECRET_KEY, &encrypted).is_err());
    }
}
