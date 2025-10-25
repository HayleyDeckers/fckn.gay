use std::{
    collections::BTreeMap,
    io::Read,
    net::SocketAddr,
    num::NonZeroU64,
    ops::Deref,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
    vec,
};

use fckn_gay_dns_interface::{Dns, Record, RecordType};
use hickory_server::{
    ServerFuture,
    authority::MessageResponseBuilder,
    proto::{
        ProtoError,
        op::{OpCode, ResponseCode},
        rr::{
            DNSClass, Name, RData, Record as HickoryRecord, RecordType as HickoryRecordType, rdata,
        },
    },
    server::{Request, RequestHandler, ResponseHandler, ResponseInfo},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeSeq};
use tokio::{
    fs::File,
    io::{AsyncSeekExt, AsyncWriteExt},
    sync::{RwLock, RwLockReadGuard},
};

/// configuration for the Porkbun DNS provider.
#[derive(Debug, Deserialize)]
pub struct Config {
    file_path: String,
    tcp_addr: Option<SocketAddr>,
    udp_addr: Option<SocketAddr>,
}

/// A DNS provider implementation using Porkbun.
/// This struct holds the client for interacting with the Porkbun API.
pub struct HickoryDns {
    server_file: FileBacked,
}

#[derive(Debug, Default, Deserialize, Clone)]
struct Database {
    #[serde(default, deserialize_with = "deserialize_records")]
    records: Arc<RwLock<BTreeMap<NonZeroU64, HickoryRecord>>>,
}

#[derive(Serialize)]
struct LockedRecords<'a> {
    #[serde(serialize_with = "serialize_records")]
    records: RwLockReadGuard<'a, BTreeMap<NonZeroU64, HickoryRecord>>,
}

fn serialize_records<'a, S: Serializer>(
    records: &RwLockReadGuard<'a, BTreeMap<NonZeroU64, HickoryRecord>>,
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
) -> Result<Arc<RwLock<BTreeMap<NonZeroU64, HickoryRecord>>>, D::Error> {
    let seq = Vec::<(NonZeroU64, HickoryRecord)>::deserialize(deserializer)?;
    let map = seq.into_iter().collect();
    Ok(Arc::new(RwLock::new(map)))
}
impl Database {
    async fn add_record(&self, record: HickoryRecord) -> u64 {
        let mut records = self.records.write().await;
        let id = records
            .last_key_value()
            .map(|(id, _)| id.get())
            .unwrap_or_else(|| 0);
        let id = NonZeroU64::new(id + 1).expect("record id overflow");
        records.insert(id, record);
        id.get()
    }
    async fn delete_record(&self, id: u64) -> Option<HickoryRecord> {
        let mut records = self.records.write().await;
        records.remove(&NonZeroU64::new(id).unwrap())
    }

    async fn update_record(&self, id: u64, record: HickoryRecord) -> Option<HickoryRecord> {
        let mut records = self.records.write().await;
        if let Some(id) = NonZeroU64::new(id) {
            if records.contains_key(&id) {
                records.insert(id, record.clone());
                Some(record)
            } else {
                None
            }
        } else {
            None
        }
    }
}

struct FileBacked {
    file: RwLock<File>,
    data: Database,
}

impl Deref for FileBacked {
    type Target = Database;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl FileBacked {
    // fragile! can get corrupted if we get killed mid write
    async fn save(&self) -> Result<(), std::io::Error> {
        let mut file = self.file.write().await;
        file.seek(std::io::SeekFrom::Start(0)).await?;
        let records = LockedRecords {
            records: self.data.records.read().await,
        };
        let contents = toml::to_string(&records).map_err(std::io::Error::other)?;
        file.write_all(contents.as_bytes()).await?;
        file.set_len(contents.len() as u64).await?;
        file.flush().await?;
        Ok(())
    }
}

impl FileBacked {
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
        let file = RwLock::new(File::from_std(file));
        Self { file, data }
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
        let path = PathBuf::from(&config.file_path);
        let file = FileBacked::from_file(&path);
        let database = file.data.clone();
        let mut server = ServerFuture::new(database);
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
        tokio::spawn(async move {
            while let Err(e) = server.block_until_done().await {
                eprintln!("oopsie: {e}");
            }
        });
        Ok(Self { server_file: file })
    }

    async fn add_record(&self, record: Record) -> Result<Self::Key, Self::Error> {
        let hickory_record = hickory_record_from_record(record);
        //this whole thing is a bit of a bad hack for now.
        let id = self.server_file.add_record(hickory_record.clone()).await;
        self.server_file.save().await?;
        Ok(id)
    }

    async fn delete_record(&self, key: Self::Key) -> Result<(), Self::Error> {
        if self.server_file.delete_record(key).await.is_some() {
            self.server_file.save().await?;
        }
        Ok(())
    }

    async fn list_records(&self) -> Result<Vec<(Self::Key, Record)>, Self::Error> {
        let records = self.server_file.data.records.read().await;
        Ok(records
            .iter()
            .map(|(id, record)| (id.get(), record_from_hickory_record(record)))
            .collect())
    }

    async fn update_record(&self, key: Self::Key, record: Record) -> Result<(), Self::Error> {
        let hickory_record = hickory_record_from_record(record);
        if self
            .server_file
            .update_record(key, hickory_record)
            .await
            .is_some()
        {
            self.server_file.save().await?;
        }
        Ok(())
    }
}

// todo: is this correct?
//  what edge cases do we need to handle? (SOA? Multiple records in one query?)
// are we returning the right header?
// handle crashes/unwraps
#[async_trait::async_trait]
impl RequestHandler for Database {
    async fn handle_request<R: ResponseHandler>(
        &self,
        request: &Request,
        mut response_handle: R,
    ) -> ResponseInfo {
        if request.op_code() != OpCode::Query {
            response_handle
                .send_response(
                    MessageResponseBuilder::from_message_request(request)
                        .error_msg(request.header(), ResponseCode::NotImp),
                )
                .await
                .unwrap()
        } else {
            let response = MessageResponseBuilder::from_message_request(request);
            let mut answers = Vec::new();
            let rlock = self.records.read().await;

            for query in request.queries() {
                let name = query.name();
                // some clients send non-ascii names, technically invalid but the default behaviour here is a bit weird
                // so we check if there's any non-ascii characters and if so, we reparse the name as utf8
                // and then use that as the name, leading to proper handling of punycoded or non-escaped names.
                let name = if name.iter().any(|v| !v.is_ascii()) {
                    if let Ok(reparsed) = String::from_utf8(
                        name.iter()
                            .flat_map(|v| v.iter().copied().chain(std::iter::once(b'.')))
                            .collect(),
                    ) {
                        Name::from_utf8(reparsed).ok()
                    } else {
                        None
                    }
                } else {
                    None
                }
                .unwrap_or_else(|| name.clone().into());
                let query_type = query.query_type();
                let query_class = query.query_class();
                // class has to be IN for now
                if query_class != DNSClass::IN {
                    continue;
                }
                for record in rlock.values() {
                    if record.name().eq(&name)
                        && (record.record_type() == query_type
                            || query_type == HickoryRecordType::ANY)
                    {
                        answers.push(record);
                    }
                }
            }
            if answers.is_empty() {
                return response_handle
                    .send_response(
                        MessageResponseBuilder::from_message_request(request)
                            .error_msg(request.header(), ResponseCode::NXDomain),
                    )
                    .await
                    .unwrap();
            } else {
                let response = response.build(*request.header(), answers, vec![], vec![], vec![]);
                response_handle.send_response(response).await.unwrap()
            }
        }
    }
}
