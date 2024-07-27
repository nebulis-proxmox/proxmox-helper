use std::{collections::HashMap, fmt::Debug, hash::Hash};

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::CONFIG;

#[derive(Serialize, Deserialize, Debug, Clone, Hash)]
pub(crate) struct ProxmoxData<T: Clone + Hash + Debug> {
    #[serde(bound(deserialize = "for<'a> T: Deserialize<'a>"))]
    pub data: T,
}

#[derive(Hash, Debug, Clone, Deserialize)]
pub(crate) struct Ticket {
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

pub type ProxmoxTicket = ProxmoxData<Ticket>;
pub type ProxmoxNodeEntries = ProxmoxData<Vec<NodeEntry>>;
pub type ProxmoxVirtualMachineEntries = ProxmoxData<Vec<VirtualMachineEntry>>;
pub type ProxmoxIpamEntries = ProxmoxData<Vec<IpamEntry>>;

#[tracing::instrument(skip(password), err)]
pub async fn query_ticket<S1, S2>(username: S1, password: S2) -> anyhow::Result<ProxmoxTicket>
where
    S1: AsRef<str> + Debug,
    S2: AsRef<str> + Debug,
{
    info!("Querying ticket");

    let mut params = HashMap::new();

    params.insert("username", username.as_ref());
    params.insert("password", password.as_ref());

    let response = reqwest::Client::new()
        .post(format!(
            "{}/api2/json/access/ticket",
            &CONFIG.proxmox_api_url
        ))
        .form(&params)
        .send()
        .await?
        .error_for_status()?;

    Ok(response.json().await?)
}

#[tracing::instrument(skip(client), err)]
pub(crate) async fn get_nodes(client: reqwest::Client) -> anyhow::Result<ProxmoxNodeEntries> {
    Ok(client
        .get(format!("{}/api2/json/nodes", &CONFIG.proxmox_api_url))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

#[tracing::instrument(skip(client, node), err)]
pub(crate) async fn get_all_vms_for_node(
    client: reqwest::Client,
    node: &NodeEntry,
) -> anyhow::Result<ProxmoxVirtualMachineEntries> {
    Ok(client
        .get(format!(
            "{}/api2/json/nodes/{}/qemu",
            &CONFIG.proxmox_api_url, node.node
        ))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

#[tracing::instrument(skip(client, node), err)]
pub(crate) async fn get_ipams_for_node(
    client: reqwest::Client,
    node: &NodeEntry,
) -> anyhow::Result<ProxmoxIpamEntries> {
    Ok(client
        .get(format!(
            "{}/api2/json/cluster/sdn/ipams/{}/status",
            &CONFIG.proxmox_api_url, node.node
        ))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
