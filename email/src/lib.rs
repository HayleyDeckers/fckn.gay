//! Email interface wrapper crate with feature-gated implementations.
//!
//! Enable specific email providers via Cargo features:
//! - `dummy`: Prints emails to stdout (default, for testing)
//! - `lettre`: Real SMTP email sending

#[cfg(feature = "dummy")]
use fckn_gay_email_dummy::Email as DummyEmail;
pub use fckn_gay_email_interface::Email as Interface;
#[cfg(feature = "lettre")]
use fckn_gay_email_lettre::LettreEmail;
use serde::Deserializer;

/// A sink type that absorbs any TOML value when a feature is disabled.
/// This lets us keep config files unchanged even if you don't compile in a provider.
#[derive(Debug, Clone)]
pub struct Disabled;

impl<'de> serde::Deserialize<'de> for Disabled {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Nom nom nom, we eat the config and do nothing with it 🍽️
        let _ = serde::de::IgnoredAny::deserialize(deserializer)?;
        Ok(Disabled)
    }
}

/// Available email providers. All variants exist regardless of feature flags -
/// we check at runtime if the selected provider is actually compiled in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Providers {
    Lettre,
    Dummy,
}

impl std::fmt::Display for Providers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Providers::Lettre => write!(f, "lettre"),
            Providers::Dummy => write!(f, "dummy"),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    provider: Option<Providers>,
    #[cfg(feature = "lettre")]
    lettre: Option<<LettreEmail as Interface>::Config>,
    #[cfg(not(feature = "lettre"))]
    lettre: Option<Disabled>,
    #[cfg(feature = "dummy")]
    dummy: Option<<DummyEmail as Interface>::Config>,
    #[cfg(not(feature = "dummy"))]
    dummy: Option<Disabled>,
}

impl Config {
    /// Warns about provider configs that are present but won't be validated
    /// because the feature isn't compiled in.
    fn warn_uncompiled_providers(&self) {
        #[cfg(not(feature = "lettre"))]
        if self.lettre.is_some() {
            eprintln!(
                "⚠️  Warning: [email.lettre] config present but 'lettre' feature not compiled in - config won't be validated"
            );
        }
        #[cfg(not(feature = "dummy"))]
        if self.dummy.is_some() {
            eprintln!(
                "⚠️  Warning: [email.dummy] config present but 'dummy' feature not compiled in - config won't be validated"
            );
        }
    }

    /// Returns which provider is active, checking ALL config sections regardless of feature flags.
    /// This ensures config behavior is consistent no matter which features are compiled in.
    fn active(&self) -> Result<Providers, Error> {
        if let Some(provider) = &self.provider {
            return Ok(*provider);
        }

        // Count ALL present configs, including disabled ones
        let mut found: Option<Providers> = None;
        let mut count = 0;

        if self.lettre.is_some() {
            found = Some(Providers::Lettre);
            count += 1;
        }
        if self.dummy.is_some() {
            found = Some(Providers::Dummy);
            count += 1;
        }

        match (found, count) {
            (Some(p), 1) => Ok(p),
            (None, 0) => Err(Error::NoConfig),
            _ => Err(Error::CantChoseProvider),
        }
    }
}

pub enum Email {
    #[cfg(feature = "lettre")]
    Lettre(LettreEmail),
    #[cfg(feature = "dummy")]
    Dummy(DummyEmail),
}

#[derive(Debug)]
pub enum Error {
    #[cfg(feature = "lettre")]
    Lettre(<LettreEmail as Interface>::Error),
    #[cfg(feature = "dummy")]
    Dummy(<DummyEmail as Interface>::Error),
    MissingConfig(&'static str),
    NoConfig,
    CantChoseProvider,
    /// The selected provider was not compiled into this binary
    ProviderNotCompiled(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "lettre")]
            Error::Lettre(err) => write!(f, "{err}"),
            #[cfg(feature = "dummy")]
            Error::Dummy(err) => write!(f, "{err}"),
            Error::MissingConfig(msg) => {
                write!(f, "Missing configuration for selected provider: {msg}")
            }
            Error::NoConfig => write!(f, "No email provider configured"),
            Error::CantChoseProvider => {
                write!(
                    f,
                    "Multiple providers specified, please choose one with `provider` field or set only one in the config"
                )
            }
            Error::ProviderNotCompiled(provider) => {
                write!(
                    f,
                    "Provider '{provider}' was selected but is not compiled into this binary. \
                     Recompile with the '{provider}' feature enabled."
                )
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            #[cfg(feature = "lettre")]
            Error::Lettre(err) => err.source(),
            #[cfg(feature = "dummy")]
            Error::Dummy(err) => err.source(),
            Error::MissingConfig(_) | Error::NoConfig | Error::CantChoseProvider => None,
            Error::ProviderNotCompiled(_) => None,
        }
    }
}

