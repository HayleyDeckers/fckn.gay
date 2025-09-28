pub use fckn_gay_email_interface::Email as Interface;
use fckn_gay_email_lettre::LettreEmail;
use fckn_gay_email_stdout::Email as StdOutEmail;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Providers {
    Lettre,
    StdOut,
}

#[derive(Debug, serde::Deserialize)]
pub struct Config {
    provider: Option<Providers>,
    lettre: Option<<LettreEmail as Interface>::Config>,
    stdout: Option<<StdOutEmail as Interface>::Config>,
}

pub enum Email {
    Lettre(LettreEmail),
    StdOut(StdOutEmail),
}

#[derive(Debug)]
pub enum Error {
    Lettre(<LettreEmail as Interface>::Error),
    StdOut(<StdOutEmail as Interface>::Error),
    MissingConfig(&'static str),
    NoConfig,
    CantChoseProvider,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Lettre(err) => write!(f, "{err}"),
            Error::StdOut(err) => write!(f, "{err}"),
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
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Lettre(err) => err.source(),
            Error::StdOut(err) => err.source(),
            Error::MissingConfig(_) | Error::NoConfig | Error::CantChoseProvider => None,
        }
    }
}

impl Interface for Email {
    type Config = Config;
    type Error = Error;

    fn new(config: Self::Config) -> Result<Self, Self::Error> {
        if let Some(provider) = config.provider {
            match provider {
                Providers::Lettre => {
                    LettreEmail::new(config.lettre.ok_or(Error::MissingConfig("Lettre"))?)
                        .map(Email::Lettre)
                        .map_err(Error::Lettre)
                }
                Providers::StdOut => {
                    StdOutEmail::new(config.stdout.ok_or(Error::MissingConfig("StdOut"))?)
                        .map(Email::StdOut)
                        .map_err(Error::StdOut)
                }
            }
        } else {
            match (config.lettre, config.stdout) {
                (Some(lettre), None) => LettreEmail::new(lettre)
                    .map(Email::Lettre)
                    .map_err(Error::Lettre),
                (None, Some(stdout)) => StdOutEmail::new(stdout)
                    .map(Email::StdOut)
                    .map_err(Error::StdOut),
                (None, None) => Err(Error::NoConfig),
                _ => Err(Error::CantChoseProvider),
            }
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
            Email::Lettre(email) => email
                .send_email(from, to, subject, body)
                .await
                .map_err(Error::Lettre),
            Email::StdOut(email) => email
                .send_email(from, to, subject, body)
                .await
                .map_err(Error::StdOut),
        }
    }
}
