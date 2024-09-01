use crate::account_manager::helpers::account::AccountStatus;
use crate::common::time::SECOND;
use crate::common::{cbor_to_struct, wait};
use crate::crawlers::Crawlers;
use crate::db::establish_connection;
use crate::models;
use crate::repo::types::{CommitData, PreparedWrite};
use crate::sequencer::events::{
    format_seq_account_evt, format_seq_commit, format_seq_handle_update, format_seq_identity_evt,
    format_seq_tombstone, SeqEvt, TypedAccountEvt, TypedCommitEvt, TypedHandleEvt,
    TypedIdentityEvt, TypedTombstoneEvt,
};
use crate::EVENT_EMITTER;
use anyhow::Result;
use diesel::*;
use futures::{Stream, StreamExt};
use std::cmp;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

pub struct RequestSeqRangeOpts {
    pub earliest_seq: Option<i64>,
    pub latest_seq: Option<i64>,
    pub earliest_time: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Sequencer {
    pub destroyed: bool,
    pub tries_with_no_results: u32,
    pub waker: Option<Waker>,
    pub crawlers: Crawlers,
    pub last_seen: Option<i64>,
}

impl Sequencer {
    pub fn new(crawlers: Crawlers, last_seen: Option<i64>) -> Self {
        Sequencer {
            destroyed: false,
            tries_with_no_results: 0,
            last_seen: Some(last_seen.unwrap_or(0)),
            waker: None,
            crawlers,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        let curr = self.curr().await?;
        self.last_seen = Some(curr.unwrap_or(0));
        if self.waker.is_none() {
            loop {
                while let Some(_) = self.next().await {
                    ()
                }
            }
        }
        Ok(())
    }

    pub async fn destroy(&mut self) {
        self.destroyed = true;
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
        EVENT_EMITTER.write().await.emit("close", ());
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

    pub async fn next_seq(&self, cursor: i64) -> Result<Option<models::RepoSeq>> {
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
                            evt: cbor_to_struct(row.event)?,
                        }));
                    } else if row.event_type == "handle" {
                        seq_evts.push(SeqEvt::TypedHandleEvt(TypedHandleEvt {
                            r#type: "handle".to_string(),
                            seq,
                            time: row.sequenced_at,
                            evt: cbor_to_struct(row.event)?,
                        }));
                    } else if row.event_type == "identity" {
                        seq_evts.push(SeqEvt::TypedIdentityEvt(TypedIdentityEvt {
                            r#type: "identity".to_string(),
                            seq,
                            time: row.sequenced_at,
                            evt: cbor_to_struct(row.event)?,
                        }));
                    } else if row.event_type == "account" {
                        seq_evts.push(SeqEvt::TypedAccountEvt(TypedAccountEvt {
                            r#type: "account".to_string(),
                            seq,
                            time: row.sequenced_at,
                            evt: cbor_to_struct(row.event)?,
                        }));
                    } else if row.event_type == "tombstone" {
                        seq_evts.push(SeqEvt::TypedTombstoneEvt(TypedTombstoneEvt {
                            r#type: "tombstone".to_string(),
                            seq,
                            time: row.sequenced_at,
                            evt: cbor_to_struct(row.event)?,
                        }));
                    }
                }
            }
        }

        Ok(seq_evts)
    }

    async fn exponential_backoff(&mut self) -> () {
        self.tries_with_no_results += 1;
        let wait_time = cmp::min(
            2u64.checked_pow(self.tries_with_no_results).unwrap_or(2),
            SECOND as u64,
        );
        wait(wait_time);
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
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

impl Stream for Sequencer {
    type Item = Result<(), anyhow::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.destroyed {
            return Poll::Ready(None);
        }
        // if already polling, do not start another poll
        return match futures::executor::block_on(self.request_seq_range(RequestSeqRangeOpts {
            earliest_seq: self.last_seen,
            latest_seq: None,
            earliest_time: None,
            limit: Some(1000),
        })) {
            Err(err) => {
                eprintln!(
                    "@LOG: sequencer failed to poll db, err: {}, last_seen: {:?}",
                    err.to_string(),
                    self.last_seen
                );
                self.waker = Some(cx.waker().clone());
                futures::executor::block_on(self.exponential_backoff());
                Poll::Ready(Some(Err(err)))
            }
            Ok(evts) => {
                if evts.len() > 0 {
                    self.tries_with_no_results = 0;
                    futures::executor::block_on(EVENT_EMITTER.write()).emit(
                        "events",
                        evts.iter()
                            .map(|evt| serde_json::to_string(evt).unwrap())
                            .collect::<Vec<String>>(),
                    );
                    self.last_seen = match evts.last() {
                        None => self.last_seen,
                        Some(last_evt) => Some(last_evt.seq()),
                    };
                    self.waker = Some(cx.waker().clone());
                    Poll::Ready(Some(Ok(())))
                } else {
                    self.waker = Some(cx.waker().clone());
                    futures::executor::block_on(self.exponential_backoff());
                    Poll::Pending
                }
            }
        };
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
