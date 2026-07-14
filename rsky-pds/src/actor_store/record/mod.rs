use crate::actor_store::db::{ActorDb, Backlink, Record};
use anyhow::{bail, Result};
use lexicon_cid::Cid;
use rsky_common;
use rsky_lexicon::com::atproto::admin::StatusAttr;
use rsky_repo::storage::Ipld;
use rsky_repo::types::{Ids, Lex, RepoRecord, WriteOpAction};
use rsky_repo::util::cbor_to_lex_record;
use rsky_syntax::aturi::AtUri;
use rsky_syntax::aturi_validation::ensure_valid_at_uri;
use rsky_syntax::did::ensure_valid_did;
use rusqlite::OptionalExtension;
use serde_json::Value as JsonValue;
use std::str::FromStr;

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

#[derive(Debug, Clone, PartialEq)]
pub struct RecordSinceRev {
    pub uri: String,
    pub cid: String,
    pub indexed_at: String,
    pub content: Vec<u8>,
}

fn lex_str(value: Option<&Lex>) -> Option<&String> {
    match value {
        Some(Lex::Ipld(Ipld::String(value))) => Some(value),
        Some(Lex::Ipld(Ipld::Json(JsonValue::String(value)))) => Some(value),
        _ => None,
    }
}

fn ipld_str(value: Option<&Ipld>) -> Option<&String> {
    match value {
        Some(Ipld::String(value)) => Some(value),
        Some(Ipld::Json(JsonValue::String(value))) => Some(value),
        _ => None,
    }
}

fn subject_uri(record: &RepoRecord) -> Option<&String> {
    match record.get("subject") {
        Some(Lex::Map(ref_object)) => lex_str(ref_object.get("uri")),
        Some(Lex::Ipld(Ipld::Map(ref_object))) => ipld_str(ref_object.get("uri")),
        _ => None,
    }
}

