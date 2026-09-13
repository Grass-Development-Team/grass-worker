use crate::{
    domain::{
        delivery::DeliveryError, deployments::DeploymentStateError, scheduler::ScheduleError,
    },
    infra::error::AppError,
};

pub(crate) fn map_state_error(error: DeploymentStateError, op: &'static str) -> AppError {
    match error {
        DeploymentStateError::InvalidBuildTransition { .. }
        | DeploymentStateError::InvalidReleaseTransition { .. }
        | DeploymentStateError::InvalidServeTransition { .. }
        | DeploymentStateError::BuildNotReady
        | DeploymentStateError::ServeNotReady => AppError::Conflict {
            op,
            message: error.to_string(),
        },
        DeploymentStateError::Database(source) => AppError::Infrastructure {
            op,
            source: source.into(),
        },
    }
}

pub(crate) fn map_schedule_error(error: ScheduleError, op: &'static str) -> AppError {
    match error {
        ScheduleError::Database(source) => AppError::Infrastructure {
            op,
            source: source.into(),
        },
        error @ ScheduleError::InvalidData => AppError::Infrastructure {
            op,
            source: anyhow::Error::new(error),
        },
        other => AppError::Conflict {
            op,
            message: other.to_string(),
        },
    }
}

pub(crate) fn map_delivery_error(error: DeliveryError, op: &'static str) -> AppError {
    match error {
        DeliveryError::ReleaseAlreadyPending => AppError::Conflict {
            op,
            message: error.to_string(),
        },
        DeliveryError::State(error) => map_state_error(error, op),
        DeliveryError::Schedule(error) => map_schedule_error(error, op),
        DeliveryError::Database(source) => AppError::Infrastructure {
            op,
            source: source.into(),
        },
        DeliveryError::InvalidResources
        | DeliveryError::InvalidUnsuccessfulBuildTransition
        | DeliveryError::Other(_) => AppError::Infrastructure {
            op,
            source: anyhow::Error::new(error),
        },
    }
}
