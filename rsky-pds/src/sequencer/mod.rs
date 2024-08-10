use crate::account_manager::helpers::account::AccountStatus;
use crate::crawlers::Crawlers;
use crate::db::establish_connection;
use crate::models;
use crate::repo::types::{CommitData, PreparedWrite};
use crate::sequencer::events::{
    format_seq_account_evt, format_seq_commit, format_seq_handle_update, format_seq_identity_evt,
    format_seq_tombstone, AccountEvt, CommitEvt, HandleEvt, IdentityEvt, SeqEvt, TombstoneEvt,
    TypedAccountEvt, TypedCommitEvt, TypedHandleEvt, TypedIdentityEvt, TypedTombstoneEvt,
};
use anyhow::Result;
use diesel::*;

pub struct RequestSeqRangeOpts {
    pub earliest_seq: Option<i64>,
    pub latest_seq: Option<i64>,
    pub earliest_time: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Sequencer {
    pub destroyed: bool,
    pub tries_with_no_results: u64,
    pub crawlers: Crawlers,
    pub last_seen: Option<i64>,
}

impl Sequencer {
    pub fn new(crawlers: Crawlers, last_seen: Option<i64>) -> Self {
        Sequencer {
            destroyed: false,
            tries_with_no_results: 0,
            last_seen: Some(last_seen.unwrap_or(0)),
            crawlers,
        }
    }

    pub async fn curr(&self) -> Result<Option<i64>> {
        use crate::schema::pds::repo_seq::dsl as RepoSeqSchema;
        let conn = &mut establish_connection()?;

        let got = RepoSeqSchema::repo_seq
            .select(models::RepoSeq::as_select())
            .order_by(RepoSeqSchema::seq.desc())
            .first(conn)
            .optional()?;
        match got {
            None => Ok(None),
            Some(got) => Ok(got.seq),
        }
    }

    pub async fn next(&self, cursor: i64) -> Result<Option<models::RepoSeq>> {
        use crate::schema::pds::repo_seq::dsl as RepoSeqSchema;
        let conn = &mut establish_connection()?;

        let got = RepoSeqSchema::repo_seq
            .filter(RepoSeqSchema::seq.gt(cursor))
            .select(models::RepoSeq::as_select())
            .order_by(RepoSeqSchema::seq.asc())
            .first(conn)
            .optional()?;
        Ok(got)
    }

    pub async fn earliest_after_time(&self, time: String) -> Result<Option<models::RepoSeq>> {
        use crate::schema::pds::repo_seq::dsl as RepoSeqSchema;
        let conn = &mut establish_connection()?;

        let got = RepoSeqSchema::repo_seq
            .filter(RepoSeqSchema::sequencedAt.ge(time))
            .select(models::RepoSeq::as_select())
            .order_by(RepoSeqSchema::sequencedAt.asc())
            .first(conn)
            .optional()?;
        Ok(got)
    }

