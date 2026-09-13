use axum::{
    extract::{Query, State},
    response::IntoResponse,
};
use sea_orm::{EntityTrait, PaginatorTrait, QueryOrder};
use serde::Deserialize;

use crate::{
    infra::{
        database::entity::announcement,
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/announcements", axum::routing::get(list))
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

fn view(item: &announcement::Model) -> ItemResponse {
    ItemResponse {
        id: item.id,
        title: item.title.clone(),
        content: item.content.clone(),
        auto_popup: item.auto_popup,
        published_at: item.published_at,
    }
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
    Ok(ok_response(ListResponse {
        announcements: announcements.iter().map(view).collect::<Vec<_>>(),
        pagination: ListPaginationResponse {
            page,
            per_page,
            total,
            total_pages: total.div_ceil(per_page),
        },
    }))
}

#[derive(serde::Serialize)]
struct ItemResponse {
    id: uuid::Uuid,
    title: String,
    content: String,
    auto_popup: bool,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    published_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct ListPaginationResponse {
    page: u64,
    per_page: u64,
    total: u64,
    total_pages: u64,
}

#[derive(serde::Serialize)]
struct ListResponse {
    announcements: Vec<ItemResponse>,
    pagination: ListPaginationResponse,
}
