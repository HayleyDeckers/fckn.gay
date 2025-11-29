use fckn_gay_email_interface::Email;
use fckn_gay_secret::Secret;
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
    message::{MessageBuilder, header::ContentType},
    transport::smtp::authentication::Credentials,
};

#[derive(Debug, serde::Deserialize)]
pub struct Config {
    pub smtp_server: String,
    pub smtp_port: u16,
    pub username: Secret,
    pub password: Secret,
}

pub struct LettreEmail {
    smtp_transport: AsyncSmtpTransport<Tokio1Executor>,
}

impl Email for LettreEmail {
    type Config = Config;
    type Error = lettre::transport::smtp::Error;

    fn new(config: Self::Config) -> Result<Self, Self::Error> {
        let credentials = Credentials::new(
            config.username.into_exposed(),
            config.password.into_exposed(),
        );
        let smtp_transport = AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp_server)?
            .port(config.smtp_port)
            .credentials(credentials)
            .build();

        Ok(LettreEmail { smtp_transport })
    }

    async fn send_email(
        &self,
        from: &str,
        to: &str,
        subject: &str,
        body: &str,
    ) -> Result<(), Self::Error> {
        let email = MessageBuilder::new()
            .from(from.parse().expect("invalid from address"))
            .to(to.parse().expect("invalid to address"))
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body.to_string())
            .expect("msg should be valid");
        self.smtp_transport.send(email).await.map(|_| ())
    }
}
