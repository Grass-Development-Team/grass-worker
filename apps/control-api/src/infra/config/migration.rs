use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct MigrationConfig {
    #[serde(default)]
    pub auto_migrate: bool,
}
