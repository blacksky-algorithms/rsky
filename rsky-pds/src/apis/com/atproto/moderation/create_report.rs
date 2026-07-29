use crate::apis::{ApiError, ProxyResponder};
use crate::auth_verifier::AccessStandard;
use crate::pipethrough::{pipethrough_error, pipethrough_procedure, ProxyRequest};
use rocket::http::Header;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::moderation::CreateReportInput;

pub fn validate_report_input(input: &CreateReportInput) -> Result<(), ApiError> {
    if input.reason_type.trim().is_empty() {
        return Err(ApiError::InvalidRequest(
            "Input/reasonType must not be empty".to_string(),
        ));
    }
    Ok(())
}

/// Submit a moderation report regarding an atproto account or record. Implemented
/// by moderation services (with PDS proxying), and requires auth.
#[tracing::instrument(skip_all)]
#[rocket::post(
    "/xrpc/com.atproto.moderation.createReport",
    format = "json",
    data = "<body>"
)]
pub async fn create_report(
    body: Json<CreateReportInput>,
    auth: AccessStandard,
    req: ProxyRequest<'_>,
) -> Result<ProxyResponder, ApiError> {
    let input = body.into_inner();
    validate_report_input(&input)?;
    let requester: Option<String> = auth.access.credentials.and_then(|c| c.did);
    match pipethrough_procedure(&req, requester, Some(input)).await {
        Ok(res) => {
            let content_length = Header::new("content-length", res.buffer.len().to_string());
            let content_type = Header::new("content-type", res.encoding);
            Ok(ProxyResponder(res.buffer, content_length, content_type))
        }
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(pipethrough_error(&error))
        }
    }
}
