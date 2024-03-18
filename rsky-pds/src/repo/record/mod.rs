use crate::models::models;
use crate::repo::types::{Ids, Lex, RepoRecord, StatusAttr, WriteOpAction};
use crate::repo::util::cbor_to_lex_record;
use crate::storage::Ipld;
use anyhow::Result;
use diesel::*;
use libipld::Cid;
use std::env;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct GetRecord {
    uri: String,
    cid: String,
    value: RepoRecord,
    #[serde(rename = "indexedAt")]
    indexed_at: String,
    #[serde(rename = "takedownRef")]
    takedown_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RecordsForCollection {
    uri: String,
    cid: String,
    value: RepoRecord,
}

// @NOTE in the future this can be replaced with a more generic routine that pulls backlinks based on lex docs.
// For now, we just want to ensure we're tracking links from follows, blocks, likes, and reposts.
pub fn get_backlinks(uri: String, record: RepoRecord) -> Result<Vec<models::Backlink>> {
    if let Some(Lex::Ipld(Ipld::String(record_type))) = record.get("$type") {
        if record_type == Ids::AppBskyGraphFollow.as_str()
            || record_type == Ids::AppBskyGraphBlock.as_str()
        {
            if let Some(Lex::Ipld(Ipld::String(subject))) = record.get("subject") {
                // TO DO: Ensure valid DID https://github.com/bluesky-social/atproto/blob/main/packages/syntax/src/did.ts
                return Ok(vec![models::Backlink {
                    uri,
                    path: "subject".to_owned(),
                    link_to: subject.clone(),
                }]);
            }
        } else if record_type == Ids::AppBskyFeedLike.as_str()
            || record_type == Ids::AppBskyFeedRepost.as_str()
        {
            if let Some(Lex::Map(ref_object)) = record.get("subject") {
                if let Some(Lex::Ipld(Ipld::String(subject_uri))) = ref_object.get("uri") {
                    // TO DO: Ensure valid AT URI
                    return Ok(vec![models::Backlink {
                        uri,
                        path: "subject.uri".to_owned(),
                        link_to: subject_uri.clone(),
                    }]);
                }
            }
        }
    }
    Ok(Vec::new())
}

pub struct RecordReader<'a> {
    pub conn: &'a PgConnection,
}

// Basically handles getting lexicon records from db
impl<'a> RecordReader<'a> {
    pub fn new(conn: &mut PgConnection) -> Self {
        RecordReader { conn }
    }

    pub fn record_count(&mut self, conn: &mut PgConnection) -> Result<i64> {
        use crate::schema::pds::record::dsl::*;
        let res: i64 = record.count().get_result(conn)?;
        Ok(res)
    }

    pub fn list_collection(&mut self, conn: &mut PgConnection) -> Result<Vec<String>> {
        use crate::schema::pds::record::dsl::*;
        let collections = record
            .select(collection)
            .group_by(collection)
            .load::<String>(conn)?
            .into_iter()
            .collect::<Vec<String>>();
        Ok(collections)
    }

    pub fn list_records_for_collection(
        &mut self,
        conn: &mut PgConnection,
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
        let res: Vec<(models::Record, models::RepoBlock)> = builder.load(conn)?;
        Ok(res
            .into_iter()
            .map(|row| {
                Ok(RecordsForCollection {
                    uri: row.0.uri,
                    cid: row.0.cid,
                    value: cbor_to_lex_record(row.1.content)?,
                })
            })
            .collect::<Result<Vec<RecordsForCollection>>>()?)
    }

    pub fn get_record(
        &mut self,
        conn: &mut PgConnection,
        uri: String,
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
            .filter(RecordSchema::uri.eq(uri))
            .into_boxed();
        if !include_soft_deleted {
            builder = builder.filter(RecordSchema::takedownRef.is_null());
        }
        if let Some(cid) = cid {
            builder = builder.filter(RecordSchema::cid.eq(cid));
        }
        let record: Option<(models::Record, models::RepoBlock)> = builder.first(conn).optional()?;
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

    pub fn has_record(
        &mut self,
        conn: &mut PgConnection,
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
        let record_uri = builder.first::<String>(conn).optional()?;
        Ok(!!record_uri.is_some())
    }

    pub fn get_record_takedown_status(
        &mut self,
        conn: &mut PgConnection,
        uri: String,
    ) -> Result<Option<StatusAttr>> {
        use crate::schema::pds::record::dsl as RecordSchema;
        let res = RecordSchema::record
            .select(RecordSchema::takedownRef)
            .filter(RecordSchema::uri.eq(uri))
            .first::<Option<String>>(conn)
            .optional()?;
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

    pub fn get_current_record_cid(
        &mut self,
        conn: &mut PgConnection,
        uri: String,
    ) -> Result<Option<Cid>> {
        use crate::schema::pds::record::dsl as RecordSchema;
        let res = RecordSchema::record
            .select(RecordSchema::cid)
            .filter(RecordSchema::uri.eq(uri))
            .first::<String>(conn)
            .optional()?;
        if let Some(res) = res {
            Ok(Some(Cid::from_str(&res)?))
        } else {
            Ok(None)
        }
    }

    pub fn get_record_backlinks(
        &mut self,
        conn: &mut PgConnection,
        collection: String,
        path: String,
        link_to: String,
    ) -> Result<Vec<models::Record>> {
        use crate::schema::pds::backlink::dsl as BacklinkSchema;
        use crate::schema::pds::record::dsl as RecordSchema;
        let res = RecordSchema::record
            .inner_join(BacklinkSchema::backlink.on(BacklinkSchema::uri.eq(RecordSchema::uri)))
            .select(models::Record::as_select())
            .filter(BacklinkSchema::path.eq(path))
            .filter(BacklinkSchema::linkTo.eq(link_to))
            .filter(RecordSchema::collection.eq(collection))
            .load::<models::Record>(conn)?;
        Ok(res)
    }

    // TO DO: Update to use AtUri
    pub fn get_backlink_conflicts(
        &mut self,
        conn: &mut PgConnection,
        uri: String,
        record: RepoRecord,
    ) -> Result<Vec<String>> {
        let record_backlinks = get_backlinks(uri, record)?;
        let collection = *uri.split("/").collect::<Vec<&str>>().get(0).unwrap_or(&"");
        let conflicts: Vec<Vec<models::Record>> = record_backlinks
            .into_iter()
            .map(|backlink| {
                Ok(self.get_record_backlinks(
                    conn,
                    collection.to_owned(),
                    backlink.path,
                    backlink.link_to,
                )?)
            })
            .collect::<Result<Vec<Vec<models::Record>>>>()?;
        Ok(conflicts
            .into_iter()
            .flatten()
            .map(|record| {
                format!(
                    "at://{0}/{1}/{2}",
                    env::var("HOSTNAME").unwrap(),
                    collection,
                    record.rkey
                )
            })
            .collect())
    }

    // Transactors
    // -------------------

    pub fn index_record(
        &mut self,
        uri: String, // TO DO: Use AtUri
        cid: Cid,
        record: Option<RepoRecord>,
        action: Option<WriteOpAction>, // Create or update with a default of create
        repo_rev: String,
        timestamp: String,
    ) -> Result<()> {
        todo!()
    }

    pub fn delete_record(
        &mut self,
        uri: String, // TO DO: Use AtUri
    ) -> Result<()> {
        todo!()
    }
}
