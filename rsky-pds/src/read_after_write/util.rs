use crate::account_manager::AccountManager;
use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::ActorStore;
use crate::db::DbConn;
use crate::pipethrough::parse_res;
use crate::read_after_write::types::LocalRecords;
use crate::read_after_write::viewer::{get_records_since_rev, LocalViewer};
use crate::xrpc_server::types::HandlerPipeThrough;
use crate::SharedLocalViewer;
use anyhow::Result;
use aws_config::SdkConfig;
use chrono::offset::Utc as UtcOffset;
use chrono::DateTime;
use rocket::http::Status;
use rocket::request::Request;
use rocket::response::{self, Responder, Response};
use rocket::State;
use rsky_common::time::from_str_to_utc;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Cursor;
use std::time::SystemTime;

const REPO_REV_HEADER: &str = "atproto-repo-rev";

pub type MungeFn<T> = fn(LocalViewer, T, LocalRecords, String) -> Result<T>;

#[derive(Serialize)]
pub struct HandlerResponse<T: Serialize> {
    pub encoding: String,
    pub body: T,
    pub headers: Option<BTreeMap<String, String>>,
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum ReadAfterWriteResponse<T: Serialize> {
    HandlerResponse(HandlerResponse<T>),
    HandlerPipeThrough(HandlerPipeThrough),
}

impl<'r, T: Serialize> Responder<'r, 'static> for ReadAfterWriteResponse<T> {
    fn respond_to(self, _req: &'r Request<'_>) -> response::Result<'static> {
        match self {
            ReadAfterWriteResponse::HandlerPipeThrough(pipethrough) => {
                let mut builder = Response::build();
                builder
                    .status(Status::Ok)
                    .raw_header("Content-Type", pipethrough.encoding)
                    .sized_body(pipethrough.buffer.len(), Cursor::new(pipethrough.buffer));
                if let Some(headers) = pipethrough.headers {
                    for header in headers.into_iter() {
                        builder.raw_header(header.0, header.1);
                    }
                }
                builder.ok()
            }
            ReadAfterWriteResponse::HandlerResponse(handler_response) => {
                let mut builder = Response::build();
                let encoding = handler_response.encoding.clone();
                let headers = handler_response.headers.clone();
                let bytes = serde_json::to_vec(&handler_response.body).unwrap();
                builder.sized_body(bytes.len(), Cursor::new(bytes));
                builder
                    .status(Status::Ok)
                    .raw_header("Content-Type", encoding);
                if let Some(headers) = headers {
                    for header in headers.into_iter() {
                        builder.raw_header(header.0, header.1);
                    }
                }
                builder.ok()
            }
        }
    }
}

pub fn get_repo_rev(headers: &BTreeMap<String, String>) -> Option<String> {
    headers.get(REPO_REV_HEADER).cloned()
}

