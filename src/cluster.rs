use std::{ffi::OsStr, net::SocketAddr, process::Output};

use axum::{
    extract::{ConnectInfo, State},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use tokio::{io, process::Command};

use crate::{
    error::AppResult,
    proxmox::{ProxmoxIpamEntries, ProxmoxVirtualMachineEntries},
    WebserverState,
};

#[derive(Serialize)]
pub(crate) struct NodeInfo {
    pub name: String,
    pub ip: String,
    pub status: String,
}

impl NodeInfo {
    pub async fn execute_command<S: AsRef<OsStr>>(&self, command: S) -> io::Result<Output> {
        Command::new("ssh")
            .arg("-o")
            .arg("StrictHostKeyChecking=no")
            .arg("-o")
            .arg("UserKnownHostsFile=/dev/null")
            .arg(format!("root@{}", self.ip))
            .arg(command)
            .output()
            .await
    }
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
            ip,
        });
    }

    infos
}

#[tracing::instrument(skip(state), err)]
async fn is_cluster_bootstrapped(
    State(state): State<WebserverState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResult<Json<bool>> {
    let controllers = state.catalog.get_controllers().await;
    let ipams = state.catalog.get_ipams().await;

    let infos = get_node_infos_from_catalogs(controllers, ipams)
        .into_iter()
        .filter(|node| node.ip != addr.ip().to_string())
        .collect::<Vec<_>>();

    let node = if let Some(node) = infos.first() {
        node
    } else {
        return Ok(Json(false));
    };

    let result = node.execute_command("k0s status").await?;

    Ok(Json(result.status.success()))
}

#[tracing::instrument(skip(state), err)]
async fn get_controller_infos(
    State(state): State<WebserverState>,
) -> AppResult<Json<Vec<NodeInfo>>> {
    let controllers = state.catalog.get_controllers().await;
    let ipams = state.catalog.get_ipams().await;

    let infos = get_node_infos_from_catalogs(controllers, ipams);
    Ok(Json(infos))
}

#[tracing::instrument(skip(state), err)]
async fn get_workers_infos(State(state): State<WebserverState>) -> AppResult<Json<Vec<NodeInfo>>> {
    let workers = state.catalog.get_workers().await;
    let ipams = state.catalog.get_ipams().await;

    let infos = get_node_infos_from_catalogs(workers, ipams);

    Ok(Json(infos))
}

#[tracing::instrument(skip(state), err)]
async fn create_controller_token(
    State(state): State<WebserverState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResult<Vec<u8>> {
    let controllers = state.catalog.get_controllers().await;
    let ipams = state.catalog.get_ipams().await;

    let infos = get_node_infos_from_catalogs(controllers, ipams)
        .into_iter()
        .filter(|node| node.ip != addr.ip().to_string())
        .collect::<Vec<_>>();

    let node = if let Some(node) = infos.first() {
        node
    } else {
        return Err(anyhow::Error::msg("No controllers found").into());
    };

    let result = node
        .execute_command("k0s token create --role=controller --expiry=1h")
        .await?;

    Ok(result.stdout)
}

#[tracing::instrument(skip(state), err)]
async fn create_worker_token(
    State(state): State<WebserverState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResult<Vec<u8>> {
    let controllers = state.catalog.get_controllers().await;
    let ipams = state.catalog.get_ipams().await;

    let infos = get_node_infos_from_catalogs(controllers, ipams)
        .into_iter()
        .filter(|node| node.ip != addr.ip().to_string())
        .collect::<Vec<_>>();

    let node = if let Some(node) = infos.first() {
        node
    } else {
        return Err(anyhow::Error::msg("No controllers found").into());
    };

    let result = node
        .execute_command("k0s token create --role=worker --expiry=1h")
        .await?;

    Ok(result.stdout)
}

#[tracing::instrument(skip(state), err)]
async fn fetch_vrrp_password(
    State(state): State<WebserverState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResult<Vec<u8>> {
    let controllers = state.catalog.get_controllers().await;
    let ipams = state.catalog.get_ipams().await;

    let infos = get_node_infos_from_catalogs(controllers, ipams)
        .into_iter()
        .filter(|node| node.ip != addr.ip().to_string())
        .collect::<Vec<_>>();

    let node = if let Some(node) = infos.first() {
        node
    } else {
        return Err(anyhow::Error::msg("No controllers found").into());
    };

    let result = node
        .execute_command("yq '.spec.network.controlPlaneLoadBalancing.keepalived.vrrpInstances[0].authPass' /etc/k0s/k0s.yaml")
        .await?;

    Ok(result.stdout)
}

pub(crate) fn create_router() -> Router<super::WebserverState> {
    Router::new()
        .route("/bootstrapped", get(is_cluster_bootstrapped))
        .route("/controllers", get(get_controller_infos))
        .route("/controllers/token", post(create_controller_token))
        .route("/workers", get(get_workers_infos))
        .route("/workers/token", post(create_worker_token))
        .route("/vrrp/password", get(fetch_vrrp_password))
}
