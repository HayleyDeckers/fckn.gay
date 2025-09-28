use std::{io::Read, net::SocketAddr, path::PathBuf, str::FromStr, sync::Arc, time::Duration, vec};

use fckn_gay_dns_interface::{Dns, Record, RecordType};
use hickory_server::{
    ServerFuture,
    authority::{Catalog, ZoneType},
    proto::{
        ProtoError,
        rr::{Name, RData, Record as HickoryRecord, RecordType as HickoryRecordType, RrKey, rdata},
    },
    server::{Request, RequestHandler, ResponseHandler, ResponseInfo},
    store::in_memory::InMemoryAuthority,
};
use serde::{Deserialize, Deserializer};
use tokio::{
    fs::File,
    io::{AsyncSeekExt, AsyncWriteExt},
    sync::Mutex,
};

/// configuration for the Porkbun DNS provider.
#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(deserialize_with = "deserialize_name")]
    zone_name: Name,
    file_path: String,
    tcp_addr: Option<SocketAddr>,
    udp_addr: Option<SocketAddr>,
}

fn deserialize_name<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Name, D::Error> {
    let s = String::deserialize(deserializer)?;
    Name::from_utf8(&s).map_err(serde::de::Error::custom)
}

/// A DNS provider implementation using Porkbun.
/// This struct holds the client for interacting with the Porkbun API.
pub struct HickoryDns {
    authority: Arc<InMemoryAuthority>,
    // fragile! can get corrupted if we get killed mid write
    server_file: Mutex<File>,
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

struct Wrapper(Catalog);

#[async_trait::async_trait]
impl RequestHandler for Wrapper {
    async fn handle_request<R: ResponseHandler>(
        &self,
        request: &Request,
        response_handle: R,
    ) -> ResponseInfo {
        self.0.handle_request(request, response_handle).await
    }
}

fn hickory_record_from_record(record: Record) -> HickoryRecord {
    let name = Name::from_utf8(if record.name.ends_with('.') {
        record.name.clone()
    } else {
        format!("{}.", record.name)
    })
    .unwrap();
    let rdata = match record.record_type {
        RecordType::A => RData::A(rdata::A(
            record.content.parse::<std::net::Ipv4Addr>().unwrap(),
        )),
        RecordType::AAAA => RData::AAAA(rdata::AAAA(
            record.content.parse::<std::net::Ipv6Addr>().unwrap(),
        )),
        RecordType::CNAME | RecordType::ALIAS => {
            RData::CNAME(rdata::CNAME(Name::from_utf8(record.content).unwrap()))
        }
        RecordType::MX => RData::MX(rdata::MX::new(
            record.priority.unwrap_or(0),
            Name::from_utf8(record.content).unwrap(),
        )),
        RecordType::NS => RData::NS(rdata::NS(Name::from_utf8(record.content).unwrap())),
        RecordType::TXT => RData::TXT(rdata::TXT::new(vec![record.content])),
        _ => panic!("unsupported record type"),
    };
    HickoryRecord::from_rdata(name, record.ttl_seconds, rdata)
}

fn record_from_hickory_record(r: &HickoryRecord) -> Record {
    Record {
        name: r.name().to_utf8().trim_end_matches('.').to_string(),
        record_type: match r.record_type() {
            HickoryRecordType::A => RecordType::A,
            HickoryRecordType::AAAA => RecordType::AAAA,
            HickoryRecordType::CNAME => RecordType::CNAME,
            HickoryRecordType::MX => RecordType::MX,
            HickoryRecordType::NS => RecordType::NS,
            HickoryRecordType::SRV => RecordType::SRV,
            HickoryRecordType::TXT => RecordType::TXT,
            HickoryRecordType::CAA => RecordType::CAA,
            HickoryRecordType::HTTPS => RecordType::HTTPS,
            HickoryRecordType::SVCB => RecordType::SVCB,
            HickoryRecordType::TLSA => RecordType::TLSA,
            _ => panic!("unsupported record type"),
        },
        content: match r.data() {
            RData::A(a) => a.to_string(),
            RData::AAAA(aaaa) => aaaa.to_string(),
            RData::CNAME(cname) => cname.to_utf8(),
            RData::MX(mx) => mx.exchange().to_utf8(),
            RData::NS(ns) => ns.to_utf8(),
            RData::TXT(txt) => txt
                .txt_data()
                .iter()
                .map(|s| String::from_utf8_lossy(&s).to_string())
                .collect::<String>(),
            _ => panic!("unsupported record type"),
        },
        ttl_seconds: r.ttl(),
        priority: match r.data() {
            RData::MX(mx) => Some(mx.preference()),
            _ => None,
        },
    }
}

impl Dns for HickoryDns {
    type Config = Config;
    type Error = ProtoError;
    type Key = RrKey; //not unique! needs another idx

