pub mod api;
pub mod tasks;

use std::{
    hash::{DefaultHasher, Hash, Hasher},
    sync::Arc,
};

pub use api::*;
use tasks::{ControllerSharedCatalog, IpamSharedCatalog, NodeSharedCatalog, WorkerSharedCatalog};
use tokio::{sync::RwLock, task::JoinHandle};
use tokio_stream::{wrappers::WatchStream, StreamExt};
use tracing::debug;

pub struct Catalog {
    pub(crate) nodes_catalog: NodeSharedCatalog,
    pub(crate) nodes_catalog_hash: RwLock<u64>,

    pub(crate) controllers_catalog: ControllerSharedCatalog,
    pub(crate) controllers_catalog_hash: RwLock<u64>,

    pub(crate) workers_catalog: WorkerSharedCatalog,
    pub(crate) workers_catalog_hash: RwLock<u64>,

    pub(crate) ipams_catalog: IpamSharedCatalog,
    pub(crate) ipams_catalog_hash: RwLock<u64>,
}

impl Default for Catalog {
    fn default() -> Self {
        let empty_vec_hash = {
            let mut hasher = DefaultHasher::new();
            Vec::<u8>::new().hash(&mut hasher);
            hasher.finish()
        };

        Self {
            nodes_catalog: Arc::new(RwLock::new(ProxmoxData { data: Vec::new() })),
            nodes_catalog_hash: RwLock::new(empty_vec_hash),

            controllers_catalog: Arc::new(RwLock::new(ProxmoxData { data: Vec::new() })),
            controllers_catalog_hash: RwLock::new(empty_vec_hash),

            workers_catalog: Arc::new(RwLock::new(ProxmoxData { data: Vec::new() })),
            workers_catalog_hash: RwLock::new(empty_vec_hash),

            ipams_catalog: Arc::new(RwLock::new(ProxmoxData { data: Vec::new() })),
            ipams_catalog_hash: RwLock::new(empty_vec_hash),
        }
    }
}

impl Catalog {
    pub async fn get_nodes(&self) -> ProxmoxNodeEntries {
        self.nodes_catalog.read().await.clone()
    }

    pub async fn get_controllers(&self) -> ProxmoxVirtualMachineEntries {
        self.controllers_catalog.read().await.clone()
    }

    pub async fn get_workers(&self) -> ProxmoxVirtualMachineEntries {
        self.workers_catalog.read().await.clone()
    }

    pub async fn get_ipams(&self) -> ProxmoxIpamEntries {
        self.ipams_catalog.read().await.clone()
    }

    async fn get_previous_nodes_hash(&self) -> u64 {
        *self.nodes_catalog_hash.read().await
    }

    async fn get_previous_controllers_hash(&self) -> u64 {
        *self.controllers_catalog_hash.read().await
    }

    async fn get_previous_workers_hash(&self) -> u64 {
        *self.workers_catalog_hash.read().await
    }

    async fn get_previous_ipams_hash(&self) -> u64 {
        *self.ipams_catalog_hash.read().await
    }

    async fn get_current_nodes_hash(&self) -> u64 {
        let nodes = self.get_nodes().await;

        let mut hasher = DefaultHasher::new();

        nodes.hash(&mut hasher);

        hasher.finish()
    }

    async fn get_current_controllers_hash(&self) -> u64 {
        let controllers = self.get_controllers().await;

        let mut hasher = DefaultHasher::new();

        controllers.hash(&mut hasher);

        hasher.finish()
    }

    async fn get_current_workers_hash(&self) -> u64 {
        let workers = self.get_workers().await;

        let mut hasher = DefaultHasher::new();

        workers.hash(&mut hasher);

        hasher.finish()
    }

    async fn get_current_ipams_hash(&self) -> u64 {
        let ipams = self.get_ipams().await;

        let mut hasher = DefaultHasher::new();

        ipams.hash(&mut hasher);

        hasher.finish()
    }

    async fn update_nodes(&self, nodes: ProxmoxNodeEntries) {
        *self.nodes_catalog.write().await = nodes;
    }

    async fn update_controllers(&self, controllers: ProxmoxVirtualMachineEntries) {
        *self.controllers_catalog.write().await = controllers;
    }

    async fn update_workers(&self, workers: ProxmoxVirtualMachineEntries) {
        *self.workers_catalog.write().await = workers;
    }

    async fn update_ipams(&self, ipams: ProxmoxIpamEntries) {
        *self.ipams_catalog.write().await = ipams;
    }

    pub async fn update_nodes_and_hash(&self, nodes: ProxmoxNodeEntries) {
        let previous_node_names = self
            .get_nodes()
            .await
            .data
            .iter()
            .map(|node| node.node.clone())
            .collect::<Vec<_>>();

        let current_node_names = nodes
            .data
            .iter()
            .map(|node| node.node.clone())
            .collect::<Vec<_>>();

        self.update_nodes(nodes).await;

        let current_hash = self.get_current_nodes_hash().await;
        let previous_hash = self.get_previous_nodes_hash().await;

        if current_hash != previous_hash {
            *self.nodes_catalog_hash.write().await = current_hash;

            current_node_names
                .iter()
                .filter(|node| !previous_node_names.contains(node))
                .for_each(|node| {
                    debug!("New node detected: {}", node);
                });

            previous_node_names
                .iter()
                .filter(|node| !current_node_names.contains(node))
                .for_each(|node| {
                    debug!("Node removed: {}", node);
                });

            debug!("Updated nodes catalog");
        }
    }

