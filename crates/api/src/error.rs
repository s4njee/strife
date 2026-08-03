//! One error type and one error body for every route.
//!
//! Before this existed, four modules each declared their own `ErrorBody` and
//! three declared their own `ApiError`, while a further set of endpoints
//! returned a bare `StatusCode` with an empty body. A client could not write one
//! error handler, and adding a failure mode meant finding every copy.
//!
//! Two properties are deliberate:
//!
//! - **The wire format is fixed and additive.** Every response is `{ code,
//!   message }`, with optional extras that are omitted when absent rather than
//!   sent as null. The existing `code` values are preserved exactly, so this is
//!   a consolidation rather than a contract change.
//! - **Detail goes to logs, never to the client.** An internal failure carries
//!   its cause into `tracing` and returns a fixed generic message. A caller
//!   learns that something failed; only an operator learns why.

use std::borrow::Cow;

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use tracing::{error, warn};

use crate::folders::MoveConflictResponse;

/// The JSON body every failing route returns.
///
/// `code` is the stable machine-readable discriminator the frontend switches on;
/// `message` is human-readable and may change. The remaining fields exist only
/// for the errors that need them and are omitted otherwise, so a client parsing
/// the common shape never sees a field it does not understand.
#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
    /// Mirrors `code` for `disk_full` only, which shipped with both fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_percent: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflicts: Option<Vec<MoveConflictResponse>>,
}