pub fn get_local_lag(local: &LocalRecords) -> Result<Option<usize>> {
    let mut oldest: Option<String> = local.profile.as_ref().map(|profile| profile.indexed_at.clone());
    for post in local.posts.clone() {
        match oldest {
            None => oldest = Some(post.indexed_at),
            Some(ref current_oldest) if &post.indexed_at < current_oldest => {
                oldest = Some(post.indexed_at)
            }
            _ => (),
        }
    }
    match oldest {
        None => Ok(None),
        Some(oldest) => {
            let system_time = SystemTime::now();
            let now: DateTime<UtcOffset> = system_time.into();
            let duration = now - from_str_to_utc(&oldest)?;
            Ok(Some(duration.num_milliseconds() as usize))
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip_all)]
pub async fn handle_read_after_write<T: DeserializeOwned + serde::Serialize>(
    nsid: String,
    requester: String,
    res: HandlerPipeThrough,
    munge: MungeFn<T>,
    s3_config: &State<SdkConfig>,
    state_local_viewer: &State<SharedLocalViewer>,
    db: DbConn,
    account_manager: AccountManager,
) -> Result<ReadAfterWriteResponse<T>> {
    match read_after_write_internal(
        nsid,
        requester.clone(),
        res.clone(),
        munge,
        s3_config,
        state_local_viewer,
        db,
        account_manager,
    )
    .await
    {
        Ok(read_after_write_result) => Ok(read_after_write_result),
        Err(err) => {
            tracing::error!(
                "Error in read after write munge {} {}",
                err.to_string(),
                requester
            );
            Ok(ReadAfterWriteResponse::HandlerPipeThrough(res))
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn read_after_write_internal<T: DeserializeOwned + serde::Serialize>(
    nsid: String,
    requester: String,
    res: HandlerPipeThrough,
    munge: MungeFn<T>,
    s3_config: &State<SdkConfig>,
    state_local_viewer: &State<SharedLocalViewer>,
    db: DbConn,
    account_manager: AccountManager,
) -> Result<ReadAfterWriteResponse<T>> {
    let headers = &res.headers.clone().unwrap_or_default();
    let rev = get_repo_rev(headers);
    match rev {
        None => Ok(ReadAfterWriteResponse::HandlerPipeThrough(res)),
        Some(rev) => {
            let actor_store = ActorStore::new(
                requester.clone(),
                S3BlobStore::new(requester.clone(), s3_config),
                db,
            );
            let local = get_records_since_rev(&actor_store, rev).await?;
            if local.count <= 0 {
                return Ok(ReadAfterWriteResponse::HandlerPipeThrough(res));
            }
            let local_viewer_lock = state_local_viewer.local_viewer.read().await;
            let local_viewer = local_viewer_lock(actor_store, account_manager);
            let parse_res = parse_res(nsid, res)?;
            let data = munge(local_viewer, parse_res, local.clone(), requester)?;
            Ok(ReadAfterWriteResponse::HandlerResponse(
                format_munged_response(data, get_local_lag(&local)?)?,
            ))
        }
    }
}

pub fn format_munged_response<T: DeserializeOwned + serde::Serialize>(
    body: T,
    lag: Option<usize>,
) -> Result<HandlerResponse<T>> {
    Ok(HandlerResponse {
        encoding: "application/json".to_string(),
        body,
        headers: match lag {
            None => None,
            Some(lag) => {
                let mut headers = BTreeMap::new();
                headers.insert("Atproto-Upstream-Lag".to_string(), lag.to_string());
                Some(headers)
            }
        },
    })
}

pub fn nodejs_format(format: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut result = String::new();
    let parts = format.split("{}");
    for (i, part) in parts.enumerate() {
        result.push_str(part);
        if i < args.len() {
            result.push_str(&args[i].to_string());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(serde::Serialize, serde::Deserialize)]
    struct TestBody {
        did: String,
        handle: String,
    }

    #[test]
    fn handler_response_serializes_body_not_wrapper() {
        let response: HandlerResponse<TestBody> = HandlerResponse {
            encoding: "application/json".to_string(),
            body: TestBody {
                did: "did:plc:abc".to_string(),
                handle: "alice.bsky.social".to_string(),
            },
            headers: None,
        };

        let bytes = serde_json::to_vec(&response.body).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        // Body fields are present at the top level
        assert_eq!(value["did"], "did:plc:abc");
        assert_eq!(value["handle"], "alice.bsky.social");

        // Wrapper fields must not appear — that was the bug
        assert!(!value.as_object().unwrap().contains_key("encoding"));
        assert!(!value.as_object().unwrap().contains_key("body"));
        assert!(!value.as_object().unwrap().contains_key("headers"));
    }

    #[test]
    fn handler_response_wrapper_shape_is_wrong_for_xrpc() {
        // Demonstrates what the bug looked like: serializing the wrapper
        // instead of .body produces an envelope that clients cannot parse.
        let response: HandlerResponse<TestBody> = HandlerResponse {
            encoding: "application/json".to_string(),
            body: TestBody {
                did: "did:plc:abc".to_string(),
                handle: "alice.bsky.social".to_string(),
            },
            headers: None,
        };

        let wrapped = serde_json::to_value(&response).unwrap();
        // The wrapper contains envelope fields — this is what clients used to receive
        assert_eq!(wrapped["encoding"], json!("application/json"));
        assert!(wrapped.as_object().unwrap().contains_key("body"));
    }
}
