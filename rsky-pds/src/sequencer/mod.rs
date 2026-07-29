use crate::account_manager::helpers::account::AccountStatus;
use crate::actor_store::repo::types::SyncEvtData;
use crate::crawlers::Crawlers;
use crate::db::sqlite::Db;
use crate::models;
use crate::sequencer::events::{
    format_seq_account_evt, format_seq_commit, format_seq_handle_update, format_seq_identity_evt,
    SeqEvt, TypedAccountEvt, TypedCommitEvt, TypedIdentityEvt, TypedSyncEvt,
};
use crate::EVENT_EMITTER;
use anyhow::Result;
use events::format_seq_sync_evt;
use rsky_common::cbor_to_struct;
use rsky_repo::types::CommitDataWithOps;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, OptionalExtension, Row};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

pub struct RequestSeqRangeOpts {
    pub earliest_seq: Option<i64>,
    pub latest_seq: Option<i64>,
    pub earliest_time: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Sequencer {
    pub db: Db,
    pub crawlers: Crawlers,
    pub last_seen: Option<i64>,
    destroyed: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

const SELECT_REPO_SEQ: &str = "\
    SELECT seq, did, \"eventType\", event, invalidated, \"sequencedAt\" FROM repo_seq";

fn repo_seq_from_row(row: &Row) -> Result<models::RepoSeq, rusqlite::Error> {
    Ok(models::RepoSeq {
        seq: row.get(0)?,
        did: row.get(1)?,
        event_type: row.get(2)?,
        event: row.get(3)?,
        invalidated: row.get(4)?,
        sequenced_at: row.get(5)?,
    })
}

impl Sequencer {
    pub fn new(db: Db, crawlers: Crawlers, last_seen: Option<i64>) -> Self {
        Sequencer {
            db,
            crawlers,
            last_seen: Some(last_seen.unwrap_or(0)),
            destroyed: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed.load(Ordering::SeqCst)
    }

    /// Polls the sequencer db for newly sequenced events and emits them.
    /// Sleeps on a notification handle between polls rather than busy-polling.
    pub async fn start(&mut self) -> Result<()> {
        let curr = self.curr().await?;
        self.last_seen = Some(curr.unwrap_or(0));
        while !self.is_destroyed() {
            // arm the notification before polling so sequencing that lands
            // mid-poll is never missed
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            match self
                .request_seq_range(RequestSeqRangeOpts {
                    earliest_seq: self.last_seen,
                    latest_seq: None,
                    earliest_time: None,
                    limit: Some(1000),
                })
                .await
            {
                Err(err) => {
                    tracing::error!(
                        "sequencer failed to poll db, err: {}, last_seen: {:?}",
                        err.to_string(),
                        self.last_seen
                    );
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Ok(evts) if !evts.is_empty() => {
                    self.last_seen = evts.last().map(|evt| evt.seq()).or(self.last_seen);
                    EVENT_EMITTER.write().await.emit(
                        "events",
                        evts.iter()
                            .map(|evt| serde_json::to_string(evt).unwrap())
                            .collect::<Vec<String>>(),
                    );
                }
                Ok(_) => {
                    tokio::select! {
                        _ = notified => {},
                        _ = tokio::time::sleep(Duration::from_secs(1)) => {},
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn destroy(&mut self) {
        self.destroyed.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
        EVENT_EMITTER.write().await.emit("close", ());
    }

    pub async fn curr(&self) -> Result<Option<i64>> {
        self.db
            .run(|conn| {
                Ok(conn
                    .query_row(
                        "SELECT seq FROM repo_seq ORDER BY seq DESC LIMIT 1",
                        [],
                        |row| row.get(0),
                    )
                    .optional()?)
            })
            .await
    }

    pub async fn next_seq(&self, cursor: i64) -> Result<Option<models::RepoSeq>> {
        self.db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        &format!("{SELECT_REPO_SEQ} WHERE seq > ?1 ORDER BY seq ASC LIMIT 1"),
                        params![cursor],
                        repo_seq_from_row,
                    )
                    .optional()?)
            })
            .await
    }

    pub async fn earliest_after_time(&self, time: String) -> Result<Option<models::RepoSeq>> {
        self.db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        &format!(
                            "{SELECT_REPO_SEQ} WHERE \"sequencedAt\" >= ?1 \
                             ORDER BY \"sequencedAt\" ASC LIMIT 1"
                        ),
                        params![time],
                        repo_seq_from_row,
                    )
                    .optional()?)
            })
            .await
    }

    pub async fn request_seq_range(&self, opts: RequestSeqRangeOpts) -> Result<Vec<SeqEvt>> {
        let RequestSeqRangeOpts {
            earliest_seq,
            latest_seq,
            earliest_time,
            limit,
        } = opts;

        let rows = self
            .db
            .run(move |conn| {
                let mut sql = format!("{SELECT_REPO_SEQ} WHERE invalidated = 0");
                let mut sql_params: Vec<SqlValue> = Vec::new();
                if let Some(earliest_seq) = earliest_seq {
                    sql.push_str(&format!(" AND seq > ?{}", sql_params.len() + 1));
                    sql_params.push(SqlValue::Integer(earliest_seq));
                }
                if let Some(latest_seq) = latest_seq {
                    sql.push_str(&format!(" AND seq <= ?{}", sql_params.len() + 1));
                    sql_params.push(SqlValue::Integer(latest_seq));
                }
                if let Some(ref earliest_time) = earliest_time {
                    sql.push_str(&format!(
                        " AND \"sequencedAt\" >= ?{}",
                        sql_params.len() + 1
                    ));
                    sql_params.push(SqlValue::Text(earliest_time.clone()));
                }
                sql.push_str(" ORDER BY seq ASC");
                if let Some(limit) = limit {
                    sql.push_str(&format!(" LIMIT ?{}", sql_params.len() + 1));
                    sql_params.push(SqlValue::Integer(limit));
                }
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt
                    .query_map(params_from_iter(sql_params), repo_seq_from_row)?
                    .collect::<Result<Vec<models::RepoSeq>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await?;

        let mut seq_evts: Vec<SeqEvt> = Vec::new();
        for row in rows {
            let time = row.sequenced_at;
            match row.seq {
                None => continue, // should never hit this because of the primary key
                Some(seq) => match row.event_type.as_str() {
                    "append" | "rebase" => {
                        seq_evts.push(SeqEvt::TypedCommitEvt(Box::new(TypedCommitEvt {
                            r#type: "commit".to_string(),
                            seq,
                            time,
                            evt: cbor_to_struct(row.event)?,
                        })));
                    }
                    "sync" => {
                        seq_evts.push(SeqEvt::TypedSyncEvt(TypedSyncEvt {
                            r#type: "sync".to_string(),
                            seq,
                            time,
                            evt: cbor_to_struct(row.event)?,
                        }));
                    }
                    "identity" => {
                        seq_evts.push(SeqEvt::TypedIdentityEvt(TypedIdentityEvt {
                            r#type: "identity".to_string(),
                            seq,
                            time,
                            evt: cbor_to_struct(row.event)?,
                        }));
                    }
                    "account" => {
                        seq_evts.push(SeqEvt::TypedAccountEvt(TypedAccountEvt {
                            r#type: "account".to_string(),
                            seq,
                            time,
                            evt: cbor_to_struct(row.event)?,
                        }));
                    }
                    _ => {
                        tracing::error!("request_seq_range invalid event type");
                    }
                },
            }
        }

        Ok(seq_evts)
    }

    pub async fn sequence_evt(&mut self, evt: models::RepoSeq) -> Result<i64> {
        let seq = self
            .db
            .run(move |conn| {
                Ok(conn.query_row(
                    "INSERT INTO repo_seq (did, event, \"eventType\", \"sequencedAt\") \
                     VALUES (?1, ?2, ?3, ?4) \
                     RETURNING seq",
                    params![evt.did, evt.event, evt.event_type, evt.sequenced_at],
                    |row| row.get::<_, i64>(0),
                )?)
            })
            .await?;
        self.crawlers.notify_of_update().await?;
        self.notify.notify_waiters();
        Ok(seq)
    }

    pub async fn sequence_commit(
        &mut self,
        did: String,
        commit_data: CommitDataWithOps,
    ) -> Result<i64> {
        let evt = format_seq_commit(did, commit_data).await?;
        self.sequence_evt(evt).await
    }

    pub async fn sequence_handle_update(&mut self, did: String, handle: String) -> Result<i64> {
        let evt = format_seq_handle_update(did, handle).await?;
        self.sequence_evt(evt).await
    }

    pub async fn sequence_identity_evt(
        &mut self,
        did: String,
        handle: Option<String>,
    ) -> Result<i64> {
        let evt = format_seq_identity_evt(did, handle).await?;
        self.sequence_evt(evt).await
    }

    pub async fn sequence_account_evt(
        &mut self,
        did: String,
        status: AccountStatus,
    ) -> Result<i64> {
        let evt = format_seq_account_evt(did, status).await?;
        self.sequence_evt(evt).await
    }

    pub async fn sequence_sync_evt(&mut self, did: String, data: SyncEvtData) -> Result<i64> {
        let evt = format_seq_sync_evt(did, data).await?;
        self.sequence_evt(evt).await
    }

    pub async fn delete_all_for_user(
        &self,
        did: &str,
        excluding_seqs: Option<Vec<i64>>,
    ) -> Result<()> {
        let did = did.to_owned();
        let excluding_seqs = excluding_seqs.unwrap_or_default();
        self.db
            .run(move |conn| {
                let mut sql = "DELETE FROM repo_seq WHERE did = ?1".to_string();
                let mut sql_params: Vec<SqlValue> = vec![SqlValue::Text(did.clone())];
                if !excluding_seqs.is_empty() {
                    sql.push_str(&format!(
                        " AND seq NOT IN ({})",
                        (0..excluding_seqs.len())
                            .map(|idx| format!("?{}", idx + 2))
                            .collect::<Vec<String>>()
                            .join(", ")
                    ));
                    sql_params.extend(excluding_seqs.iter().map(|seq| SqlValue::Integer(*seq)));
                }
                conn.execute(&sql, params_from_iter(sql_params))?;
                Ok(())
            })
            .await
    }
}

pub mod db;
pub mod events;
pub mod outbox;

#[cfg(test)]
mod tests;
