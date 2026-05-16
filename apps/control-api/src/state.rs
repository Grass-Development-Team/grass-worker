use std::sync::Arc;

use crate::infra::config::ControlApiConfig;

#[derive(Clone)]
pub struct ControlApiState {
    pub config: Arc<ControlApiConfig>,
}

impl ControlApiState {
    pub fn new(config: ControlApiConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }
}
