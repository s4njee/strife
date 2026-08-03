use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::ApiError;

const ROUTE: &str = "/api/search";

const DEFAULT_LIMIT: u32 = 25;

#[derive(Debug, Deserialize)]
struct SearchQuery {
    q: String,
    cursor: Option<Uuid>,
    limit: Option<u32>,
    #[serde(default)]
    include_trash: bool,
}

#[derive(Debug, Serialize)]
struct SearchMatch {
    node_id: Uuid,
    name: String,
    page_number: i32,
    snippet: String,
    score: f32,
}

#[derive(Debug, Serialize)]
struct SearchResponse {
    items: Vec<SearchMatch>,
    next_cursor: Option<Uuid>,
    indexed_documents: i64,
}

pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/api/search", get(search))
        .with_state(pool)
}

async fn search(
    State(pool): State<PgPool>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, ApiError> {
    let search = query.q.trim();
    if search.is_empty() {
        return Err(
            ApiError::BadRequest("Search query must not be empty".into())
                .rejected(ROUTE, "empty query"),
        );
    }
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, 100);
    let mut matches = strife_db::search_document_text(
        &pool,
        search,
        query.include_trash,
        query.cursor,
        limit.saturating_add(1),
    )
    .await
    .map_err(|error| ApiError::internal(error, ROUTE, "document search"))?;
    let has_more = matches.len() > limit as usize;
    if has_more {
        matches.pop();
    }
    let next_cursor = has_more
        .then(|| matches.last().map(|item| item.cursor_id))
        .flatten();
    let indexed_documents: i64 = sqlx::query_scalar!(
        "SELECT count(*) AS \"count!\" FROM document_text WHERE status = 'completed'"
    )
    .fetch_one(&pool)
    .await
    .map_err(|error| ApiError::internal(error, ROUTE, "document search"))?;
    Ok(Json(SearchResponse {
        items: matches
            .into_iter()
            .map(|item| SearchMatch {
                node_id: item.node_id,
                name: item.node_name,
                page_number: item.page_number,
                snippet: item.snippet,
                score: item.score,
            })
            .collect(),
        next_cursor,
        indexed_documents,
    }))
}