/// Every way an API route can fail.
///
/// Variants carrying a message let one variant serve several routes without
/// inventing a code per route: a missing folder and a missing upload session are
/// both `not_found`, and the message says which.
#[derive(Debug)]
pub enum ApiError {
    /// `Cow` so a fixed message costs no allocation while a formatted one — a
    /// validation failure quoting the offending value — is still expressible.
    BadRequest(Cow<'static, str>),
    NotFound(&'static str),
    NameConflict(&'static str),
    CycleDetected,
    MoveConflict(Vec<MoveConflictResponse>),
    NotTrashed,
    CannotTrashRoot,
    RangeConflict,
    UploadInactive,
    UploadIncomplete,
    DiskFull(u64),
    ImportDisabled,
    /// Range requests need the total size to build `Content-Range`.
    RangeNotSatisfiable(u64),
    /// The cause is already logged by the constructor; this carries only what
    /// the client is allowed to see.
    Internal(&'static str),
}

impl ApiError {
    /// Records an internal failure's cause and returns the client-safe error.
    ///
    /// The cause reaches `tracing` and nothing else. Taking the route and an
    /// identifier makes a production log line locatable: "which endpoint, which
    /// node" is the first question asked about any 500.
    #[must_use]
    pub fn internal(
        cause: impl std::fmt::Display,
        route: &'static str,
        subject: impl std::fmt::Display,
    ) -> Self {
        error!(error = %cause, route, subject = %subject, "internal server error");
        Self::Internal("The request could not be completed")
    }

    /// Same, with a message describing the operation that failed.
    #[must_use]
    pub fn internal_with(
        cause: impl std::fmt::Display,
        route: &'static str,
        subject: impl std::fmt::Display,
        message: &'static str,
    ) -> Self {
        error!(error = %cause, route, subject = %subject, "internal server error");
        Self::Internal(message)
    }

    /// Records a client-caused failure at `warn`.
    ///
    /// Deliberately not `error!`: a 4xx is the API working correctly. Logging it
    /// at error level is what turns an error log into noise nobody reads, which
    /// is the same as having no error log.
    #[must_use]
    pub fn rejected(self, route: &'static str, reason: impl std::fmt::Display) -> Self {
        warn!(route, reason = %reason, code = self.code(), "request rejected");
        self
    }

    /// The stable machine-readable discriminator.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "bad_request",
            Self::NotFound(_) => "not_found",
            Self::NameConflict(_) => "name_conflict",
            Self::CycleDetected => "cycle_detected",
            Self::MoveConflict(_) => "move_conflict",
            Self::NotTrashed => "not_trashed",
            Self::CannotTrashRoot => "cannot_trash_root",
            Self::RangeConflict => "range_conflict",
            Self::UploadInactive => "upload_inactive",
            Self::UploadIncomplete => "upload_incomplete",
            Self::DiskFull(_) => "disk_full",
            Self::ImportDisabled => "source_disabled",
            Self::RangeNotSatisfiable(_) => "range_not_satisfiable",
            Self::Internal(_) => "internal_error",
        }
    }

    #[must_use]
    pub const fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) | Self::CycleDetected | Self::UploadIncomplete => {
                StatusCode::BAD_REQUEST
            }
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::NameConflict(_)
            | Self::MoveConflict(_)
            | Self::RangeConflict
            | Self::ImportDisabled
            | Self::NotTrashed => StatusCode::CONFLICT,
            Self::CannotTrashRoot => StatusCode::BAD_REQUEST,
            Self::UploadInactive => StatusCode::GONE,
            Self::DiskFull(_) => StatusCode::INSUFFICIENT_STORAGE,
            Self::RangeNotSatisfiable(_) => StatusCode::RANGE_NOT_SATISFIABLE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn message(&self) -> String {
        match self {
            Self::BadRequest(message) => message.clone().into_owned(),
            Self::NotFound(what) | Self::NameConflict(what) | Self::Internal(what) => {
                (*what).to_owned()
            }
            Self::CycleDetected => "Moving this folder would create a cycle".to_owned(),
            Self::MoveConflict(_) => "One or more folders cannot be moved".to_owned(),
            Self::NotTrashed => "Node is not in the trash".to_owned(),
            Self::CannotTrashRoot => "The root folder cannot be moved to trash".to_owned(),
            Self::RangeConflict => "This byte range has already been received".to_owned(),
            Self::UploadInactive => "The upload session is no longer active".to_owned(),
            Self::UploadIncomplete => "The upload has missing bytes".to_owned(),
            Self::DiskFull(_) => "Storage does not have enough safe capacity".to_owned(),
            Self::ImportDisabled => "Import source is disabled".to_owned(),
            Self::RangeNotSatisfiable(_) => "The requested range is not satisfiable".to_owned(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let code = self.code();
        let message = self.message();

        // A range rejection answers with Content-Range rather than a body, which
        // is what a media client reads to correct its request.
        if let Self::RangeNotSatisfiable(total) = self {
            return (
                status,
                [(
                    axum::http::header::CONTENT_RANGE,
                    format!("bytes */{total}"),
                )],
                Json(ErrorBody {
                    code,
                    message,
                    error: None,
                    usage_percent: None,
                    conflicts: None,
                }),
            )
                .into_response();
        }

        let (usage_percent, conflicts) = match self {
            Self::DiskFull(usage) => (Some(usage), None),
            Self::MoveConflict(conflicts) => (None, Some(conflicts)),
            _ => (None, None),
        };
        (
            status,
            Json(ErrorBody {
                code,
                message,
                // `disk_full` shipped with both `code` and `error` set; kept so
                // the existing frontend check keeps working.
                error: (code == "disk_full").then_some(code),
                usage_percent,
                conflicts,
            }),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::ApiError;
    use axum::http::StatusCode;

    #[test]
    fn every_variant_keeps_its_shipped_code_and_status() {
        // These pairs are the frontend's contract. Changing one is a breaking
        // change, so they are pinned rather than left to inspection.
        let cases: Vec<(ApiError, &str, StatusCode)> = vec![
            (
                ApiError::BadRequest("x".into()),
                "bad_request",
                StatusCode::BAD_REQUEST,
            ),
            (ApiError::NotFound("x"), "not_found", StatusCode::NOT_FOUND),
            (
                ApiError::NameConflict("x"),
                "name_conflict",
                StatusCode::CONFLICT,
            ),
            (
                ApiError::CycleDetected,
                "cycle_detected",
                StatusCode::BAD_REQUEST,
            ),
            (
                ApiError::MoveConflict(Vec::new()),
                "move_conflict",
                StatusCode::CONFLICT,
            ),
            (ApiError::NotTrashed, "not_trashed", StatusCode::CONFLICT),
            (
                ApiError::CannotTrashRoot,
                "cannot_trash_root",
                StatusCode::BAD_REQUEST,
            ),
            (
                ApiError::ImportDisabled,
                "source_disabled",
                StatusCode::CONFLICT,
            ),
            (
                ApiError::RangeConflict,
                "range_conflict",
                StatusCode::CONFLICT,
            ),
            (
                ApiError::UploadInactive,
                "upload_inactive",
                StatusCode::GONE,
            ),
            (
                ApiError::UploadIncomplete,
                "upload_incomplete",
                StatusCode::BAD_REQUEST,
            ),
            (
                ApiError::DiskFull(91),
                "disk_full",
                StatusCode::INSUFFICIENT_STORAGE,
            ),
            (
                ApiError::Internal("x"),
                "internal_error",
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ];
        for (error, code, status) in cases {
            assert_eq!(error.code(), code);
            assert_eq!(error.status(), status, "{code}");
        }
    }

    #[test]
    fn an_internal_error_never_carries_its_cause_to_the_client() {
        let error = ApiError::internal(
            "sqlx: relation \"secret_table\" does not exist",
            "/api/test",
            "node-1",
        );
        let message = match &error {
            ApiError::Internal(message) => *message,
            _ => panic!("expected an internal error"),
        };
        // The cause is in the logs; the client is told only that it failed.
        assert!(!message.contains("sqlx"));
        assert!(!message.contains("secret_table"));
    }
}