    fn new(config: Self::Config) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        let zone_name = config.zone_name;
        let path = PathBuf::from(&config.file_path);
        let mut authority: InMemoryAuthority =
            InMemoryAuthority::empty(zone_name.clone(), ZoneType::Primary, false);
        if !path.exists() {
            //todo: populate with minimal zone data
            std::fs::File::create_new(&path).unwrap();
        };
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();
        for line in contents.lines() {
            println!("line: {}", line);
            authority.upsert_mut(
                hickory_record_from_record(Record::from_str(line).unwrap()),
                0,
            );
        }

        let authority = Arc::new(authority);

        let mut catalog = Catalog::new();
        catalog.upsert(zone_name.into(), vec![authority.clone()]);
        let mut server = ServerFuture::new(Wrapper(catalog));
        if let Some(tcp_addr) = config.tcp_addr {
            println!("binding to tcp: {}", tcp_addr);
            server
                .register_listener_std(
                    std::net::TcpListener::bind(tcp_addr).unwrap(),
                    Duration::from_secs(5),
                )
                .unwrap();
        }
        if let Some(udp_addr) = config.udp_addr {
            println!("binding to udp: {}", udp_addr);
            server
                .register_socket_std(std::net::UdpSocket::bind(udp_addr).unwrap())
                .unwrap();
        }
        // server.register_listenr(listener, timeout);
        tokio::spawn(async move {
            if let Err(e) = server.block_until_done().await {
                eprintln!("oopsie: {e}");
            }
        });
        Ok(Self {
            authority,
            server_file: Mutex::new(File::from_std(file)),
        })
    }

    async fn add_record(&self, record: Record) -> Result<Self::Key, Self::Error> {
        let name = Name::from_utf8(if record.name.ends_with('.') {
            record.name.clone()
        } else {
            format!("{}.", record.name)
        })?;
        let key = RrKey::new(
            name.clone().into(),
            record_type_to_hickory(record.record_type),
        );
        println!("adding record: {:?}", key);
        let hickory_record = hickory_record_from_record(record);
        let mut file = self.server_file.lock().await;
        if !self.authority.upsert(hickory_record, 0).await {
            panic!("failed to add record");
        }
        file.seek(std::io::SeekFrom::Start(0)).await.unwrap();
        let new_content = self
            .authority
            .records()
            .await
            .iter()
            .flat_map(|(_, r)| r.records_without_rrsigs())
            .map(record_from_hickory_record)
            .map(|r| format!("{r}\n"))
            .collect::<String>();
        println!("new content:\n{}", new_content);
        file.write_all(new_content.as_bytes()).await.unwrap();
        file.flush().await.unwrap();
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
            .flat_map(|(key, r)| r.records_without_rrsigs().zip(std::iter::repeat(key)))
            .map(|(r, key)| {
                let record = record_from_hickory_record(r);
                (key.clone(), record)
            })
            .collect::<Vec<_>>())
    }
}
