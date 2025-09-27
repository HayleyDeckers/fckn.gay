use std::{path::PathBuf, sync::Arc};

use fckn_gay_dns_interface::{Dns, Record, RecordType};
use hickory_server::{
    ServerFuture,
    authority::{Catalog, ZoneType},
    proto::{
        ProtoError,
        rr::{
            Name, RData, Record as HickoryRecord, RecordType as HickoryRecordType, RrKey, rdata::A,
        },
    },
    store::file::{FileAuthority, FileConfig},
};
use serde::Deserialize;

/// configuration for the Porkbun DNS provider.
#[derive(Debug, Deserialize)]
pub struct Config {
    file_path: String,
}

/// A DNS provider implementation using Porkbun.
/// This struct holds the client for interacting with the Porkbun API.
pub struct HickoryDns {
    authority: Arc<FileAuthority>,
}

fn record_type_to_hickory(record_type: RecordType) -> HickoryRecordType {
    match record_type {
        RecordType::A => HickoryRecordType::A,
        RecordType::AAAA => HickoryRecordType::AAAA,
        RecordType::CNAME => HickoryRecordType::CNAME,
        RecordType::MX => HickoryRecordType::MX,
        RecordType::NS => HickoryRecordType::NS,
        RecordType::SRV => HickoryRecordType::SRV,
        RecordType::TXT => HickoryRecordType::TXT,
        // todo: probably not support this
        RecordType::ALIAS => HickoryRecordType::CNAME,
        RecordType::CAA => HickoryRecordType::CAA,
        RecordType::HTTPS => HickoryRecordType::HTTPS,
        RecordType::SVCB => HickoryRecordType::SVCB,
        RecordType::TLSA => HickoryRecordType::TLSA,
    }
}

impl Dns for HickoryDns {
    type Config = Config;
    type Error = ProtoError;
    type Key = RrKey;

    fn new(config: Self::Config) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        let zone_name = Name::from_utf8("is.fckn.gay").unwrap();
        let path = PathBuf::from(&config.file_path);
        if !path.exists() {
            //todo: populate with minimal zone data
            std::fs::File::create_new(path).unwrap();
        }
        let authority = Arc::new(
            FileAuthority::try_from_config(
                zone_name.clone(),
                ZoneType::Primary,
                false,
                None,
                &FileConfig {
                    zone_file_path: PathBuf::from(config.file_path),
                },
            )
            .unwrap(),
        );
        let mut catalog = Catalog::new();
        catalog.upsert(zone_name.into(), vec![authority.clone()]);
        let mut server = ServerFuture::new(catalog);
        // server.register_listener(listener, timeout);
        tokio::spawn(async move { server.block_until_done().await });
        Ok(Self { authority })
    }

    async fn add_record(&self, record: Record) -> Result<Self::Key, Self::Error> {
        if record.record_type != RecordType::A {
            panic!("only A records are supported");
        }
        let name = Name::from_utf8(record.name.clone())?;
        let key = RrKey::new(
            name.clone().into(),
            record_type_to_hickory(record.record_type),
        );
        let ip = record.content.parse::<std::net::Ipv4Addr>().unwrap();
        let hickory_record =
            HickoryRecord::from_rdata(name, record.ttl_seconds, RData::A(A::from(ip)));
        self.authority.upsert(hickory_record, 0).await;
        Ok(key)
    }

    async fn delete_record(&self, key: Self::Key) -> Result<(), Self::Error> {
        self.authority.records_mut().await.remove(&key);
        //todo: ok_or
        Ok(())
    }

    async fn list_records(&self) -> Result<Vec<(Self::Key, Record)>, Self::Error> {
        Ok(self
            .authority
            .records()
            .await
            .iter()
            .filter_map(|(key, record)| {
                //todo: what is this?
                let sig = &record.records_without_rrsigs().next()?;
                let name = sig.name();
                let rtype = sig.record_type();
                let ttl = sig.ttl();
                if let Some(a) = sig.data().as_a().cloned() {
                    Some((
                        key.clone(),
                        Record {
                            name: name.to_utf8(),
                            record_type: RecordType::A,
                            content: a.to_string(),
                            ttl_seconds: ttl,
                            //todo: fill these in
                            priority: None,
                        },
                    ))
                } else {
                    eprintln!("unsupported record type: {:?}", rtype);
                    None
                }
            })
            .collect::<Vec<_>>())
    }
}
