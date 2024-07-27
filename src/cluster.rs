use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use tokio::process::Command;

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
async fn is_cluster_bootstrapped(State(state): State<WebserverState>) -> AppResult<Json<bool>> {
    let controllers = state.catalog.get_controllers().await;
    let ipams = state.catalog.get_ipams().await;

    let infos = get_node_infos_from_catalogs(controllers, ipams);

    let node = if let Some(node) = infos.first() {
        node
    } else {
        return Err(anyhow::Error::msg("No controllers found").into());
    };

    let result = Command::new("ssh")
        .arg("-o")
        .arg("StrictHostKeyChecking=no")
        .arg("-o")
        .arg("UserKnownHostsFile=/dev/null")
        .arg(format!("root@{}", node.ip))
        .arg("k0s status")
        .output()
        .await?;

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
async fn create_controller_token(State(state): State<WebserverState>) -> AppResult<Vec<u8>> {
    let controllers = state.catalog.get_controllers().await;
    let ipams = state.catalog.get_ipams().await;

    let infos = get_node_infos_from_catalogs(controllers, ipams);

    let node = if let Some(node) = infos.first() {
        node
    } else {
        return Err(anyhow::Error::msg("No controllers found").into());
    };

    let result = Command::new("ssh")
        .arg("-o")
        .arg("StrictHostKeyChecking=no")
        .arg("-o")
        .arg("UserKnownHostsFile=/dev/null")
        .arg(format!("root@{}", node.ip))
        .arg("k0s token create --role=controller --expiry=1h")
        .output()
        .await?;

    Ok(result.stdout)
}

#[tracing::instrument(skip(state), err)]
async fn create_worker_token(State(state): State<WebserverState>) -> AppResult<Vec<u8>> {
    let controllers = state.catalog.get_controllers().await;
    let ipams = state.catalog.get_ipams().await;

    let infos = get_node_infos_from_catalogs(controllers, ipams);

    let node = if let Some(node) = infos.first() {
        node
    } else {
        return Err(anyhow::Error::msg("No controllers found").into());
    };

    let result = Command::new("ssh")
        .arg("-o")
        .arg("StrictHostKeyChecking=no")
        .arg("-o")
        .arg("UserKnownHostsFile=/dev/null")
        .arg(format!("root@{}", node.ip))
        .arg("k0s token create --role=worker --expiry=1h")
        .output()
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
}
