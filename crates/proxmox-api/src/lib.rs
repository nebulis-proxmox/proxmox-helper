use std::{collections::HashMap, fmt::Debug, sync::Arc};

mod models;
mod catalog;
mod tasks;
pub use models::*;
pub use catalog::*;
pub use tasks::handle_pve_authentication;
use reqwest::cookie::Jar;

pub type ProxmoxTicket = ProxmoxData<Ticket>;
pub type ProxmoxNodeEntries = ProxmoxData<Vec<NodeEntry>>;
pub type ProxmoxVirtualMachineEntries = ProxmoxData<Vec<VirtualMachineEntry>>;
pub type ProxmoxIpamEntries = ProxmoxData<Vec<IpamEntry>>;

#[derive(Clone)]
pub struct ProxmoxApiClient {
    client: reqwest::Client,
    api_url: String,
    cookie_jar: Arc<Jar>,
}

impl ProxmoxApiClient {
    pub fn new<S: AsRef<str>>(api_url: S) -> anyhow::Result<Self> {
        let cookie_jar = Arc::new(Jar::default());

        Ok(Self {
            client: reqwest::Client::builder()
                .cookie_provider(cookie_jar.clone())
                .build()?,
            api_url: api_url.as_ref().to_string(),
            cookie_jar,
        })
    }

    pub fn update_ticket(&self, ticket: &ProxmoxTicket) -> anyhow::Result<()> {
        self.cookie_jar.add_cookie_str(
            &format!("PVEAuthCookie={}", ticket.data.ticket),
            &self.api_url.parse()?,
        );

        Ok(())
    }

    #[tracing::instrument(skip(self, password), err)]
    pub async fn query_ticket<S1, S2>(
        &self,
        username: S1,
        password: S2,
    ) -> anyhow::Result<ProxmoxTicket>
    where
        S1: AsRef<str> + Debug,
        S2: AsRef<str>,
    {
        let mut params = HashMap::new();

        params.insert("username", username.as_ref());
        params.insert("password", password.as_ref());

        let url = format!("{}/api2/json/access/ticket", &self.api_url);

        let response = self
            .client
            .post(url)
            .form(&params)
            .send()
            .await?
            .error_for_status()?;

        Ok(response.json().await?)
    }

    #[tracing::instrument(skip(self), err)]
    pub async fn get_nodes(&self) -> anyhow::Result<ProxmoxNodeEntries> {
        let url = format!("{}/api2/json/nodes", &self.api_url);

        Ok(self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    #[tracing::instrument(skip(self, node), err)]
    pub async fn get_all_vms_for_node(
        &self,
        node: &NodeEntry,
    ) -> anyhow::Result<ProxmoxVirtualMachineEntries> {
        let url = format!("{}/api2/json/nodes/{}/qemu", &self.api_url, node.node);

        Ok(self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    #[tracing::instrument(skip(self, node), err)]
    pub async fn get_ipams_for_node(&self, node: &NodeEntry) -> anyhow::Result<ProxmoxIpamEntries> {
        let url = format!(
            "{}/api2/json/cluster/sdn/ipams/{}/status",
            &self.api_url, node.node
        );

        Ok(self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
}
