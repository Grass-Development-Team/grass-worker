use std::{collections::HashSet, future::Future};

use serde::Serialize;
use uuid::Uuid;

use crate::infra::error::AppError;

#[derive(Debug, Serialize)]
pub struct BatchItemResult {
    pub id: Uuid,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub fn normalize_ids(ids: Vec<Uuid>, op: &'static str) -> Result<Vec<Uuid>, AppError> {
    if ids.is_empty() || ids.len() > 100 {
        return Err(AppError::Validation {
            op,
            message: "ids must contain between 1 and 100 items".to_owned(),
        });
    }

    let mut seen = HashSet::with_capacity(ids.len());
    Ok(ids.into_iter().filter(|id| seen.insert(*id)).collect())
}

pub async fn run<F, Fut>(ids: Vec<Uuid>, mut operation: F) -> Vec<BatchItemResult>
where
    F: FnMut(Uuid) -> Fut,
    Fut: Future<Output = Result<(), AppError>>,
{
    let mut results = Vec::with_capacity(ids.len());
    for id in ids {
        match operation(id).await {
            Ok(()) => results.push(BatchItemResult {
                id,
                success: true,
                code: None,
                message: None,
            }),
            Err(error) => results.push(BatchItemResult {
                id,
                success: false,
                code: Some(error.error_code()),
                message: Some(error.to_string()),
            }),
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use crate::infra::error::AppError;

    use super::{normalize_ids, run};

    #[test]
    fn batch_ids_are_required_deduplicated_and_bounded() {
        let first = Uuid::now_v7();
        let second = Uuid::now_v7();

        assert!(normalize_ids(Vec::new(), "test.batch").is_err());
        assert_eq!(
            normalize_ids(vec![first, first, second], "test.batch").unwrap(),
            vec![first, second]
        );
        assert!(normalize_ids(vec![first; 101], "test.batch").is_err());
    }

    #[tokio::test]
    async fn batch_execution_reports_each_result_and_continues_after_failure() {
        let first = Uuid::now_v7();
        let second = Uuid::now_v7();
        let third = Uuid::now_v7();

        let results = run(vec![first, second, third], |id| async move {
            if id == second {
                Err(AppError::Conflict {
                    op: "test.batch.item",
                    message: "item is busy".to_owned(),
                })
            } else {
                Ok(())
            }
        })
        .await;

        assert_eq!(results.len(), 3);
        assert!(results[0].success);
        assert!(!results[1].success);
        assert_eq!(results[1].code, Some(40901));
        assert_eq!(results[1].message.as_deref(), Some("item is busy"));
        assert!(results[2].success);
    }

    #[tokio::test]
    async fn batch_results_never_expose_infrastructure_sources() {
        let id = Uuid::now_v7();
        let results = run(vec![id], |_| async {
            Err(AppError::Infrastructure {
                op: "test.batch.item",
                source: anyhow::anyhow!("database password leaked"),
            })
        })
        .await;

        assert_eq!(results[0].code, Some(50001));
        assert_eq!(
            results[0].message.as_deref(),
            Some("infrastructure service unavailable")
        );
    }
}
