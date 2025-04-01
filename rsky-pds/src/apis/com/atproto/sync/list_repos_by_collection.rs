use crate::apis::ApiError;
use crate::db::DbConn;
use anyhow::Result;
use base64::{engine::general_purpose::URL_SAFE, Engine as _};
use diesel::prelude::*;
use diesel::sql_types::{Bool, Text};
use diesel::QueryableByName;
use rocket::data::{Data, FromData, Outcome};
use rocket::http::Status;
use rocket::request::{FromParam, Request};
use rocket::serde::json::Json;
use rsky_syntax::nsid::ensure_valid_nsid;
use serde::{Deserialize, Serialize};
use std::ops::Deref;

#[derive(Debug, Serialize)]
pub struct ListReposByCollectionOutput {
    cursor: Option<String>,
    repos: Vec<Repo>,
}

#[derive(Debug, Serialize)]
pub struct Repo {
    did: String,
}

// Represents the cursor data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorData {
    last_seen_id: i64,
    did: String,
}

// Wrapper type for CursorData to avoid orphan rule issues
#[derive(Debug, Clone)]
pub struct CursorWrapper(Option<CursorData>);

impl CursorWrapper {
    pub fn new(cursor: Option<CursorData>) -> Self {
        CursorWrapper(cursor)
    }

    pub fn into_inner(self) -> Option<CursorData> {
        self.0
    }
}

