use axum::{
    Router,
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
};
use sea_orm::{EntityTrait, PaginatorTrait, QueryOrder};
use serde::Deserialize;
use serde_json::json;

use crate::{
    infra::{
        database::entity::announcement,
        error::{AppError, ok_response},
        http::{extractors::Session, timestamps::ts},
    },
    state::ControlApiState,
};

pub fn router() -> Router<ControlApiState> {
    Router::new().route("/announcements", get(list))
}

#[derive(Default, Deserialize)]
pub struct ListQuery {
    pub page: Option<u64>,
    pub per_page: Option<u64>,
}

fn database<'a>(
    state: &'a ControlApiState,
    op: &'static str,
) -> Result<&'a sea_orm::DatabaseConnection, AppError> {
    state.try_database().ok_or_else(|| AppError::Internal {
        op,
        message: "database not available".to_owned(),
    })
}

fn view(item: &announcement::Model) -> serde_json::Value {
    json!({
        "id": item.id,
        "title": item.title,
        "content": item.content,
        "auto_popup": item.auto_popup,
        "published_at": ts(item.published_at),
    })
}

pub async fn list(
    State(state): State<ControlApiState>,
    Session { .. }: Session,
    Query(query): Query<ListQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "announcements.list";
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(5).clamp(1, 50);
    let paginator = announcement::Entity::find()
        .order_by_desc(announcement::Column::PublishedAt)
        .order_by_desc(announcement::Column::Id)
        .paginate(database(&state, OP)?, per_page);
    let total = paginator
        .num_items()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let announcements =
        paginator
            .fetch_page(page - 1)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;
    Ok(ok_response(json!({
        "announcements": announcements.iter().map(view).collect::<Vec<_>>(),
        "pagination": {
            "page": page,
            "per_page": per_page,
            "total": total,
            "total_pages": total.div_ceil(per_page),
        },
    })))
}
