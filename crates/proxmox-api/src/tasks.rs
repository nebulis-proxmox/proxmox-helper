use std::sync::Arc;

use crate::{
    ProxmoxData, ProxmoxIpamEntries, ProxmoxNodeEntries, ProxmoxTicket,
    ProxmoxVirtualMachineEntries,
};
use tokio::{
    sync::{watch, RwLock},
    task::JoinHandle,
};
use tracing::info;

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

#[tracing::instrument(skip(client, password), err)]
pub async fn handle_pve_authentication<S1, S2>(
    client: crate::ProxmoxApiClient,
    username: S1,
    password: S2,
) -> anyhow::Result<(watch::Receiver<ProxmoxTicket>, TicketUpdaterHandle)>
where
    S1: AsRef<str> + std::fmt::Debug,
    S2: AsRef<str>,
{
    info!("Querying first ticket");

    let mut ticket = client.query_ticket(username, password).await?;
    let (tx, rx) = watch::channel(ticket.clone());

    tx.send(ticket.clone())?;

    let ticket_updater_handle = tokio::task::Builder::new()
        .name("handle_pve_authentication")
        .spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(PVE_REAUTHENTICATION_DELAY))
                    .await;

                ticket = client
                    .query_ticket(&ticket.data.username, &ticket.data.ticket)
                    .await?;

                tx.send(ticket.clone())?;
            }
        })?;

    Ok((rx, ticket_updater_handle))
}

#[tracing::instrument(skip(client), err)]
pub async fn handle_pve_nodes_update(
    client: crate::ProxmoxApiClient,
) -> anyhow::Result<(watch::Receiver<ProxmoxNodeEntries>, NodeUpdaterHandle)> {
    let nodes = client.get_nodes().await?;
    let (tx, rx) = watch::channel(nodes);

    let handle = tokio::task::Builder::new()
        .name("handle_pve_nodes_update")
        .spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(PVE_NODE_UPDATE_DELAY)).await;
                let nodes = client.get_nodes().await?;

                tx.send(nodes)?;
            }
        })?;

    Ok((rx, handle))
}

#[tracing::instrument(skip(client, nodes_catalog), err)]
async fn get_running_vms(
    client: crate::ProxmoxApiClient,
    nodes_catalog: NodeSharedCatalog,
) -> anyhow::Result<ProxmoxVirtualMachineEntries> {
    let mut vms = vec![];

    for node in nodes_catalog.read().await.data.iter() {
        vms.extend(
            client
                .get_all_vms_for_node(node)
                .await?
                .data
                .into_iter()
                .filter(|vm| vm.template.is_none())
                .filter(|vm| vm.status == "running"),
        );
    }

    vms.sort_by(|a, b| a.vmid.cmp(&b.vmid));

    Ok(ProxmoxData { data: vms })
}

#[tracing::instrument(skip(client, nodes_catalog), err)]
async fn get_running_vms_with_tags(
    client: crate::ProxmoxApiClient,
    nodes_catalog: NodeSharedCatalog,
    tag: &str,
) -> anyhow::Result<ProxmoxVirtualMachineEntries> {
    let mut vms = vec![];

    for node in nodes_catalog.read().await.data.iter() {
        vms.extend(
            client
                .get_all_vms_for_node(node)
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
    client: crate::ProxmoxApiClient,
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
pub async fn handle_vms_catalog_update(
    client: crate::ProxmoxApiClient,
    nodes_catalog: NodeSharedCatalog,
) -> anyhow::Result<(
    watch::Receiver<ProxmoxVirtualMachineEntries>,
    ControllerCatalogUpdaterHandle,
)> {
    let vms = get_running_vms(client.clone(), nodes_catalog.clone()).await?;

    let (tx, rx) = watch::channel(vms);

    let handle = tokio::task::Builder::new()
        .name(&format!("handle_vms_catalog_update"))
        .spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(PVE_VM_UPDATE_DELAY)).await;

                let vms = get_running_vms(client.clone(), nodes_catalog.clone()).await?;

                tx.send(vms)?;
            }
        })?;

    Ok((rx, handle))
}

#[tracing::instrument(skip(client, nodes_catalog), err)]
async fn get_all_ipams_matching_criterias(
    client: crate::ProxmoxApiClient,
    nodes_catalog: NodeSharedCatalog,
    interface_name: String,
) -> anyhow::Result<ProxmoxIpamEntries> {
    let mut ipam = vec![];

    for node in nodes_catalog.read().await.data.iter() {
        ipam.extend(
            client
                .get_ipams_for_node(node)
                .await?
                .data
                .into_iter()
                .filter(|entry| entry.vnet == interface_name)
                .filter(|entry| entry.vmid.is_some()),
        );
    }

    ipam.sort_by(|a, b| a.vmid.cmp(&b.vmid));

    Ok(ProxmoxData { data: ipam })
}

#[tracing::instrument(skip(client, nodes_catalog), err)]
pub async fn handle_ipams_update(
    client: crate::ProxmoxApiClient,
    nodes_catalog: NodeSharedCatalog,
    interface_name: String,
) -> anyhow::Result<(
    watch::Receiver<ProxmoxIpamEntries>,
    IpamCatalogUpdaterHandle,
)> {
    let vms = get_all_ipams_matching_criterias(
        client.clone(),
        nodes_catalog.clone(),
        interface_name.clone(),
    )
    .await?;

    let (tx, rx) = watch::channel(vms);

    let handle = tokio::task::Builder::new()
        .name("handle_ipams_update")
        .spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(PVE_VM_UPDATE_DELAY)).await;

                let vms = get_all_ipams_matching_criterias(
                    client.clone(),
                    nodes_catalog.clone(),
                    interface_name.clone(),
                )
                .await?;

                tx.send(vms)?;
            }
        })?;

    Ok((rx, handle))
}
