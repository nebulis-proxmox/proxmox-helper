use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct ProxmoxData<T: Clone> {
    #[serde(bound(deserialize = "for<'a> T: Deserialize<'a>"))]
    pub data: T,
}
