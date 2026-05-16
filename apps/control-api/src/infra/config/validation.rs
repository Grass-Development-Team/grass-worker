use super::ConfigError;

pub trait Validate {
    fn validate(&self) -> Result<(), ConfigError>;
}
