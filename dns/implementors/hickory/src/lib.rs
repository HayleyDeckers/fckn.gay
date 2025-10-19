use std::{
    collections::BTreeMap,
    io::Read,
    net::SocketAddr,
    num::NonZeroU64,
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
    vec,
};

use fckn_gay_dns_interface::{Dns, Record, RecordType};
use hickory_server::{
    ServerFuture,
    authority::{Catalog, ZoneType},
    proto::{
        ProtoError,
        rr::{Name, RData, Record as HickoryRecord, RecordType as HickoryRecordType, rdata},
    },
    server::{Request, RequestHandler, ResponseHandler, ResponseInfo},
    store::in_memory::InMemoryAuthority,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeSeq};
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
    server_file: Mutex<FileBacked<Database>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Database {
    #[serde(
        default,
        serialize_with = "serialize_records",
        deserialize_with = "deserialize_records"
    )]
    records: BTreeMap<NonZeroU64, HickoryRecord>,
}

fn serialize_records<S: Serializer>(
    records: &BTreeMap<NonZeroU64, HickoryRecord>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut seq = serializer.serialize_seq(Some(records.len()))?;
    for (id, record) in records.iter() {
        seq.serialize_element(&(id.get(), record))?;
    }
    seq.end()
}

fn deserialize_records<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<NonZeroU64, HickoryRecord>, D::Error> {
    let seq = Vec::<(NonZeroU64, HickoryRecord)>::deserialize(deserializer)?;
    let map = seq.into_iter().collect();
    Ok(map)
}
impl Database {
    fn add_record(&mut self, record: HickoryRecord) -> u64 {
        let id = self
            .records
            .last_key_value()
            .map(|(id, _)| id.get())
            .unwrap_or_else(|| 0);
        let id = NonZeroU64::new(id + 1).expect("record id overflow");
        self.records.insert(id, record);
        id.get()
    }
    fn delete_record(&mut self, id: u64) -> Option<HickoryRecord> {
        self.records.remove(&NonZeroU64::new(id).unwrap())
    }
}

struct FileBacked<T: Serialize> {
    file: File,
    data: T,
}

impl<T: Serialize> Deref for FileBacked<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<T: Serialize> DerefMut for FileBacked<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl<T: Serialize> FileBacked<T> {
    // fragile! can get corrupted if we get killed mid write
    async fn save(&mut self) -> Result<(), std::io::Error> {
        self.file.seek(std::io::SeekFrom::Start(0)).await?;
        self.file
            .write_all(
                toml::to_string(&self.data)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )
            .await?;
        self.file.flush().await?;
        Ok(())
    }
}

impl<T: Serialize + for<'de> Deserialize<'de>> FileBacked<T> {
    fn from_file(path: &Path) -> Self {
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();
        let data = toml::from_str(&contents).unwrap();
        let file = File::from_std(file);
        Self { file, data }
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
                .map(|s| String::from_utf8_lossy(s))
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
    type Key = u64;

    fn new(config: Self::Config) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        let zone_name = config.zone_name;
        let path = PathBuf::from(&config.file_path);
        let file: FileBacked<Database> = FileBacked::from_file(&path);
        let mut authority: InMemoryAuthority =
            InMemoryAuthority::empty(zone_name.clone(), ZoneType::Primary, false);
        for record in file.records.values().cloned() {
            if !authority.upsert_mut(record, 0) {
                panic!("failed to add record to authority");
            }
        }
        let authority = Arc::new(authority);

        let mut catalog = Catalog::new();
        catalog.upsert(zone_name.into(), vec![authority.clone()]);
        let mut server = ServerFuture::new(Wrapper(catalog));
        if let Some(tcp_addr) = config.tcp_addr {
            println!("binding to tcp: {tcp_addr}");
            server
                .register_listener_std(
                    std::net::TcpListener::bind(tcp_addr).unwrap(),
                    Duration::from_secs(5),
                )
                .unwrap();
        }
        if let Some(udp_addr) = config.udp_addr {
            println!("binding to udp: {udp_addr}");
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
            server_file: Mutex::new(file),
        })
    }

    async fn add_record(&self, record: Record) -> Result<Self::Key, Self::Error> {
        let hickory_record = hickory_record_from_record(record);
        //this whole thing is a bit of a bad hack for now.
        let mut file = self.server_file.lock().await;
        let id = file.add_record(hickory_record.clone());
        if !self.authority.upsert(hickory_record, 0).await {
            file.delete_record(id);
            // will panic here on identical records
            panic!("failed to add record to authority");
        }
        file.save().await?;
        Ok(id)
    }

    async fn delete_record(&self, key: Self::Key) -> Result<(), Self::Error> {
        let mut file = self.server_file.lock().await;
        if file.delete_record(key).is_some() {
            file.save().await?;
        }
        Ok(())
    }

    async fn list_records(&self) -> Result<Vec<(Self::Key, Record)>, Self::Error> {
        let file = self.server_file.lock().await;
        Ok(file
            .records
            .iter()
            .map(|(id, record)| (id.get(), record_from_hickory_record(record)))
            .collect())
    }
}
