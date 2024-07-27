use std::sync::Arc;

use tokio::{
    sync::{watch, RwLock},
    task::JoinHandle,
};
use tracing::info;

use crate::{
    proxmox::{get_all_vms_for_node, get_nodes, query_ticket},
    CONFIG,
};

use super::{
    get_ipams_for_node, ProxmoxData, ProxmoxIpamEntries, ProxmoxNodeEntries, ProxmoxTicket,
    ProxmoxVirtualMachineEntries,
};

pub type SharedCatalog<T> = Arc<RwLock<T>>;
pub type NodeSharedCatalog = SharedCatalog<ProxmoxNodeEntries>;
pub type ControllerSharedCatalog = SharedCatalog<ProxmoxVirtualMachineEntries>;
pub type WorkerSharedCatalog = SharedCatalog<ProxmoxVirtualMachineEntries>;
pub type IpamSharedCatalog = SharedCatalog<ProxmoxIpamEntries>;

type ControllerCatalogUpdaterHandle = JoinHandle<anyhow::Result<()>>;
type NodeUpdaterHandle = JoinHandle<anyhow::Result<()>>;
type TicketUpdaterHandle = JoinHandle<anyhow::Result<()>>;
type IpamCatalogUpdaterHandle = JoinHandle<anyhow::Result<()>>;

static PVE_REAUTHENTICATION_DELAY: u64 = 600;
static PVE_NODE_UPDATE_DELAY: u64 = 60;
static PVE_VM_UPDATE_DELAY: u64 = 2;

#[tracing::instrument(err)]
pub async fn handle_pve_authentication(
) -> anyhow::Result<(watch::Receiver<ProxmoxTicket>, TicketUpdaterHandle)> {
    info!("Querying first ticket");

    let mut ticket = query_ticket(&CONFIG.proxmox_api_user, &CONFIG.proxmox_api_password).await?;
    let (tx, rx) = watch::channel(ticket.clone());

    tx.send(ticket.clone())?;

    let ticket_updater_handle = tokio::task::Builder::new()
        .name("handle_pve_authentication")
        .spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(PVE_REAUTHENTICATION_DELAY))
                    .await;

                ticket = query_ticket(&ticket.data.username, &ticket.data.ticket).await?;

                tx.send(ticket.clone())?;
            }
        })?;

    Ok((rx, ticket_updater_handle))
}

#[tracing::instrument(skip(client), err)]
pub async fn handle_pve_nodes_update(
    client: reqwest::Client,
) -> anyhow::Result<(watch::Receiver<ProxmoxNodeEntries>, NodeUpdaterHandle)> {
    let nodes = get_nodes(client.clone()).await?;
    let (tx, rx) = watch::channel(nodes);

    let handle = tokio::task::Builder::new()
        .name("handle_pve_nodes_update")
        .spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(PVE_NODE_UPDATE_DELAY)).await;
                let nodes = get_nodes(client.clone()).await?;

                tx.send(nodes)?;
            }
        })?;

    Ok((rx, handle))
}

#[tracing::instrument(skip(client, nodes_catalog), err)]
async fn get_running_vms_with_tags(
    client: reqwest::Client,
    nodes_catalog: NodeSharedCatalog,
    tag: &str,
) -> anyhow::Result<ProxmoxVirtualMachineEntries> {
    let mut vms = vec![];

    for node in nodes_catalog.read().await.data.iter() {
        vms.extend(
            get_all_vms_for_node(client.clone(), node)
                .await?
                .data
                .into_iter()
                .filter(|vm| vm.tags.as_ref().map_or(false, |tags| tags.contains(tag)))
                .filter(|vm| vm.template.is_none())
                .filter(|vm| vm.status == "running"),
        );
    }

    vms.sort_by(|a, b| a.vmid.cmp(&b.vmid));

    Ok(ProxmoxData { data: vms })
}

#[tracing::instrument(skip(client, nodes_catalog), err)]
pub async fn handle_tagged_catalog_update(
    client: reqwest::Client,
    nodes_catalog: NodeSharedCatalog,
    tag: &'static str,
) -> anyhow::Result<(
    watch::Receiver<ProxmoxVirtualMachineEntries>,
    ControllerCatalogUpdaterHandle,
)> {
    let vms = get_running_vms_with_tags(client.clone(), nodes_catalog.clone(), tag).await?;

    let (tx, rx) = watch::channel(vms);

    let handle = tokio::task::Builder::new()
        .name(&format!("handle_tagged_catalog_update_{}", tag))
        .spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(PVE_VM_UPDATE_DELAY)).await;

                let vms =
                    get_running_vms_with_tags(client.clone(), nodes_catalog.clone(), tag).await?;

                tx.send(vms)?;
            }
        })?;

    Ok((rx, handle))
}

#[tracing::instrument(skip(client, nodes_catalog), err)]
async fn get_all_ipams_matching_criterias(
    client: reqwest::Client,
    nodes_catalog: NodeSharedCatalog,
) -> anyhow::Result<ProxmoxIpamEntries> {
    let mut ipam = vec![];

    for node in nodes_catalog.read().await.data.iter() {
        ipam.extend(
            get_ipams_for_node(client.clone(), node)
                .await?
                .data
                .into_iter()
                .filter(|entry| entry.vnet == CONFIG.internal_network_interface)
                .filter(|entry| entry.vmid.is_some()),
        );
    }

    ipam.sort_by(|a, b| a.vmid.cmp(&b.vmid));

    Ok(ProxmoxData { data: ipam })
}

#[tracing::instrument(skip(client, nodes_catalog), err)]
pub async fn handle_ipams_update(
    client: reqwest::Client,
    nodes_catalog: NodeSharedCatalog,
) -> anyhow::Result<(
    watch::Receiver<ProxmoxIpamEntries>,
    IpamCatalogUpdaterHandle,
)> {
    let vms = get_all_ipams_matching_criterias(client.clone(), nodes_catalog.clone()).await?;

    let (tx, rx) = watch::channel(vms);

    let handle = tokio::task::Builder::new()
        .name("handle_ipams_update")
        .spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(PVE_VM_UPDATE_DELAY)).await;

                let vms =
                    get_all_ipams_matching_criterias(client.clone(), nodes_catalog.clone()).await?;

                tx.send(vms)?;
            }
        })?;

    Ok((rx, handle))
}
