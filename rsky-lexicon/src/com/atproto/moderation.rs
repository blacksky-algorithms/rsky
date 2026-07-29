use crate::com::atproto::admin::RepoRef;
use crate::com::atproto::repo::StrongRef;
use serde::{Deserialize, Serialize};

/// Subject of a moderation report: an account or a specific record.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "$type")]
pub enum ReportSubject {
    #[serde(rename = "com.atproto.admin.defs#repoRef")]
    RepoRef(RepoRef),
    #[serde(rename = "com.atproto.repo.strongRef")]
    StrongRef(StrongRef),
}

/// Submit a moderation report regarding an atproto account or record.
/// Implemented by moderation services (with PDS proxying), and requires auth.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CreateReportInput {
    /// Indicates the broad category of violation the report is for.
    pub reason_type: String,
    /// Additional context about the content and violation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub subject: ReportSubject,
}
