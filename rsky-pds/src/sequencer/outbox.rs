//! One subscriber's view of the firehose: everything after its cursor,
//! from the database while it catches up and from the poll loop's
//! broadcast once it has.
//!
//! The subscription to the broadcast is taken before the backfill starts,
//! so a batch the poll loop emits during the backfill is buffered rather
//! than missed, and every event is delivered once because the live phase
//! drops anything at or below the last sequence the backfill yielded. A
//! subscriber that falls further behind the broadcast than its capacity
//! is disconnected as too slow rather than served a gap.

use crate::sequencer::events::SeqEvt;
use crate::sequencer::{RequestSeqRangeOpts, Sequencer};
use anyhow::Result;
use futures::stream::Stream;
use rocket::async_stream::try_stream;
use tokio::sync::broadcast::error::RecvError;

/// Why a subscription ended early.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OutboxError {
    /// The subscriber could not keep up with the broadcast and would have
    /// missed events; it must reconnect from its last cursor.
    #[error("ConsumerTooSlow: the subscriber fell {0} batches behind")]
    ConsumerTooSlow(u64),
    /// The poll loop that feeds the broadcast is gone.
    #[error("the sequencer stopped")]
    SequencerStopped,
}

impl From<RecvError> for OutboxError {
    fn from(err: RecvError) -> Self {
        match err {
            RecvError::Lagged(behind) => OutboxError::ConsumerTooSlow(behind),
            RecvError::Closed => OutboxError::SequencerStopped,
        }
    }
}

pub struct Outbox {
    pub sequencer: Sequencer,
}

const PAGE_SIZE: i64 = 500;

impl Outbox {
    pub fn new(sequencer: Sequencer) -> Self {
        Self { sequencer }
    }

    /// Every event with a sequence number above `cursor` (or every event
    /// after this call when `None`), in order, without gaps or repeats.
    pub async fn events(
        &self,
        cursor: Option<i64>,
    ) -> impl Stream<Item = Result<SeqEvt>> + use<'_> {
        // the subscription and the head are taken now, not on first poll,
        // so nothing sequenced between this call and the first poll can
        // slip past either phase
        let mut live = self.sequencer.subscribe();
        let head = self.sequencer.curr().await.map(|head| head.unwrap_or(0));
        try_stream! {
            let head = head?;
            let mut last_seen: i64 = cursor.unwrap_or(head);
            if cursor.is_some() {
                while last_seen < head {
                    let page = self
                        .sequencer
                        .request_seq_range(RequestSeqRangeOpts {
                            earliest_seq: Some(last_seen),
                            latest_seq: Some(head),
                            earliest_time: None,
                            limit: Some(PAGE_SIZE),
                        })
                        .await?;
                    if page.is_empty() {
                        break;
                    }
                    for evt in page {
                        last_seen = evt.seq();
                        yield evt;
                    }
                }
            }
            loop {
                let batch = live.recv().await.map_err(OutboxError::from)?;
                for evt in batch {
                    if evt.seq() > last_seen {
                        last_seen = evt.seq();
                        yield evt;
                    }
                }
            }
        }
    }
}