impl Interface for Email {
    type Config = Config;
    type Error = Error;

    fn new(config: Self::Config) -> Result<Self, Self::Error> {
        // Warn about configs that won't be validated
        config.warn_uncompiled_providers();

        let selected = config.active()?;

        // Check if the selected provider is compiled in
        match selected {
            #[cfg(not(feature = "lettre"))]
            Providers::Lettre => {
                return Err(Error::ProviderNotCompiled("lettre".to_string()));
            }
            #[cfg(not(feature = "dummy"))]
            Providers::Dummy => return Err(Error::ProviderNotCompiled("dummy".to_string())),
            #[allow(unreachable_patterns)]
            _ => {}
        }

        match selected {
            #[cfg(feature = "lettre")]
            Providers::Lettre => {
                LettreEmail::new(config.lettre.ok_or(Error::MissingConfig("Lettre"))?)
                    .map(Email::Lettre)
                    .map_err(Error::Lettre)
            }
            #[cfg(feature = "dummy")]
            Providers::Dummy => DummyEmail::new(config.dummy.unwrap_or_default())
                .map(Email::Dummy)
                .map_err(Error::Dummy),
            #[allow(unreachable_patterns)]
            _ => unreachable!("Provider availability was already checked above"),
        }
    }

    async fn send_email(
        &self,
        from: &str,
        to: &str,
        subject: &str,
        body: &str,
    ) -> Result<(), Self::Error> {
        match self {
            #[cfg(feature = "lettre")]
            Email::Lettre(email) => email
                .send_email(from, to, subject, body)
                .await
                .map_err(Error::Lettre),
            #[cfg(feature = "dummy")]
            Email::Dummy(email) => email
                .send_email(from, to, subject, body)
                .await
                .map_err(Error::Dummy),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_with_deny_unknown_fields() {
        // This should parse fine - only known fields
        let toml_str = r#"
            provider = "dummy"
            [dummy]
        "#;
        let config: Result<Config, _> = toml::from_str(toml_str);
        assert!(config.is_ok(), "Valid config should parse: {:?}", config);
    }

    #[test]
    fn test_config_rejects_unknown_fields() {
        // This should fail - unknown field at root level
        let toml_str = r#"
            provider = "dummy"
            unknown_field = "oops"
            [dummy]
        "#;
        let config: Result<Config, _> = toml::from_str(toml_str);
        assert!(config.is_err(), "Unknown fields should be rejected");
    }

    #[test]
    fn test_ambiguous_config_errors() {
        // Multiple providers without explicit selection should error
        let toml_str = r#"
            [dummy]
            [lettre]
            smtp_server = "mail.example.com"
            smtp_port = 587
            username = "test"
            password = "test"
        "#;
        let config: Config = toml::from_str(toml_str).expect("Should parse");
        let result = config.active();
        assert!(
            matches!(result, Err(Error::CantChoseProvider)),
            "Should error on ambiguous config: {:?}",
            result
        );
    }

    #[test]
    fn test_explicit_provider_resolves_ambiguity() {
        // Explicit provider should work even with multiple configs
        let toml_str = r#"
            provider = "dummy"
            [dummy]
            [lettre]
            smtp_server = "mail.example.com"
            smtp_port = 587
            username = "test"
            password = "test"
        "#;
        let config: Config = toml::from_str(toml_str).expect("Should parse");
        let result = config.active();
        assert!(
            matches!(result, Ok(Providers::Dummy)),
            "Explicit provider should work: {:?}",
            result
        );
    }
}