// @NOTE in the future this can be replaced with a more generic routine that pulls backlinks based on lex docs.
// For now, we just want to ensure we're tracking links from follows, blocks, likes, and reposts.
pub fn get_backlinks(uri: &AtUri, record: &RepoRecord) -> Result<Vec<Backlink>> {
    if let Some(record_type) = lex_str(record.get("$type")) {
        if record_type == Ids::AppBskyGraphFollow.as_str()
            || record_type == Ids::AppBskyGraphBlock.as_str()
        {
            if let Some(subject) = lex_str(record.get("subject")) {
                match ensure_valid_did(subject) {
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
            if let Some(subject_uri) = subject_uri(record) {
                match ensure_valid_at_uri(subject_uri) {
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
    Ok(Vec::new())
}

type RecordRowWithContent = (String, String, String, Option<String>, Vec<u8>);

fn record_from_row(row: &rusqlite::Row) -> Result<Record, rusqlite::Error> {
    Ok(Record {
        uri: row.get("uri")?,
        cid: row.get("cid")?,
        collection: row.get("collection")?,
        rkey: row.get("rkey")?,
        repo_rev: row.get("repoRev")?,
        indexed_at: row.get("indexedAt")?,
        takedown_ref: row.get("takedownRef")?,
    })
}

pub struct RecordReader {
    pub did: String,
    pub db: ActorDb,
}

// Handles getting lexicon records from the per-actor db
impl RecordReader {
    pub fn new(did: String, db: ActorDb) -> Self {
        RecordReader { did, db }
    }

    pub async fn record_count(&mut self) -> Result<i64> {
        self.db
            .run(|conn| Ok(conn.query_row("SELECT count(*) FROM record", [], |row| row.get(0))?))
            .await
    }

    pub async fn list_collections(&mut self) -> Result<Vec<String>> {
        self.db
            .run(|conn| {
                let mut stmt = conn.prepare("SELECT collection FROM record GROUP BY collection")?;
                let collections = stmt
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<String>, rusqlite::Error>>()?;
                Ok(collections)
            })
            .await
    }

    #[allow(clippy::too_many_arguments)]
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
        let include_soft_deleted: bool = include_soft_deleted.unwrap_or_default();
        let rows: Vec<(String, String, Vec<u8>)> = self
            .db
            .run(move |conn| {
                let mut sql = String::from(
                    "SELECT record.uri, record.cid, repo_block.content FROM record \
                     INNER JOIN repo_block ON repo_block.cid = record.cid \
                     WHERE record.collection = ?",
                );
                let mut params: Vec<Box<dyn rusqlite::types::ToSql>> =
                    vec![Box::new(collection.clone())];
                if !include_soft_deleted {
                    sql.push_str(" AND record.\"takedownRef\" IS NULL");
                }
                // prioritize cursor but fall back to soon-to-be-deprecated rkey start/end
                if let Some(cursor) = &cursor {
                    if reverse {
                        sql.push_str(" AND record.rkey > ?");
                    } else {
                        sql.push_str(" AND record.rkey < ?");
                    }
                    params.push(Box::new(cursor.clone()));
                } else {
                    if let Some(rkey_start) = &rkey_start {
                        sql.push_str(" AND record.rkey > ?");
                        params.push(Box::new(rkey_start.clone()));
                    }
                    if let Some(rkey_end) = &rkey_end {
                        sql.push_str(" AND record.rkey < ?");
                        params.push(Box::new(rkey_end.clone()));
                    }
                }
                if reverse {
                    sql.push_str(" ORDER BY record.rkey ASC");
                } else {
                    sql.push_str(" ORDER BY record.rkey DESC");
                }
                sql.push_str(" LIMIT ?");
                params.push(Box::new(limit));
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt
                    .query_map(
                        rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )?
                    .collect::<Result<Vec<(String, String, Vec<u8>)>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await?;
        rows.into_iter()
            .map(|(uri, cid, content)| {
                Ok(RecordsForCollection {
                    uri,
                    cid,
                    value: cbor_to_lex_record(content)?,
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
        let include_soft_deleted: bool = include_soft_deleted.unwrap_or_default();
        let uri = uri.to_string();
        let record: Option<RecordRowWithContent> = self
            .db
            .run(move |conn| {
                let mut sql = String::from(
                    "SELECT record.uri, record.cid, record.\"indexedAt\", record.\"takedownRef\", \
                     repo_block.content FROM record \
                     INNER JOIN repo_block ON repo_block.cid = record.cid \
                     WHERE record.uri = ?",
                );
                let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(uri.clone())];
                if !include_soft_deleted {
                    sql.push_str(" AND record.\"takedownRef\" IS NULL");
                }
                if let Some(cid) = &cid {
                    sql.push_str(" AND record.cid = ?");
                    params.push(Box::new(cid.clone()));
                }
                let mut stmt = conn.prepare(&sql)?;
                Ok(stmt
                    .query_row(
                        rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
                        |row| {
                            Ok((
                                row.get(0)?,
                                row.get(1)?,
                                row.get(2)?,
                                row.get(3)?,
                                row.get(4)?,
                            ))
                        },
                    )
                    .optional()?)
            })
            .await?;
        match record {
            Some((uri, cid, indexed_at, takedown_ref, content)) => Ok(Some(GetRecord {
                uri,
                cid,
                value: cbor_to_lex_record(content)?,
                indexed_at,
                takedown_ref,
            })),
            None => Ok(None),
        }
    }

    pub async fn has_record(
        &mut self,
        uri: String,
        cid: Option<String>,
        include_soft_deleted: Option<bool>,
    ) -> Result<bool> {
        let include_soft_deleted: bool = include_soft_deleted.unwrap_or_default();
        let record_uri: Option<String> = self
            .db
            .run(move |conn| {
                let mut sql = String::from("SELECT uri FROM record WHERE uri = ?");
                let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(uri.clone())];
                if !include_soft_deleted {
                    sql.push_str(" AND \"takedownRef\" IS NULL");
                }
                if let Some(cid) = &cid {
                    sql.push_str(" AND cid = ?");
                    params.push(Box::new(cid.clone()));
                }
                let mut stmt = conn.prepare(&sql)?;
                Ok(stmt
                    .query_row(
                        rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
                        |row| row.get(0),
                    )
                    .optional()?)
            })
            .await?;
        Ok(record_uri.is_some())
    }

    pub async fn get_record_takedown_status(&self, uri: String) -> Result<Option<StatusAttr>> {
        let res: Option<Option<String>> = self
            .db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT \"takedownRef\" FROM record WHERE uri = ?1",
                        [uri.clone()],
                        |row| row.get(0),
                    )
                    .optional()?)
            })
            .await?;
        match res {
            None => Ok(None),
            Some(Some(takedown_ref)) => Ok(Some(StatusAttr {
                applied: true,
                r#ref: Some(takedown_ref),
            })),
            Some(None) => Ok(Some(StatusAttr {
                applied: false,
                r#ref: None,
            })),
        }
    }

    pub async fn get_current_record_cid(&self, uri: String) -> Result<Option<Cid>> {
        let res: Option<String> = self
            .db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT cid FROM record WHERE uri = ?1",
                        [uri.clone()],
                        |row| row.get(0),
                    )
                    .optional()?)
            })
            .await?;
        match res {
            Some(res) => Ok(Some(Cid::from_str(&res)?)),
            None => Ok(None),
        }
    }

    pub async fn get_record_backlinks(
        &self,
        collection: String,
        path: String,
        link_to: String,
    ) -> Result<Vec<Record>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT record.* FROM record \
                     INNER JOIN backlink ON backlink.uri = record.uri \
                     WHERE backlink.path = ?1 AND backlink.\"linkTo\" = ?2 \
                     AND record.collection = ?3",
                )?;
                let rows = stmt
                    .query_map(
                        rusqlite::params![path, link_to, collection],
                        record_from_row,
                    )?
                    .collect::<Result<Vec<Record>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await
    }

    pub async fn get_backlink_conflicts(
        &self,
        uri: &AtUri,
        record: &RepoRecord,
    ) -> Result<Vec<AtUri>> {
        let record_backlinks = get_backlinks(uri, record)?;
        let mut conflicts: Vec<AtUri> = Vec::new();
        for backlink in record_backlinks {
            let records = self
                .get_record_backlinks(uri.get_collection(), backlink.path, backlink.link_to)
                .await?;
            for record in records {
                conflicts.push(AtUri::make(
                    uri.get_hostname().to_string(),
                    Some(uri.get_collection()),
                    Some(record.rkey),
                )?);
            }
        }
        Ok(conflicts)
    }

    pub async fn get_profile_record(&self) -> Result<Option<Vec<u8>>> {
        self.db
            .run(|conn| {
                Ok(conn
                    .query_row(
                        "SELECT repo_block.content FROM record \
                         LEFT JOIN repo_block ON repo_block.cid = record.cid \
                         WHERE record.collection = ?1 AND record.rkey = 'self'",
                        [Ids::AppBskyActorProfile.as_str()],
                        |row| row.get(0),
                    )
                    .optional()?)
            })
            .await
    }

    pub async fn get_records_since_rev(&self, rev: String) -> Result<Vec<RecordSinceRev>> {
        let rev_clone = rev.clone();
        let res: Vec<RecordSinceRev> = self
            .db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT record.uri, repo_block.cid, record.\"indexedAt\", repo_block.content \
                     FROM record \
                     INNER JOIN repo_block ON repo_block.cid = record.cid \
                     WHERE record.\"repoRev\" > ?1 \
                     ORDER BY record.\"repoRev\" ASC LIMIT 10",
                )?;
                let rows = stmt
                    .query_map([rev_clone.clone()], |row| {
                        Ok(RecordSinceRev {
                            uri: row.get(0)?,
                            cid: row.get(1)?,
                            indexed_at: row.get(2)?,
                            content: row.get(3)?,
                        })
                    })?
                    .collect::<Result<Vec<RecordSinceRev>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await?;
        if res.is_empty() {
            return Ok(res);
        }
        // sanity check to ensure that the clock received is not before _all_ local records
        // (for instance in case of account migration)
        let sanity_check: Option<String> = self
            .db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT uri FROM record WHERE \"repoRev\" <= ?1 LIMIT 1",
                        [rev.clone()],
                        |row| row.get(0),
                    )
                    .optional()?)
            })
            .await?;
        if sanity_check.is_none() {
            return Ok(Vec::new());
        }
        Ok(res)
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
        tracing::debug!("indexing record {uri}");

        let collection = uri.get_collection();
        let rkey = uri.get_rkey();
        let hostname = uri.get_hostname().to_string();
        let action = action.unwrap_or(WriteOpAction::Create);
        let indexed_at = timestamp.unwrap_or_else(rsky_common::now);

        if !hostname.starts_with("did:") {
            bail!("Expected indexed URI to contain DID")
        } else if collection.is_empty() {
            bail!("Expected indexed URI to contain a collection")
        } else if rkey.is_empty() {
            bail!("Expected indexed URI to contain a record key")
        }

        // Track current version of record
        let uri_string = uri.to_string();
        let cid_string = cid.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO record (uri, cid, collection, rkey, \"repoRev\", \"indexedAt\") \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
                     ON CONFLICT (uri) DO UPDATE SET \
                     cid = excluded.cid, \
                     \"repoRev\" = excluded.\"repoRev\", \
                     \"indexedAt\" = excluded.\"indexedAt\"",
                    rusqlite::params![
                        uri_string, cid_string, collection, rkey, repo_rev, indexed_at
                    ],
                )?;
                Ok(())
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
        tracing::debug!("indexed record {uri}");
        Ok(())
    }

    #[tracing::instrument(skip_all)]
    pub async fn delete_record(&self, uri: &AtUri) -> Result<()> {
        tracing::debug!("deleting indexed record {uri}");
        let uri = uri.to_string();
        self.db
            .run(move |conn| {
                conn.execute("DELETE FROM record WHERE uri = ?1", [uri.clone()])?;
                conn.execute("DELETE FROM backlink WHERE uri = ?1", [uri.clone()])?;
                Ok(())
            })
            .await
    }

    pub async fn remove_backlinks_by_uri(&self, uri: &AtUri) -> Result<()> {
        let uri = uri.to_string();
        self.db
            .run(move |conn| {
                conn.execute("DELETE FROM backlink WHERE uri = ?1", [uri.clone()])?;
                Ok(())
            })
            .await
    }

    pub async fn add_backlinks(&self, backlinks: Vec<Backlink>) -> Result<()> {
        if backlinks.is_empty() {
            return Ok(());
        }
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "INSERT INTO backlink (uri, path, \"linkTo\") \
                     VALUES (?1, ?2, ?3) ON CONFLICT DO NOTHING",
                )?;
                for backlink in &backlinks {
                    stmt.execute(rusqlite::params![
                        backlink.uri,
                        backlink.path,
                        backlink.link_to
                    ])?;
                }
                Ok(())
            })
            .await
    }

    pub async fn update_record_takedown_status(
        &self,
        uri: &AtUri,
        takedown: StatusAttr,
    ) -> Result<()> {
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
                conn.execute(
                    "UPDATE record SET \"takedownRef\" = ?1 WHERE uri = ?2",
                    rusqlite::params![takedown_ref, uri_string],
                )?;
                Ok(())
            })
            .await
    }

    pub async fn list_existing_blocks(&self) -> Result<Vec<Cid>> {
        let cids: Vec<String> = self
            .db
            .run(|conn| {
                let mut stmt = conn.prepare("SELECT cid FROM repo_block ORDER BY cid ASC")?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<String>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await?;
        cids.into_iter()
            .map(|cid| Cid::from_str(&cid).map_err(anyhow::Error::new))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor_store::db::get_migrated_db;
    use rsky_common::ipld::cid_for_cbor;

    async fn test_reader() -> (tempfile::TempDir, RecordReader) {
        let dir = tempfile::tempdir().unwrap();
        let db = get_migrated_db(dir.path().join("store.sqlite"))
            .await
            .unwrap();
        let reader = RecordReader::new("did:example:alice".to_owned(), db);
        (dir, reader)
    }

    fn record_from_json(value: serde_json::Value) -> RepoRecord {
        serde_json::from_value(value).unwrap()
    }

    fn post_record(text: &str) -> RepoRecord {
        record_from_json(serde_json::json!({
            "$type": "app.bsky.feed.post",
            "text": text,
            "createdAt": "2023-01-01T00:00:00.000Z",
        }))
    }

    fn follow_record(subject: &str) -> RepoRecord {
        record_from_json(serde_json::json!({
            "$type": "app.bsky.graph.follow",
            "subject": subject,
            "createdAt": "2023-01-01T00:00:00.000Z",
        }))
    }

    fn like_record(subject_uri: &str) -> RepoRecord {
        record_from_json(serde_json::json!({
            "$type": "app.bsky.feed.like",
            "subject": { "uri": subject_uri, "cid": "bafkreibjfgx2gprinfvicegelk5kosd6y2frmqpqzwqkg7usac74l3t2v4" },
            "createdAt": "2023-01-01T00:00:00.000Z",
        }))
    }

    async fn put_block(reader: &RecordReader, record: &RepoRecord, rev: &str) -> Cid {
        let cid = cid_for_cbor(record).unwrap();
        let content = serde_ipld_dagcbor::to_vec(record).unwrap();
        let cid_string = cid.to_string();
        let rev = rev.to_owned();
        reader
            .db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO repo_block (cid, \"repoRev\", size, content) \
                     VALUES (?1, ?2, ?3, ?4) ON CONFLICT DO NOTHING",
                    rusqlite::params![cid_string, rev, content.len() as i64, content],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        cid
    }

    fn at_uri(uri: &str) -> AtUri {
        AtUri::new(uri.to_owned(), None).unwrap()
    }

    async fn index_with_block(
        reader: &RecordReader,
        uri: &str,
        record: RepoRecord,
        rev: &str,
    ) -> Cid {
        let cid = put_block(reader, &record, rev).await;
        reader
            .index_record(
                at_uri(uri),
                cid,
                Some(record),
                Some(WriteOpAction::Create),
                rev.to_owned(),
                Some("2023-01-01T00:00:00.000Z".to_owned()),
            )
            .await
            .unwrap();
        cid
    }

    #[tokio::test]
    async fn index_get_and_delete_record() {
        let (_dir, mut reader) = test_reader().await;
        let uri = "at://did:example:alice/app.bsky.feed.post/3jt5vlkorlk2v";
        let record = post_record("hello");
        let cid = index_with_block(&reader, uri, record.clone(), "rev-1").await;

        assert_eq!(reader.record_count().await.unwrap(), 1);
        assert_eq!(
            reader.list_collections().await.unwrap(),
            ["app.bsky.feed.post"]
        );
        let got = reader
            .get_record(&at_uri(uri), None, None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.uri, uri);
        assert_eq!(got.cid, cid.to_string());
        assert_eq!(got.value, record);
        assert!(got.takedown_ref.is_none());
        // filter by cid
        assert!(reader
            .get_record(&at_uri(uri), Some(cid.to_string()), None)
            .await
            .unwrap()
            .is_some());
        assert!(reader
            .get_record(
                &at_uri(uri),
                Some("bafkreibjfgx2gprinfvicegelk5kosd6y2frmqpqzwqkg7usac74l3t2v4".to_owned()),
                None
            )
            .await
            .unwrap()
            .is_none());
        assert!(reader.has_record(uri.to_owned(), None, None).await.unwrap());
        assert!(reader
            .has_record(uri.to_owned(), Some(cid.to_string()), None)
            .await
            .unwrap());
        assert_eq!(
            reader.get_current_record_cid(uri.to_owned()).await.unwrap(),
            Some(cid)
        );
        assert_eq!(reader.list_existing_blocks().await.unwrap(), vec![cid]);

        reader.delete_record(&at_uri(uri)).await.unwrap();
        assert_eq!(reader.record_count().await.unwrap(), 0);
        assert!(reader
            .get_record(&at_uri(uri), None, None)
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            reader.get_current_record_cid(uri.to_owned()).await.unwrap(),
            None
        );
        assert!(reader
            .get_record_takedown_status(uri.to_owned())
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn index_record_validates_uri() {
        let (_dir, reader) = test_reader().await;
        let record = post_record("hi");
        let cid = cid_for_cbor(&record).unwrap();
        let bad_host = AtUri::make(
            "example.com".to_owned(),
            Some("app.bsky.feed.post".to_owned()),
            Some("3jt5vlkorlk2v".to_owned()),
        )
        .unwrap();
        let res = reader
            .index_record(
                bad_host,
                cid,
                Some(record.clone()),
                None,
                "rev-1".to_owned(),
                None,
            )
            .await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn takedown_status_round_trip() {
        let (_dir, mut reader) = test_reader().await;
        let uri = "at://did:example:alice/app.bsky.feed.post/3jt5vlkorlk2v";
        index_with_block(&reader, uri, post_record("hello"), "rev-1").await;

        let status = reader
            .get_record_takedown_status(uri.to_owned())
            .await
            .unwrap()
            .unwrap();
        assert!(!status.applied);

        reader
            .update_record_takedown_status(
                &at_uri(uri),
                StatusAttr {
                    applied: true,
                    r#ref: Some("takedown-ref".to_owned()),
                },
            )
            .await
            .unwrap();
        let status = reader
            .get_record_takedown_status(uri.to_owned())
            .await
            .unwrap()
            .unwrap();
        assert!(status.applied);
        assert_eq!(status.r#ref.as_deref(), Some("takedown-ref"));

        // soft-deleted records are hidden unless requested
        assert!(reader
            .get_record(&at_uri(uri), None, None)
            .await
            .unwrap()
            .is_none());
        assert!(!reader.has_record(uri.to_owned(), None, None).await.unwrap());
        assert!(reader
            .get_record(&at_uri(uri), None, Some(true))
            .await
            .unwrap()
            .is_some());
        assert!(reader
            .has_record(uri.to_owned(), None, Some(true))
            .await
            .unwrap());

        // takedown applied without explicit ref defaults to a timestamp
        reader
            .update_record_takedown_status(
                &at_uri(uri),
                StatusAttr {
                    applied: true,
                    r#ref: None,
                },
            )
            .await
            .unwrap();
        let status = reader
            .get_record_takedown_status(uri.to_owned())
            .await
            .unwrap()
            .unwrap();
        assert!(status.applied && status.r#ref.is_some());

        reader
            .update_record_takedown_status(
                &at_uri(uri),
                StatusAttr {
                    applied: false,
                    r#ref: None,
                },
            )
            .await
            .unwrap();
        assert!(reader
            .get_record(&at_uri(uri), None, None)
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn lists_records_for_collection_with_pagination() {
        let (_dir, mut reader) = test_reader().await;
        for (i, rkey) in ["3jt5vlkoraa2a", "3jt5vlkorbb2b", "3jt5vlkorcc2c"]
            .iter()
            .enumerate()
        {
            let uri = format!("at://did:example:alice/app.bsky.feed.post/{rkey}");
            index_with_block(&reader, &uri, post_record(&format!("post {i}")), "rev-1").await;
        }

        let forward = reader
            .list_records_for_collection(
                "app.bsky.feed.post".to_owned(),
                10,
                true,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(forward.len(), 3);
        assert!(forward[0].uri.ends_with("3jt5vlkoraa2a"));

        let backward = reader
            .list_records_for_collection(
                "app.bsky.feed.post".to_owned(),
                10,
                false,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();
        assert!(backward[0].uri.ends_with("3jt5vlkorcc2c"));

        let paged = reader
            .list_records_for_collection(
                "app.bsky.feed.post".to_owned(),
                10,
                true,
                Some("3jt5vlkoraa2a".to_owned()),
                None,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(paged.len(), 2);

        let paged_desc = reader
            .list_records_for_collection(
                "app.bsky.feed.post".to_owned(),
                10,
                false,
                Some("3jt5vlkorcc2c".to_owned()),
                None,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(paged_desc.len(), 2);

        let ranged = reader
            .list_records_for_collection(
                "app.bsky.feed.post".to_owned(),
                10,
                true,
                None,
                Some("3jt5vlkoraa2a".to_owned()),
                Some("3jt5vlkorcc2c".to_owned()),
                None,
            )
            .await
            .unwrap();
        assert_eq!(ranged.len(), 1);
        assert!(ranged[0].uri.ends_with("3jt5vlkorbb2b"));

        let limited = reader
            .list_records_for_collection(
                "app.bsky.feed.post".to_owned(),
                2,
                true,
                None,
                None,
                None,
                Some(true),
            )
            .await
            .unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[tokio::test]
    async fn maintains_backlinks_for_follows_and_likes() {
        let (_dir, reader) = test_reader().await;
        let follow_uri = "at://did:example:alice/app.bsky.graph.follow/3jt5vlkoraa2a";
        index_with_block(
            &reader,
            follow_uri,
            follow_record("did:example:bob"),
            "rev-1",
        )
        .await;
        let like_uri = "at://did:example:alice/app.bsky.feed.like/3jt5vlkorbb2b";
        let liked_post = "at://did:example:carol/app.bsky.feed.post/3jt5vlkorcc2c";
        index_with_block(&reader, like_uri, like_record(liked_post), "rev-1").await;

        let follow_backlinks = reader
            .get_record_backlinks(
                "app.bsky.graph.follow".to_owned(),
                "subject".to_owned(),
                "did:example:bob".to_owned(),
            )
            .await
            .unwrap();
        assert_eq!(follow_backlinks.len(), 1);
        assert_eq!(follow_backlinks[0].uri, follow_uri);

        let conflicts = reader
            .get_backlink_conflicts(
                &at_uri("at://did:example:alice/app.bsky.graph.follow/3jt5vlkorxx2x"),
                &follow_record("did:example:bob"),
            )
            .await
            .unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].get_rkey(), "3jt5vlkoraa2a");

        let like_conflicts = reader
            .get_backlink_conflicts(
                &at_uri("at://did:example:alice/app.bsky.feed.like/3jt5vlkoryy2y"),
                &like_record(liked_post),
            )
            .await
            .unwrap();
        assert_eq!(like_conflicts.len(), 1);

        // plain posts produce no backlinks
        assert!(get_backlinks(
            &at_uri("at://did:example:alice/app.bsky.feed.post/3jt5vlkorzz2z"),
            &post_record("no links")
        )
        .unwrap()
        .is_empty());

        // updating a follow re-points its backlink
        let updated = follow_record("did:example:dan");
        let updated_cid = put_block(&reader, &updated, "rev-2").await;
        reader
            .index_record(
                at_uri(follow_uri),
                updated_cid,
                Some(updated),
                Some(WriteOpAction::Update),
                "rev-2".to_owned(),
                None,
            )
            .await
            .unwrap();
        assert!(reader
            .get_record_backlinks(
                "app.bsky.graph.follow".to_owned(),
                "subject".to_owned(),
                "did:example:bob".to_owned(),
            )
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            reader
                .get_record_backlinks(
                    "app.bsky.graph.follow".to_owned(),
                    "subject".to_owned(),
                    "did:example:dan".to_owned(),
                )
                .await
                .unwrap()
                .len(),
            1
        );

        // deleting the record removes its backlinks
        reader.delete_record(&at_uri(follow_uri)).await.unwrap();
        assert!(reader
            .get_record_backlinks(
                "app.bsky.graph.follow".to_owned(),
                "subject".to_owned(),
                "did:example:dan".to_owned(),
            )
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn rejects_backlinks_with_invalid_subjects() {
        let follow = follow_record("not-a-did");
        let uri = at_uri("at://did:example:alice/app.bsky.graph.follow/3jt5vlkoraa2a");
        assert!(get_backlinks(&uri, &follow).is_err());

        let like = like_record("not-an-at-uri");
        let like_uri = at_uri("at://did:example:alice/app.bsky.feed.like/3jt5vlkoraa2a");
        assert!(get_backlinks(&like_uri, &like).is_err());
    }

    #[tokio::test]
    async fn records_since_rev_with_sanity_check() {
        let (_dir, reader) = test_reader().await;
        // no records at all
        assert!(reader
            .get_records_since_rev("rev-0".to_owned())
            .await
            .unwrap()
            .is_empty());

        let uri = "at://did:example:alice/app.bsky.feed.post/3jt5vlkoraa2a";
        index_with_block(&reader, uri, post_record("first"), "rev-2").await;

        // rev before all local records: sanity check kicks in
        assert!(reader
            .get_records_since_rev("rev-0".to_owned())
            .await
            .unwrap()
            .is_empty());

        let uri_two = "at://did:example:alice/app.bsky.feed.post/3jt5vlkorbb2b";
        index_with_block(&reader, uri_two, post_record("second"), "rev-3").await;
        let since = reader
            .get_records_since_rev("rev-2".to_owned())
            .await
            .unwrap();
        assert_eq!(since.len(), 1);
        assert_eq!(since[0].uri, uri_two);
        assert!(!since[0].content.is_empty());
    }

    #[tokio::test]
    async fn gets_profile_record() {
        let (_dir, reader) = test_reader().await;
        assert!(reader.get_profile_record().await.unwrap().is_none());
        let profile = record_from_json(serde_json::json!({
            "$type": "app.bsky.actor.profile",
            "displayName": "Alice",
        }));
        index_with_block(
            &reader,
            "at://did:example:alice/app.bsky.actor.profile/self",
            profile,
            "rev-1",
        )
        .await;
        assert!(reader.get_profile_record().await.unwrap().is_some());
    }
}
