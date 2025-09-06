use fckn_gay_email_interface::Email as Interface;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {}

pub struct Email;

impl Interface for Email {
    type Config = Config;
    type Error = lettre::error::Error;

    fn new(_: Self::Config) -> Result<Self, Self::Error> {
        Ok(Email)
    }

    async fn send_email(
        &self,
        from: &str,
        to: &str,
        subject: &str,
        body: &str,
    ) -> Result<(), Self::Error> {
        println!(
            "Sending email:\nFrom: <{}>\nTo: <{}>\nSubject: \"{}\"\n----\n{}\n----",
            from, to, subject, body
        );
        Ok(())
    }
}
