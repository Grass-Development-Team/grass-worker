use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NodeManagerConfig {
    #[serde(default)]
    pub auto_start_local_node: bool,
    #[serde(default = "default_local_node_binary")]
    pub local_node_binary: String,
    #[serde(default = "default_local_node_config")]
    pub local_node_config: String,
    #[serde(default = "default_restart_on_exit")]
    pub restart_on_exit: bool,
}

impl Default for NodeManagerConfig {
    fn default() -> Self {
        Self {
            auto_start_local_node: false,
            local_node_binary: default_local_node_binary(),
            local_node_config: default_local_node_config(),
            restart_on_exit: true,
        }
    }
}

fn default_local_node_binary() -> String {
    "grass-node".to_owned()
}

fn default_local_node_config() -> String {
    "./node.toml".to_owned()
}

const fn default_restart_on_exit() -> bool {
    true
}
