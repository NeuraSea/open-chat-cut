use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ProjectId, Revision};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(
    tag = "code",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DomainError {
    #[error("invalid {field}: {message}")]
    InvalidTransaction { field: String, message: String },

    #[error("invalid document at {path}: {message}")]
    InvalidDocument { path: String, message: String },

    #[error("unsupported project schema version {actual}; supported version is {supported}")]
    UnsupportedSchemaVersion { actual: u32, supported: u32 },

    #[error(
        "revision conflict: expected {expected_revision}, current revision is {actual_revision}"
    )]
    RevisionConflict {
        expected_revision: Revision,
        actual_revision: Revision,
    },

    #[error("transaction targets project {actual_project_id}, not {expected_project_id}")]
    ProjectMismatch {
        expected_project_id: ProjectId,
        actual_project_id: ProjectId,
    },

    #[error("envelope hash mismatch: expected {expected_hash}, computed {actual_hash}")]
    EnvelopeHashMismatch {
        expected_hash: String,
        actual_hash: String,
    },

    #[error("duplicate {entity} ID '{id}'")]
    DuplicateEntity { entity: String, id: String },

    #[error("{entity} '{id}' does not exist")]
    EntityNotFound { entity: String, id: String },

    #[error("cannot change {entity} '{id}': it is referenced by {referenced_by}")]
    ReferentialIntegrity {
        entity: String,
        id: String,
        referenced_by: String,
    },

    #[error("invalid operation: {message}")]
    InvalidOperation { message: String },

    #[error("timeline arithmetic overflow")]
    ArithmeticOverflow,

    #[error("operation {operation_index} failed: {cause}")]
    OperationFailed {
        operation_index: usize,
        #[source]
        cause: Box<DomainError>,
    },

    #[error("failed to serialize canonical domain data: {message}")]
    Serialization { message: String },
}

impl DomainError {
    /// A stable machine-readable error code suitable for HTTP/MCP responses.
    pub const fn error_code(&self) -> &'static str {
        match self {
            Self::InvalidTransaction { .. } => "invalidTransaction",
            Self::InvalidDocument { .. } => "invalidDocument",
            Self::UnsupportedSchemaVersion { .. } => "unsupportedSchemaVersion",
            Self::RevisionConflict { .. } => "revisionConflict",
            Self::ProjectMismatch { .. } => "projectMismatch",
            Self::EnvelopeHashMismatch { .. } => "envelopeHashMismatch",
            Self::DuplicateEntity { .. } => "duplicateEntity",
            Self::EntityNotFound { .. } => "entityNotFound",
            Self::ReferentialIntegrity { .. } => "referentialIntegrity",
            Self::InvalidOperation { .. } => "invalidOperation",
            Self::ArithmeticOverflow => "arithmeticOverflow",
            Self::OperationFailed { .. } => "operationFailed",
            Self::Serialization { .. } => "serialization",
        }
    }

    pub const fn is_conflict(&self) -> bool {
        matches!(
            self,
            Self::RevisionConflict { .. } | Self::ProjectMismatch { .. }
        )
    }
}
