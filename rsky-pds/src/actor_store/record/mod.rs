use crate::db::DbConn;
use crate::models::{models, Backlink, Record};
use anyhow::{bail, Result};
use diesel::result::Error;
use diesel::*;
use futures::stream::{self, StreamExt};
use lexicon_cid::Cid;
use rsky_common;
use rsky_lexicon::com::atproto::admin::StatusAttr;
use rsky_repo::storage::Ipld;
use rsky_repo::types::{Ids, Lex, RepoRecord, WriteOpAction};
use rsky_repo::util::cbor_to_lex_record;
use rsky_syntax::aturi::AtUri;
use rsky_syntax::aturi_validation::ensure_valid_at_uri;
use rsky_syntax::did::ensure_valid_did;
use serde_json::Value as JsonValue;
use std::env;
use std::str::FromStr;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct GetRecord {
    pub uri: String,
    pub cid: String,
    pub value: RepoRecord,
    #[serde(rename = "indexedAt")]
    pub indexed_at: String,
    #[serde(rename = "takedownRef")]
    pub takedown_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RecordsForCollection {
    pub uri: String,
    pub cid: String,
    pub value: RepoRecord,
}

// @NOTE in the future this can be replaced with a more generic routine that pulls backlinks based on lex docs.
// For now, we just want to ensure we're tracking links from follows, blocks, likes, and reposts.
pub fn get_backlinks(uri: &AtUri, record: &RepoRecord) -> Result<Vec<Backlink>> {
    if let Some(Lex::Ipld(Ipld::Json(JsonValue::String(record_type)))) = record.get("$type") {
        if record_type == Ids::AppBskyGraphFollow.as_str()
            || record_type == Ids::AppBskyGraphBlock.as_str()
        {
            if let Some(Lex::Ipld(Ipld::Json(JsonValue::String(subject)))) = record.get("subject") {
                match ensure_valid_did(uri) {
                    Ok(_) => {
                        return Ok(vec![Backlink {
                            uri: uri.to_string(),
                            path: "subject".to_owned(),
                            link_to: subject.clone(),
                        }])
                    }
                    Err(e) => bail!("get_backlinks Error: invalid did {}", e),
                };
            }
        } else if record_type == Ids::AppBskyFeedLike.as_str()
            || record_type == Ids::AppBskyFeedRepost.as_str()
        {
            if let Some(Lex::Map(ref_object)) = record.get("subject") {
                if let Some(Lex::Ipld(Ipld::Json(JsonValue::String(subject_uri)))) =
                    ref_object.get("uri")
                {
                    match ensure_valid_at_uri(uri) {
                        Ok(_) => {
                            return Ok(vec![Backlink {
                                uri: uri.to_string(),
                                path: "subject.uri".to_owned(),
                                link_to: subject_uri.clone(),
                            }])
                        }
                        Err(e) => bail!("get_backlinks Error: invalid AtUri {}", e),
                    };
                }
            }
        }
    }
    Ok(Vec::new())
}

pub struct RecordReader {
    pub did: String,
    pub db: Arc<DbConn>,
}

// Basically handles getting lexicon records from db
impl RecordReader {
    pub fn new(did: String, db: Arc<DbConn>) -> Self {
        RecordReader { did, db }
    }

    pub async fn record_count(&mut self) -> Result<i64> {
        use crate::schema::pds::record::dsl::*;

        let other_did = self.did.clone();
        self.db
            .run(move |conn| {
                let res: i64 = record.filter(did.eq(&other_did)).count().get_result(conn)?;
                Ok(res)
            })
            .await
    }

    pub async fn list_collections(&mut self) -> Result<Vec<String>> {
        use crate::schema::pds::record::dsl::*;

        let other_did = self.did.clone();
        self.db
            .run(move |conn| {
                let collections = record
                    .filter(did.eq(&other_did))
                    .select(collection)
                    .group_by(collection)
                    .load::<String>(conn)?
                    .into_iter()
                    .collect::<Vec<String>>();
                Ok(collections)
            })
            .await
    }

    pub async fn list_records_for_collection(
        &mut self,
        collection: String,
        limit: i64,
        reverse: bool,
        cursor: Option<String>,
        rkey_start: Option<String>,
        rkey_end: Option<String>,
        include_soft_deleted: Option<bool>,
    ) -> Result<Vec<RecordsForCollection>> {
        use crate::schema::pds::record::dsl as RecordSchema;
        use crate::schema::pds::repo_block::dsl as RepoBlockSchema;

        let include_soft_deleted: bool = if let Some(include_soft_deleted) = include_soft_deleted {
            include_soft_deleted
        } else {
            false
        };
        let mut builder = RecordSchema::record
            .inner_join(RepoBlockSchema::repo_block.on(RepoBlockSchema::cid.eq(RecordSchema::cid)))
            .limit(limit)
            .select((models::Record::as_select(), models::RepoBlock::as_select()))
            .filter(RecordSchema::did.eq(self.did.clone()))
            .filter(RecordSchema::collection.eq(collection))
            .into_boxed();
        if !include_soft_deleted {
            builder = builder.filter(RecordSchema::takedownRef.is_null());
        }
        if reverse {
            builder = builder.order(RecordSchema::rkey.asc());
        } else {
            builder = builder.order(RecordSchema::rkey.desc());
        }

        if let Some(cursor) = cursor {
            if reverse {
                builder = builder.filter(RecordSchema::rkey.gt(cursor));
            } else {
                builder = builder.filter(RecordSchema::rkey.lt(cursor));
            }
        } else {
            if let Some(rkey_start) = rkey_start {
                builder = builder.filter(RecordSchema::rkey.gt(rkey_start));
            }
            if let Some(rkey_end) = rkey_end {
                builder = builder.filter(RecordSchema::rkey.lt(rkey_end));
            }
        }
        let res: Vec<(models::Record, models::RepoBlock)> =
            self.db.run(move |conn| builder.load(conn)).await?;
        res.into_iter()
            .map(|row| {
                Ok(RecordsForCollection {
                    uri: row.0.uri,
                    cid: row.0.cid,
                    value: cbor_to_lex_record(row.1.content)?,
                })
            })
            .collect::<Result<Vec<RecordsForCollection>>>()
    }

    pub async fn get_record(
        &mut self,
        uri: &AtUri,
        cid: Option<String>,
        include_soft_deleted: Option<bool>,
    ) -> Result<Option<GetRecord>> {
        use crate::schema::pds::record::dsl as RecordSchema;
        use crate::schema::pds::repo_block::dsl as RepoBlockSchema;

        let include_soft_deleted: bool = if let Some(include_soft_deleted) = include_soft_deleted {
            include_soft_deleted
        } else {
            false
        };
        let mut builder = RecordSchema::record
            .inner_join(RepoBlockSchema::repo_block.on(RepoBlockSchema::cid.eq(RecordSchema::cid)))
            .select((models::Record::as_select(), models::RepoBlock::as_select()))
            .filter(RecordSchema::uri.eq(uri.to_string()))
            .into_boxed();
        if !include_soft_deleted {
            builder = builder.filter(RecordSchema::takedownRef.is_null());
        }
        if let Some(cid) = cid {
            builder = builder.filter(RecordSchema::cid.eq(cid));
        }
        let record: Option<(models::Record, models::RepoBlock)> = self
            .db
            .run(move |conn| builder.first(conn).optional())
            .await?;
        if let Some(record) = record {
            Ok(Some(GetRecord {
                uri: record.0.uri,
                cid: record.0.cid,
                value: cbor_to_lex_record(record.1.content)?,
                indexed_at: record.0.indexed_at,
                takedown_ref: record.0.takedown_ref,
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn has_record(
        &mut self,
        uri: String,
        cid: Option<String>,
        include_soft_deleted: Option<bool>,
    ) -> Result<bool> {
        use crate::schema::pds::record::dsl as RecordSchema;

        let include_soft_deleted: bool = if let Some(include_soft_deleted) = include_soft_deleted {
            include_soft_deleted
        } else {
            false
        };
        let mut builder = RecordSchema::record
            .select(RecordSchema::uri)
            .filter(RecordSchema::uri.eq(uri))
            .into_boxed();
        if !include_soft_deleted {
            builder = builder.filter(RecordSchema::takedownRef.is_null());
        }
        if let Some(cid) = cid {
            builder = builder.filter(RecordSchema::cid.eq(cid));
        }
        let record_uri = self
            .db
            .run(move |conn| builder.first::<String>(conn).optional())
            .await?;
        Ok(!!record_uri.is_some())
    }

    pub async fn get_record_takedown_status(&self, uri: String) -> Result<Option<StatusAttr>> {
        use crate::schema::pds::record::dsl as RecordSchema;

        let res = self
            .db
            .run(move |conn| {
                RecordSchema::record
                    .select(RecordSchema::takedownRef)
                    .filter(RecordSchema::uri.eq(uri))
                    .first::<Option<String>>(conn)
                    .optional()
            })
            .await?;
        if let Some(res) = res {
            if let Some(takedown_ref) = res {
                Ok(Some(StatusAttr {
                    applied: true,
                    r#ref: Some(takedown_ref),
                }))
            } else {
                Ok(Some(StatusAttr {
                    applied: false,
                    r#ref: None,
                }))
            }
        } else {
            Ok(None)
        }
    }

    pub async fn get_current_record_cid(&self, uri: String) -> Result<Option<Cid>> {
        use crate::schema::pds::record::dsl as RecordSchema;

        let res = self
            .db
            .run(move |conn| {
                RecordSchema::record
                    .select(RecordSchema::cid)
                    .filter(RecordSchema::uri.eq(uri))
                    .first::<String>(conn)
                    .optional()
            })
            .await?;
        if let Some(res) = res {
            Ok(Some(Cid::from_str(&res)?))
        } else {
            Ok(None)
        }
    }

    pub async fn get_record_backlinks(
        &self,
        collection: String,
        path: String,
        link_to: String,
    ) -> Result<Vec<Record>> {
        use crate::schema::pds::backlink::dsl as BacklinkSchema;
        use crate::schema::pds::record::dsl as RecordSchema;

        let res = self
            .db
            .run(move |conn| {
                RecordSchema::record
                    .inner_join(
                        BacklinkSchema::backlink.on(BacklinkSchema::uri.eq(RecordSchema::uri)),
                    )
                    .select(Record::as_select())
                    .filter(BacklinkSchema::path.eq(path))
                    .filter(BacklinkSchema::linkTo.eq(link_to))
                    .filter(RecordSchema::collection.eq(collection))
                    .load::<Record>(conn)
            })
            .await?;
        Ok(res)
    }

    pub async fn get_backlink_conflicts(
        &self,
        uri: &AtUri,
        record: &RepoRecord,
    ) -> Result<Vec<AtUri>> {
        let record_backlinks = get_backlinks(uri, record)?;
        let conflicts: Vec<Vec<Record>> = stream::iter(record_backlinks)
            .then(|backlink| async move {
                Ok::<Vec<Record>, anyhow::Error>(
                    self.get_record_backlinks(
                        uri.get_collection(),
                        backlink.path,
                        backlink.link_to,
                    )
                    .await?,
                )
            })
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        Ok(conflicts
            .into_iter()
            .flatten()
            .filter_map(|record| {
                AtUri::make(
                    env::var("PDS_HOSTNAME").unwrap_or("localhost".to_owned()),
                    Some(String::from(uri.get_collection())),
                    Some(record.rkey),
                )
                .ok()
            })
            .collect::<Vec<AtUri>>())
    }

    // Transactors
    // -------------------
    #[tracing::instrument(skip_all)]
    pub async fn index_record(
        &self,
        uri: AtUri,
        cid: Cid,
        record: Option<RepoRecord>,
        action: Option<WriteOpAction>, // Create or update with a default of create
        repo_rev: String,
        timestamp: Option<String>,
    ) -> Result<()> {
        tracing::debug!("@LOG DEBUG RecordReader::index_record, indexing record {uri}");

        let collection = uri.get_collection();
        let rkey = uri.get_rkey();
        let hostname = uri.get_hostname().to_string();
        let action = action.unwrap_or(WriteOpAction::Create);
        let indexed_at = timestamp.unwrap_or_else(|| rsky_common::now());
        let row = Record {
            did: self.did.clone(),
            uri: uri.to_string(),
            cid: cid.to_string(),
            collection: collection.clone(),
            rkey: rkey.to_string(),
            repo_rev: Some(repo_rev.clone()),
            indexed_at: indexed_at.clone(),
            takedown_ref: None,
        };

        if !hostname.starts_with("did:") {
            bail!("Expected indexed URI to contain DID")
        } else if collection.is_empty() {
            bail!("Expected indexed URI to contain a collection")
        } else if rkey.is_empty() {
            bail!("Expected indexed URI to contain a record key")
        }

        use crate::schema::pds::record::dsl as RecordSchema;

        // Track current version of record
        let (record, uri) = self
            .db
            .run(move |conn| {
                insert_into(RecordSchema::record)
                    .values(row)
                    .on_conflict(RecordSchema::uri)
                    .do_update()
                    .set((
                        RecordSchema::cid.eq(cid.to_string()),
                        RecordSchema::repoRev.eq(&repo_rev),
                        RecordSchema::indexedAt.eq(&indexed_at),
                    ))
                    .execute(conn)?;
                Ok::<_, Error>((record, uri))
            })
            .await?;

        if let Some(record) = record {
            // Maintain backlinks
            let backlinks = get_backlinks(&uri, &record)?;
            if let WriteOpAction::Update = action {
                // On update just recreate backlinks from scratch for the record, so we can clear out
                // the old ones. E.g. for weird cases like updating a follow to be for a different did.
                self.remove_backlinks_by_uri(&uri).await?;
            }
            self.add_backlinks(backlinks).await?;
        }
        tracing::debug!("@LOG DEBUG RecordReader::index_record, indexed record {uri}");
        Ok(())
    }

    #[tracing::instrument(skip_all)]
    pub async fn delete_record(&self, uri: &AtUri) -> Result<()> {
        tracing::debug!("@LOG DEBUG RecordReader::delete_record, deleting indexed record {uri}");
        use crate::schema::pds::backlink::dsl as BacklinkSchema;
        use crate::schema::pds::record::dsl as RecordSchema;
        let uri = uri.to_string();
        self.db
            .run(move |conn| {
                delete(RecordSchema::record)
                    .filter(RecordSchema::uri.eq(&uri))
                    .execute(conn)?;
                delete(BacklinkSchema::backlink)
                    .filter(BacklinkSchema::uri.eq(&uri))
                    .execute(conn)?;
                tracing::debug!(
                    "@LOG DEBUG RecordReader::delete_record, deleted indexed record {uri}"
                );
                Ok(())
            })
            .await
    }

    pub async fn remove_backlinks_by_uri(&self, uri: &AtUri) -> Result<()> {
        use crate::schema::pds::backlink::dsl as BacklinkSchema;
        let uri = uri.to_string();
        self.db
            .run(move |conn| {
                delete(BacklinkSchema::backlink)
                    .filter(BacklinkSchema::uri.eq(uri))
                    .execute(conn)?;
                Ok(())
            })
            .await
    }

    pub async fn add_backlinks(&self, backlinks: Vec<Backlink>) -> Result<()> {
        if backlinks.len() == 0 {
            Ok(())
        } else {
            use crate::schema::pds::backlink::dsl as BacklinkSchema;
            self.db
                .run(move |conn| {
                    insert_into(BacklinkSchema::backlink)
                        .values(&backlinks)
                        .on_conflict_do_nothing()
                        .execute(conn)?;
                    Ok(())
                })
                .await
        }
    }

    pub async fn update_record_takedown_status(
        &self,
        uri: &AtUri,
        takedown: StatusAttr,
    ) -> Result<()> {
        use crate::schema::pds::record::dsl as RecordSchema;

        let takedown_ref: Option<String> = match takedown.applied {
            true => match takedown.r#ref {
                Some(takedown_ref) => Some(takedown_ref),
                None => Some(rsky_common::now()),
            },
            false => None,
        };
        let uri_string = uri.to_string();

        self.db
            .run(move |conn| {
                update(RecordSchema::record)
                    .filter(RecordSchema::uri.eq(uri_string))
                    .set(RecordSchema::takedownRef.eq(takedown_ref))
                    .execute(conn)?;
                Ok(())
            })
            .await
    }
}