    pub async fn update_controllers_and_hash(&self, controllers: ProxmoxVirtualMachineEntries) {
        let previous_controller_names = self
            .get_controllers()
            .await
            .data
            .iter()
            .map(|controller| controller.name.clone())
            .collect::<Vec<_>>();

        let current_controller_names = controllers
            .data
            .iter()
            .map(|controller| controller.name.clone())
            .collect::<Vec<_>>();

        self.update_controllers(controllers).await;

        let current_hash = self.get_current_controllers_hash().await;
        let previous_hash = self.get_previous_controllers_hash().await;

        if current_hash != previous_hash {
            *self.controllers_catalog_hash.write().await = current_hash;

            current_controller_names
                .iter()
                .filter(|controller| !previous_controller_names.contains(controller))
                .for_each(|controller| {
                    debug!("New controller detected: {}", controller);
                });

            previous_controller_names
                .iter()
                .filter(|controller| !current_controller_names.contains(controller))
                .for_each(|controller| {
                    debug!("Controller removed: {}", controller);
                });

            debug!("Updated controllers catalog");
        }
    }

    pub async fn update_workers_and_hash(&self, workers: ProxmoxVirtualMachineEntries) {
        let previous_worker_names = self
            .get_workers()
            .await
            .data
            .iter()
            .map(|worker| worker.name.clone())
            .collect::<Vec<_>>();

        let current_worker_names = workers
            .data
            .iter()
            .map(|worker| worker.name.clone())
            .collect::<Vec<_>>();

        self.update_workers(workers).await;

        let current_hash = self.get_current_workers_hash().await;
        let previous_hash = self.get_previous_workers_hash().await;

        if current_hash != previous_hash {
            *self.workers_catalog_hash.write().await = current_hash;

            current_worker_names
                .iter()
                .filter(|worker| !previous_worker_names.contains(worker))
                .for_each(|worker| {
                    debug!("New worker detected: {}", worker);
                });

            previous_worker_names
                .iter()
                .filter(|worker| !current_worker_names.contains(worker))
                .for_each(|worker| {
                    debug!("Worker removed: {}", worker);
                });

            debug!("Updated workers catalog");
        }
    }

    pub async fn update_ipams_and_hash(&self, ipams: ProxmoxIpamEntries) {
        let previous_ipam_names = self
            .get_ipams()
            .await
            .data
            .iter()
            .map(|ipam| ipam.ip.clone())
            .collect::<Vec<_>>();

        let current_ipam_names = ipams
            .data
            .iter()
            .map(|ipam| ipam.ip.clone())
            .collect::<Vec<_>>();

        self.update_ipams(ipams).await;

        let current_hash = self.get_current_ipams_hash().await;
        let previous_hash = self.get_previous_ipams_hash().await;

        if current_hash != previous_hash {
            *self.ipams_catalog_hash.write().await = current_hash;

            current_ipam_names
                .iter()
                .filter(|ipam| !previous_ipam_names.contains(ipam))
                .for_each(|ipam| {
                    debug!("New ipam detected: {}", ipam);
                });

            previous_ipam_names
                .iter()
                .filter(|ipam| !current_ipam_names.contains(ipam))
                .for_each(|ipam| {
                    debug!("Ipam removed: {}", ipam);
                });

            debug!("Updated ipams catalog");
        }
    }
}

pub type SharedCatalog = Arc<Catalog>;

pub async fn handle_catalog_update(
    client: reqwest::Client,
) -> anyhow::Result<(SharedCatalog, JoinHandle<()>)> {
    let catalog = Arc::new(Catalog::default());

    let (nodes_rx, nodes_updater_handle) = tasks::handle_pve_nodes_update(client.clone()).await?;
    let mut nodes_stream = WatchStream::new(nodes_rx.clone());

    let (controllers_rx, controllers_updater_handle) = tasks::handle_tagged_catalog_update(
        client.clone(),
        catalog.nodes_catalog.clone(),
        "k0s-controller",
    )
    .await?;
    let mut controllers_stream = WatchStream::new(controllers_rx.clone());

    let (workers_rx, workers_updater_handle) = tasks::handle_tagged_catalog_update(
        client.clone(),
        catalog.nodes_catalog.clone(),
        "k0s-worker",
    )
    .await?;
    let mut workers_stream = WatchStream::new(workers_rx.clone());

    let (ipams_rx, ipams_updater_handle) =
        tasks::handle_ipams_update(client.clone(), catalog.nodes_catalog.clone()).await?;
    let mut ipams_stream = WatchStream::new(ipams_rx.clone());

    let cloned_catalog = catalog.clone();

    let handle = tokio::task::Builder::new()
        .name("handle_catalog_update")
        .spawn(async move {
            tokio::pin!(nodes_updater_handle);
            tokio::pin!(controllers_updater_handle);
            tokio::pin!(workers_updater_handle);
            tokio::pin!(ipams_updater_handle);

            loop {
                tokio::select! {
                    _ = &mut nodes_updater_handle => {
                        break;
                    },
                    _ = &mut controllers_updater_handle => {
                        break;
                    },
                    _ = &mut workers_updater_handle => {
                        break;
                    },
                    _ = &mut ipams_updater_handle => {
                        break;
                    },
                    Some(nodes) = nodes_stream.next() => {
                        cloned_catalog.update_nodes_and_hash(nodes).await;
                    },
                    Some(controllers) = controllers_stream.next() => {
                        cloned_catalog.update_controllers_and_hash(controllers).await;
                    },
                    Some(workers) = workers_stream.next() => {
                        cloned_catalog.update_workers_and_hash(workers).await;
                    },
                    Some(ipams) = ipams_stream.next() => {
                        cloned_catalog.update_ipams_and_hash(ipams).await;
                    }
                }
            }
        })?;

    Ok((catalog, handle))
}
