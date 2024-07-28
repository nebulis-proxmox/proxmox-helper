use std::{ffi::OsStr, net::SocketAddr, process::Output};

use axum::{
    extract::{ConnectInfo, Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::{io, process::Command};

use crate::{
    error::AppResult,
    proxmox::{ProxmoxIpamEntries, ProxmoxVirtualMachineEntries},
    WebserverState, CONFIG,
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

#[derive(Serialize, Deserialize)]
struct ClusterCertificateConfig {
    server: String,
    #[serde(rename = "certificate-authority-data")]
    certificate_authority_data: String,
}

#[derive(Serialize, Deserialize)]
struct ClusterConfig {
    cluster: ClusterCertificateConfig,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct ClustersConfig {
    cluster: Vec<ClusterConfig>,
}

#[derive(Serialize, Deserialize)]
struct ContextConfig {
    cluster: String,
    user: String,
}

#[derive(Serialize, Deserialize)]
struct ContextsConfig {
    context: Vec<ContextConfig>,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct UserConfig {
    #[serde(rename = "client-certificate-data")]
    client_certificate_data: String,
    #[serde(rename = "client-key-data")]
    client_key_data: String,
}

#[derive(Serialize, Deserialize)]
struct UsersConfig {
    user: Vec<UserConfig>,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct KubeConfig {
    #[serde(rename = "apiVersion")]
    api_version: String,
    clusters: Vec<ClustersConfig>,
    contexts: Vec<ContextsConfig>,
    #[serde(rename = "current-context")]
    current_context: String,
    kind: String,
    preferences: serde_yaml::Value,
    users: Vec<UsersConfig>,
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

#[tracing::instrument(skip(state), err)]
async fn create_user_kubeconfig(
    State(state): State<WebserverState>,
    Path(user): Path<String>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResult<String> {
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
        .execute_command(&format!(
            "k0s kubectl get clusterrolebinding | grep {user} | awk '{{print $1}}'"
        ))
        .await?;

    let result = String::from_utf8(result.stdout)?.trim().to_string();

    let role_bindings = result
        .split("\n")
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    for role_binding in role_bindings {
        node.execute_command(&format!(
            "k0s kubectl delete clusterrolebinding {role_binding} --all-namespaces"
        ))
        .await?;
    }

    let result = node
        .execute_command(&format!(
            "k0s kubeconfig create --groups \"system:masters\" {user} | tee /root/{user}-kubeconfig"
        ))
        .await?;

    let kube_config = result.stdout;

    node.execute_command(&format!(
        "k0s kubectl create clusterrolebinding --kubeconfig /root/{user}-kubeconfig {user}-admin-binding --clusterrole=admin --user={user}"
    )).await?;

    node.execute_command(&format!("rm -f /root/{user}-kubeconfig"))
        .await?;

    let yaml = String::from_utf8(kube_config)?;
    let mut kube_config: KubeConfig = serde_yaml::from_str(&yaml)?;

    kube_config.clusters.iter_mut().for_each(|cluster| {
        cluster.cluster.iter_mut().for_each(|cluster| {
            cluster
                .cluster
                .server
                .clone_from(&CONFIG.k8s_api_server_url)
        })
    });

    Ok(serde_yaml::to_string(&kube_config)?)
}

pub(crate) fn create_router() -> Router<super::WebserverState> {
    Router::new()
        .route("/bootstrapped", get(is_cluster_bootstrapped))
        .route("/controllers", get(get_controller_infos))
        .route("/controllers/token", post(create_controller_token))
        .route("/workers", get(get_workers_infos))
        .route("/workers/token", post(create_worker_token))
        .route("/vrrp/password", get(fetch_vrrp_password))
        .route("/user/:user", post(create_user_kubeconfig))
}
