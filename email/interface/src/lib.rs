pub trait Email {
    /// Configuration type for the Email provider.
    type Error: std::error::Error + Send + Sync + 'static;
    type Config: serde::de::DeserializeOwned;
    /// Creates a new instance of the Email provider with the given configuration.
    fn new(config: Self::Config) -> Result<Self, Self::Error>
    where
        Self: Sized;

    /// Sends an email with the given subject and body to the specified recipient.
    ///
    /// # Arguments
    ///
    /// * `to` - The recipient's email address.
    /// * `subject` - The subject of the email.
    /// * `body` - The body of the email.
    ///
    /// # Returns
    ///
    /// A result indicating success or failure.
    fn send_email(
        &self,
        from: &str,
        to: &str,
        subject: &str,
        body: &str,
    ) -> impl Future<Output = Result<(), Self::Error>>;
}
