use std::fmt::{Debug, Display};

use fckn_gay_dns_interface::{Dns, Record};
use porkbun_api::{
    Client, Error as ApiError,
    transport::{DefaultTransport, DefaultTransportError},
};
use serde::Deserialize;

/// configuration for the Porkbun DNS provider.
#[derive(Debug, Deserialize)]
pub struct Config {
    /// The domain name to manage with this Porkbun client.
    // todo(hayley): could be extended to support multiple domains in the future
    pub domain: String,
    /// The API key for Porkbun.
    pub api_key: String,
    /// The secret key for Porkbun.
    pub secret_key: String,
}

/// A DNS provider implementation using Porkbun.
/// This struct holds the client for interacting with the Porkbun API.
pub struct PorkbunDns {
    domain: String,
    client: Client<DefaultTransport>,
}

impl PorkbunDns {
    fn convert_record_type_to_porkbun(
        record_type: fckn_gay_dns_interface::RecordType,
    ) -> porkbun_api::DnsRecordType {
        match record_type {
            fckn_gay_dns_interface::RecordType::A => porkbun_api::DnsRecordType::A,
            fckn_gay_dns_interface::RecordType::AAAA => porkbun_api::DnsRecordType::AAAA,
            fckn_gay_dns_interface::RecordType::ALIAS => porkbun_api::DnsRecordType::ALIAS,
            fckn_gay_dns_interface::RecordType::CAA => porkbun_api::DnsRecordType::CAA,
            fckn_gay_dns_interface::RecordType::CNAME => porkbun_api::DnsRecordType::CNAME,
            fckn_gay_dns_interface::RecordType::HTTPS => porkbun_api::DnsRecordType::HTTPS,
            fckn_gay_dns_interface::RecordType::MX => porkbun_api::DnsRecordType::MX,
            fckn_gay_dns_interface::RecordType::NS => porkbun_api::DnsRecordType::NS,
            fckn_gay_dns_interface::RecordType::SRV => porkbun_api::DnsRecordType::SRV,
            fckn_gay_dns_interface::RecordType::SVCB => porkbun_api::DnsRecordType::SVCB,
            fckn_gay_dns_interface::RecordType::TLSA => porkbun_api::DnsRecordType::TLSA,
            fckn_gay_dns_interface::RecordType::TXT => porkbun_api::DnsRecordType::TXT,
        }
    }
}

pub enum Error {
    Api(ApiError<DefaultTransportError>),
    Other(String),
}

impl Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Api(e) => write!(f, "Porkbun API returned an error: {:?}", e),
            Error::Other(s) => write!(f, "{}", s),
        }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Api(_) => write!(f, "Porkbun API returned an error"),
            Error::Other(s) => write!(f, "{}", s),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Api(e) => Some(e),
            Error::Other(_) => None,
        }
    }
}

impl From<ApiError<DefaultTransportError>> for Error {
    fn from(e: ApiError<DefaultTransportError>) -> Self {
        Error::Api(e)
    }
}

impl Dns for PorkbunDns {
    type Config = Config;
    type Error = Error;
    type Key = String;

    /// Creates a new instance of the DNS provider with the given configuration.
    fn new(config: Self::Config) -> Result<Self, Self::Error> {
        let Config {
            domain,
            api_key,
            secret_key,
        } = config;
        let client = Client::new(porkbun_api::ApiKey::new(secret_key, api_key));
        Ok(PorkbunDns { client, domain })
    }

    /// Adds a DNS record to the provider.
    async fn add_record(&self, record: Record) -> Result<Self::Key, Self::Error> {
        let Some(subdomain) = record.name.strip_suffix(self.domain.as_str()) else {
            return Err(Error::Other(format!(
                "The subdomain {} does not end with the domain {}",
                record.name, self.domain
            )));
        };
        let subdomain = subdomain.strip_suffix('.').unwrap_or(subdomain);
        if !subdomain.is_ascii() {
            return Err(Error::Other(format!(
                "The domain {} contains non-ascii characters",
                record.name
            )));
        }
        let cmd = porkbun_api::CreateOrEditDnsRecord {
            subdomain: Some(subdomain),
            record_type: Self::convert_record_type_to_porkbun(record.record_type),
            content: record.content.into(),
            ttl: Some(record.ttl_seconds.into()),
            prio: record.priority.unwrap_or(0).into(),
        };
        let response = self.client.create(&self.domain, cmd).await?;
        Ok(response)
    }

    /// Deletes a DNS record from the provider.
    async fn delete_record(&self, key: Self::Key) -> Result<(), Self::Error> {
        self.client
            .delete(&self.domain, &key)
            .await
            .map_err(Error::from)
    }

    /// Lists all DNS records for a domain.
    async fn list_records(&self) -> Result<Vec<(Self::Key, Record)>, Self::Error> {
        let records = self.client.get_all(&self.domain).await?;
        Ok(records
            .into_iter()
            .map(|r| {
                (
                    r.id.clone(),
                    Record {
                        name: r.name,
                        record_type: match r.record_type {
                            porkbun_api::DnsRecordType::A => fckn_gay_dns_interface::RecordType::A,
                            porkbun_api::DnsRecordType::AAAA => {
                                fckn_gay_dns_interface::RecordType::AAAA
                            }
                            porkbun_api::DnsRecordType::ALIAS => {
                                fckn_gay_dns_interface::RecordType::ALIAS
                            }
                            porkbun_api::DnsRecordType::CAA => {
                                fckn_gay_dns_interface::RecordType::CAA
                            }
                            porkbun_api::DnsRecordType::CNAME => {
                                fckn_gay_dns_interface::RecordType::CNAME
                            }
                            porkbun_api::DnsRecordType::HTTPS => {
                                fckn_gay_dns_interface::RecordType::HTTPS
                            }
                            porkbun_api::DnsRecordType::MX => {
                                fckn_gay_dns_interface::RecordType::MX
                            }
                            porkbun_api::DnsRecordType::NS => {
                                fckn_gay_dns_interface::RecordType::NS
                            }
                            porkbun_api::DnsRecordType::SRV => {
                                fckn_gay_dns_interface::RecordType::SRV
                            }
                            porkbun_api::DnsRecordType::SVCB => {
                                fckn_gay_dns_interface::RecordType::SVCB
                            }
                            porkbun_api::DnsRecordType::TLSA => {
                                fckn_gay_dns_interface::RecordType::TLSA
                            }
                            porkbun_api::DnsRecordType::TXT => {
                                fckn_gay_dns_interface::RecordType::TXT
                            }
                        },
                        content: r.content,
                        ttl_seconds: r.ttl as u32,
                        priority: Some(r.prio as u16),
                    },
                )
            })
            .collect())
    }
}