impl Deref for CursorWrapper {
    type Target = Option<CursorData>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// Type alias for a collection NSID with validation
#[derive(Debug, Clone)]
pub struct NsidCollection(String);

impl NsidCollection {
    pub fn new(nsid: String) -> Result<Self, ApiError> {
        match ensure_valid_nsid(&nsid) {
            Ok(_) => Ok(NsidCollection(nsid)),
            Err(_) => Err(ApiError::InvalidRequest(
                "Invalid collection NSID format".to_string(),
            )),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// Implement FromParam for NsidCollection to allow direct validation in route params
impl<'a> FromParam<'a> for NsidCollection {
    type Error = ApiError;

    fn from_param(param: &'a str) -> Result<Self, Self::Error> {
        NsidCollection::new(param.to_string())
    }
}

#[rocket::async_trait]
impl<'r> FromData<'r> for NsidCollection {
    type Error = ApiError;

    async fn from_data(req: &'r Request<'_>, data: Data<'r>) -> Outcome<'r, Self> {
        use rocket::outcome::Outcome::*;

        // Check if we have a collection parameter in the query instead
        if let Some(collection) = req.query_value::<String>("collection").and_then(|r| r.ok()) {
            match NsidCollection::new(collection) {
                Ok(nsid) => return Success(nsid),
                Err(err) => return Error((Status::BadRequest, err)),
            }
        }

        // If not in the query, we'd expect it in the request body
        let limit = req.limits().get("string").unwrap_or_else(|| 256.into());
        let string = match data.open(limit).into_string().await {
            Ok(string) => string.into_inner(),
            Err(_) => {
                return Error((
                    Status::BadRequest,
                    ApiError::InvalidRequest("Failed to read collection parameter".to_string()),
                ))
            }
        };

        match NsidCollection::new(string) {
            Ok(nsid) => Success(nsid),
            Err(err) => Error((Status::BadRequest, err)),
        }
    }
}

#[rocket::async_trait]
impl<'r> FromData<'r> for CursorWrapper {
    type Error = ApiError;

    async fn from_data(req: &'r Request<'_>, data: Data<'r>) -> Outcome<'r, Self> {
        use rocket::outcome::Outcome::*;

        // First check if cursor is provided as a query parameter
        if let Some(cursor_str) = req.query_value::<String>("cursor").and_then(|r| r.ok()) {
            return match decode_cursor(&cursor_str) {
                Ok(cursor_data) => Success(CursorWrapper::new(Some(cursor_data))),
                Err(_) => Error((
                    Status::BadRequest,
                    ApiError::InvalidRequest("Invalid cursor format in query".to_string()),
                )),
            };
        }

        // If no cursor in query params, check if it's in the request body
        let limit = req.limits().get("string").unwrap_or_else(|| 256.into());
        let cursor_str = match data.open(limit).into_string().await {
            Ok(string) if string.is_empty() => return Success(CursorWrapper::new(None)), // No cursor provided
            Ok(string) => string.into_inner(),
            Err(_) => {
                return Error((
                    Status::BadRequest,
                    ApiError::InvalidRequest("Failed to read cursor data".to_string()),
                ))
            }
        };

        // Try to decode the cursor string
        match decode_cursor(&cursor_str) {
            Ok(cursor_data) => Success(CursorWrapper::new(Some(cursor_data))),
            Err(_) => Error((
                Status::BadRequest,
                ApiError::InvalidRequest("Invalid cursor format in body".to_string()),
            )),
        }
    }
}

async fn inner_list_repos_by_collection(
    collection: NsidCollection,
    limit: Option<i64>,
    cursor_data: Option<CursorData>,
    db: DbConn,
) -> Result<Json<ListReposByCollectionOutput>, ApiError> {
    // Default limit to 500 if not provided, max 2000
    let limit = limit.unwrap_or(500).min(2000).max(1);

    // Query records table to find DIDs that have the specified collection
    use crate::schema::pds::record::dsl as RecordSchema;

    // Define a result struct for SQL query
    #[derive(QueryableByName, Debug)]
    struct DidResult {
        #[diesel(sql_type = Text)]
        did: String,
    }

    let repos = match db
        .run(move |conn| -> Result<Vec<String>, diesel::result::Error> {
            if let Some(cursor_data) = &cursor_data {
                // Use a raw SQL query for better control over index usage
                // This allows us to specify index hints and use more efficient queries
                // The collection index is critical for this query's performance
                let query = diesel::sql_query(format!(
                    "SELECT DISTINCT did FROM pds.record 
                WHERE collection = $1 
                AND (did > $2 OR (did = $2 AND CTID::text::point[0]::int8 > $3))
                ORDER BY did ASC
                LIMIT $4"
                ))
                .bind::<Text, _>(collection.as_str())
                .bind::<Text, _>(&cursor_data.did)
                .bind::<diesel::sql_types::BigInt, _>(cursor_data.last_seen_id)
                .bind::<diesel::sql_types::BigInt, _>(limit + 1);

                // Execute and map results
                let results: Vec<DidResult> = query.load(conn)?;
                Ok(results.into_iter().map(|r| r.did).collect::<Vec<String>>())
            } else {
                // When no cursor is provided, use the standard Diesel query builder
                // which will take advantage of the collection index
                let results: Vec<String> = RecordSchema::record
                    .select(RecordSchema::did)
                    .filter(RecordSchema::collection.eq(collection.as_str()))
                    .distinct()
                    .order_by(RecordSchema::did.asc())
                    .limit(limit + 1)
                    .load(conn)?;

                Ok(results)
            }
        })
        .await
    {
        Ok(repos) => repos,
        Err(error) => {
            tracing::error!("Database error: {}", error);
            // Return a more informative error message that follows API specification
            return Err(ApiError::InvalidRequest(format!(
                "Failed to retrieve repository data: database error"
            )));
        }
    };

    // Determine if there are more results and create the next cursor
    let has_more = repos.len() > limit as usize;
    let repos_to_return = if has_more {
        repos[..limit as usize].to_vec()
    } else {
        repos
    };

    // Generate the next cursor if there are more results
    let next_cursor = if has_more && !repos_to_return.is_empty() {
        let last_did = repos_to_return.last().unwrap();
        let cursor_data = CursorData {
            // Use the current timestamp for ordering as we don't have explicit row IDs
            last_seen_id: chrono::Utc::now().timestamp(),
            did: last_did.clone(),
        };
        match encode_cursor(&cursor_data) {
            Ok(cursor) => Some(cursor),
            Err(_) => {
                tracing::error!("Failed to encode cursor");
                None
            }
        }
    } else {
        None
    };

    // Format response
    let result = ListReposByCollectionOutput {
        cursor: next_cursor,
        repos: repos_to_return
            .iter()
            .map(|did| Repo { did: did.clone() })
            .collect(),
    };

    Ok(Json(result))
}

// Helper function to encode cursor data to base64
fn encode_cursor(data: &CursorData) -> Result<String> {
    let serialized = serde_json::to_string(data)?;
    let encoded = URL_SAFE.encode(serialized);
    Ok(encoded)
}

// Helper function to decode cursor from base64
fn decode_cursor(cursor: &str) -> Result<CursorData> {
    let decoded = URL_SAFE.decode(cursor)?;
    let cursor_str = String::from_utf8(decoded)?;
    let cursor_data = serde_json::from_str::<CursorData>(&cursor_str)?;
    Ok(cursor_data)
}

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.sync.listReposByCollection?<collection>&<limit>&<cursor>")]
pub async fn list_repos_by_collection(
    collection: Option<String>,
    limit: Option<i64>,
    cursor: Option<String>,
    db: DbConn,
) -> Result<Json<ListReposByCollectionOutput>, ApiError> {
    // Convert and validate the collection
    let collection = match collection {
        Some(c) => match NsidCollection::new(c) {
            Ok(nsid) => nsid,
            Err(e) => return Err(e),
        },
        None => {
            return Err(ApiError::InvalidRequest(
                "Collection query parameter must be set".to_string(),
            ))
        }
    };

    // Parse cursor if provided
    let cursor_data = if let Some(cursor_str) = cursor {
        match decode_cursor(&cursor_str) {
            Ok(data) => Some(data),
            Err(_) => {
                return Err(ApiError::InvalidRequest(
                    "Invalid cursor format".to_string(),
                ))
            }
        }
    } else {
        None
    };

    match inner_list_repos_by_collection(collection, limit, cursor_data, db).await {
        Ok(res) => Ok(res),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {}", error);
            Err(error)
        }
    }
}
