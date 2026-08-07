mod chromium;

use async_trait::async_trait;

use crate::infra::config::screenshot::ScreenshotConfig;

pub struct CaptureRequest {
    pub url: String,
    pub cookie_name: String,
    pub cookie_value: String,
}

#[async_trait]
pub trait ScreenshotProvider: Send + Sync {
    async fn capture(&self, request: CaptureRequest) -> anyhow::Result<Vec<u8>>;
}

pub fn from_config(config: &ScreenshotConfig) -> Box<dyn ScreenshotProvider> {
    match config {
        ScreenshotConfig::Chromium { executable_path } => {
            Box::new(chromium::ChromiumProvider::new(executable_path.clone()))
        }
    }
}
