use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, State},
    routing::get,
    Router,
};
use proxmox_api::{ProxmoxIpamEntries, ProxmoxVirtualMachineEntries};
use serde::Serialize;

use crate::{error::AppResult, WebserverState};

#[derive(Serialize)]
pub(crate) struct NodeInfo {
    pub name: String,
    pub ip: String,
    pub status: String,
    pub vmid: i64,
}

fn get_node_infos_from_catalogs(
    nodes: ProxmoxVirtualMachineEntries,
    ipams: ProxmoxIpamEntries,
) -> Vec<NodeInfo> {
    let mut infos = vec![];

    for node in nodes.data {
        let ipam = ipams.data.iter().find(|ipam| {
            ipam.vmid
                .as_ref()
                .is_some_and(|vmid| vmid == &node.vmid.to_string())
        });

        let ip = if let Some(ipam) = ipam {
            ipam.ip.clone()
        } else {
            continue;
        };

        infos.push(NodeInfo {
            name: node.name.clone(),
            status: node.status.clone(),
            vmid: node.vmid.clone(),
            ip,
        });
    }

    infos
}

#[tracing::instrument(skip(state), err)]
async fn get_instance_id(
    State(state): State<WebserverState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResult<String> {
    let controllers = state.catalog.get_controllers().await;
    let ipams = state.catalog.get_ipams().await;

    let requesting_node = get_node_infos_from_catalogs(controllers, ipams)
        .into_iter()
        .find(|node| node.ip == addr.ip().to_string());

    let node = if let Some(node) = requesting_node {
        node
    } else {
        return Err(anyhow::Error::msg("No node found").into());
    };

    Ok(node.vmid.to_string())
}

pub(crate) fn create_router() -> Router<super::WebserverState> {
    Router::new().route("/instance-id", get(get_instance_id))
}
