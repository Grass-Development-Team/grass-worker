use crate::SetupStage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiInfo {
    Ready,
    Setup { stage: SetupStage },
}

impl ApiInfo {
    pub fn ready() -> Self {
        Self::Ready
    }

    pub fn setup(stage: SetupStage) -> Self {
        Self::Setup { stage }
    }
}
