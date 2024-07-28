use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::hash::Hash;

#[derive(Serialize, Deserialize, Debug, Clone, Hash)]
pub struct ProxmoxData<T: Clone + Hash + Debug> {
    #[serde(bound(deserialize = "for<'a> T: Deserialize<'a>"))]
    pub data: T,
}

#[derive(Hash, Debug, Clone, Deserialize)]
pub struct Ticket {
    pub username: String,
    pub ticket: String,
    #[serde(rename = "CSRFPreventionToken")]
    pub _csrf_prevention_token: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NodeEntry {
    pub cpu: f64,
    pub maxcpu: i32,
    pub mem: i64,
    pub maxmem: i64,
    pub disk: i64,
    pub maxdisk: i64,
    pub uptime: i64,
    pub status: String,
    pub node: String,
    pub level: String,
    pub ssl_fingerprint: String,
}

impl Hash for NodeEntry {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.node.hash(state);
        self.level.hash(state);
        self.status.hash(state);
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VirtualMachineEntry {
    pub status: String,
    pub vmid: i64,
    pub name: String,
    pub template: Option<u8>,
    pub tags: Option<String>,
}

impl Hash for VirtualMachineEntry {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.vmid.hash(state);
        self.name.hash(state);
    }
}

#[derive(Hash, Clone, Debug, Deserialize, Serialize)]
pub struct IpamEntry {
    pub zone: String,
    pub hostname: Option<String>,
    pub vmid: Option<String>,
    pub vnet: String,
    pub ip: String,
    pub mac: Option<String>,
    pub subnet: String,
}