    pub async fn request_seq_range(&self, opts: RequestSeqRangeOpts) -> Result<Vec<SeqEvt>> {
        use crate::schema::pds::repo_seq::dsl as RepoSeqSchema;
        let conn = &mut establish_connection()?;

        let RequestSeqRangeOpts {
            earliest_seq,
            latest_seq,
            earliest_time,
            limit,
        } = opts;

        let mut seq_qb = RepoSeqSchema::repo_seq
            .select(models::RepoSeq::as_select())
            .order_by(RepoSeqSchema::seq.asc())
            .filter(RepoSeqSchema::invalidated.eq(0))
            .into_boxed();
        if let Some(earliest_seq) = earliest_seq {
            seq_qb = seq_qb.filter(RepoSeqSchema::seq.gt(earliest_seq));
        }
        if let Some(latest_seq) = latest_seq {
            seq_qb = seq_qb.filter(RepoSeqSchema::seq.le(latest_seq));
        }
        if let Some(earliest_time) = earliest_time {
            seq_qb = seq_qb.filter(RepoSeqSchema::sequencedAt.ge(earliest_time));
        }
        if let Some(limit) = limit {
            seq_qb = seq_qb.limit(limit);
        }

        let rows = seq_qb.get_results(conn)?;
        if rows.len() < 1 {
            return Ok(vec![]);
        }

        let mut seq_evts: Vec<SeqEvt> = Vec::new();
        for row in rows {
            match row.seq {
                None => continue, // should never hit this because of WHERE clause
                Some(seq) => {
                    if row.event_type == "append" || row.event_type == "rebase" {
                        seq_evts.push(SeqEvt::TypedCommitEvt(TypedCommitEvt {
                            r#type: "commit".to_string(),
                            seq,
                            time: row.sequenced_at,
                            evt: serde_ipld_dagcbor::from_slice::<CommitEvt>(row.event.as_slice())?,
                        }));
                    } else if row.event_type == "handle" {
                        seq_evts.push(SeqEvt::TypedHandleEvt(TypedHandleEvt {
                            r#type: "handle".to_string(),
                            seq,
                            time: row.sequenced_at,
                            evt: serde_ipld_dagcbor::from_slice::<HandleEvt>(row.event.as_slice())?,
                        }));
                    } else if row.event_type == "identity" {
                        seq_evts.push(SeqEvt::TypedIdentityEvt(TypedIdentityEvt {
                            r#type: "identity".to_string(),
                            seq,
                            time: row.sequenced_at,
                            evt: serde_ipld_dagcbor::from_slice::<IdentityEvt>(
                                row.event.as_slice(),
                            )?,
                        }));
                    } else if row.event_type == "account" {
                        seq_evts.push(SeqEvt::TypedAccountEvt(TypedAccountEvt {
                            r#type: "account".to_string(),
                            seq,
                            time: row.sequenced_at,
                            evt: serde_ipld_dagcbor::from_slice::<AccountEvt>(
                                row.event.as_slice(),
                            )?,
                        }));
                    } else if row.event_type == "tombstone" {
                        seq_evts.push(SeqEvt::TypedTombstoneEvt(TypedTombstoneEvt {
                            r#type: "tombstone".to_string(),
                            seq,
                            time: row.sequenced_at,
                            evt: serde_ipld_dagcbor::from_slice::<TombstoneEvt>(
                                row.event.as_slice(),
                            )?,
                        }));
                    }
                }
            }
        }

        Ok(seq_evts)
    }

    pub async fn sequence_evt(&mut self, evt: models::RepoSeq) -> Result<i64> {
        use crate::schema::pds::repo_seq::dsl as RepoSeqSchema;
        let conn = &mut establish_connection()?;

        let res = insert_into(RepoSeqSchema::repo_seq)
            .values((
                RepoSeqSchema::did.eq(evt.did),
                RepoSeqSchema::event.eq(evt.event),
                RepoSeqSchema::eventType.eq(evt.event_type),
                RepoSeqSchema::sequencedAt.eq(evt.sequenced_at),
            ))
            .get_result::<models::RepoSeq>(conn)?;
        self.crawlers.notify_of_update().await?;
        Ok(res.seq.expect("Sequence number wasn't updated on insert."))
    }

    pub async fn sequence_commit(
        &mut self,
        did: String,
        commit_data: CommitData,
        writes: Vec<PreparedWrite>,
    ) -> Result<i64> {
        let evt = format_seq_commit(did, commit_data, writes).await?;
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

    pub async fn sequence_tombstone(&mut self, did: String) -> Result<i64> {
        let evt = format_seq_tombstone(did).await?;
        self.sequence_evt(evt).await
    }
}

pub async fn delete_all_for_user(did: &String, excluding_seqs: Option<Vec<i64>>) -> Result<()> {
    use crate::schema::pds::repo_seq::dsl as RepoSeqSchema;
    let conn = &mut establish_connection()?;
    let excluding_seqs = excluding_seqs.unwrap_or_else(|| vec![]);

    let mut builder = delete(RepoSeqSchema::repo_seq)
        .filter(RepoSeqSchema::did.eq(did))
        .into_boxed();
    if excluding_seqs.len() > 0 {
        builder = builder.filter(RepoSeqSchema::seq.ne_all(excluding_seqs));
    }
    builder.execute(conn)?;
    Ok(())
}

pub mod events;
pub mod outbox;
