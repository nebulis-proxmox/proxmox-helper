use anyhow::Context;
use axum::Router;
use clap::Parser;
use config::Config;
use network_interface::NetworkInterfaceConfig;
use once_cell::sync::Lazy;
use proxmox_api::SharedCatalog;
use std::net::SocketAddr;
use tokio::task::JoinHandle;
use tokio_stream::{wrappers::WatchStream, StreamExt};
use tower_http::trace::TraceLayer;
use tracing::{debug, warn};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

mod config;
mod error;
mod metadata;
mod models;

static CONFIG: Lazy<Config> = Lazy::new(Config::parse);

#[derive(Clone)]
pub(crate) struct WebserverState {
    pub _client: proxmox_api::ProxmoxApiClient,
    pub catalog: SharedCatalog,
}

fn get_exposed_address() -> anyhow::Result<(std::net::IpAddr, u16)> {
    let network_interfaces = network_interface::NetworkInterface::show()?;

    let interface_to_listen = network_interfaces
        .iter()
        .find(|interface| interface.name == CONFIG.internal_network_interface)
        .context(format!(
            "Network interface {} not found",
            CONFIG.internal_network_interface
        ))?;

    let address_to_listen = interface_to_listen
        .addr
        .iter()
        .find(|addr| addr.ip().is_ipv4())
        .context("No IPv4 address found")?
        .ip();

    Ok((address_to_listen, CONFIG.port))
}

async fn setup_webserver(
    client: proxmox_api::ProxmoxApiClient,
    catalog: SharedCatalog,
) -> anyhow::Result<JoinHandle<anyhow::Result<()>>> {
    let address_to_listen = get_exposed_address()?;

    let state = WebserverState {
        _client: client,
        catalog,
    };

    let app = Router::new()
        .nest("/latest/meta-data", metadata::create_router())
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(address_to_listen).await?;

    println!("Listening on {}", listener.local_addr()?);

    Ok(tokio::spawn(async move {
        Ok(axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await?)
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let console_layer = console_subscriber::spawn();

    tracing_subscriber::registry()
        .with(console_layer)
        .with(fmt::layer().with_filter(EnvFilter::from_default_env()))
        .init();

    let client = proxmox_api::ProxmoxApiClient::new(&CONFIG.proxmox_api_url)?;

    let (ticket_rx, ticket_updater_handle) = proxmox_api::handle_pve_authentication(
        client.clone(),
        &CONFIG.proxmox_api_user,
        &CONFIG.proxmox_api_password,
    )
    .await?;
    let mut ticket_stream = WatchStream::new(ticket_rx.clone());
    tokio::pin!(ticket_updater_handle);

    client.update_ticket(&ticket_rx.borrow())?;

    let (catalog, catalog_updater_handle) = proxmox_api::handle_full_catalog_update(
        client.clone(),
        CONFIG.internal_network_interface.clone(),
    )
    .await?;
    tokio::pin!(catalog_updater_handle);

    let webserver_handle = setup_webserver(client.clone(), catalog.clone()).await?;
    tokio::pin!(webserver_handle);

    loop {
        tokio::select! {
            _ = &mut ticket_updater_handle => {
                warn!("Ticket updater handle finished");
                break;
            },
            _ = &mut catalog_updater_handle => {
                warn!("Catalog updater handle finished");
                break;
            },
            _ = &mut webserver_handle => {
                warn!("Webserver handle finished");
                break;
            },
            Some(ticket) = ticket_stream.next() => {
                debug!("Setting new ticket in cookie jar");
                client.update_ticket(&ticket)?;
            }
        }
    }

    Ok(())
}
