use crate::common::r#async::{AsyncBuffer, AsyncBufferFullError};
use crate::sequencer::events::SeqEvt;
use crate::sequencer::{RequestSeqRangeOpts, Sequencer};
use anyhow::{anyhow, Result};
use futures::stream::Stream;
use futures::{pin_mut, StreamExt};
use rocket::async_stream::try_stream;

#[derive(Debug, Clone)]
pub struct OutboxOpts {
    pub max_buffer_size: usize,
}

#[derive(Debug, Clone)]
pub enum BackfillState {
    Requesting,
    Yielding,
    Done,
}

pub struct Outbox {
    caught_up: bool,
    pub last_seen: i64,
    pub cutover_buffer: Vec<SeqEvt>,
    pub out_buffer: AsyncBuffer<SeqEvt>,
    pub sequencer: Sequencer,
    pub state: BackfillState,
    pub backfill_cursor: Option<i64>,
}
const PAGE_SIZE: i64 = 500;

impl Outbox {
    pub fn new(sequencer: Sequencer, opts: Option<OutboxOpts>) -> Self {
        let OutboxOpts { max_buffer_size } = opts.unwrap_or(OutboxOpts {
            max_buffer_size: 500,
        });
        Self {
            sequencer,
            caught_up: false,
            last_seen: -1,
            cutover_buffer: vec![],
            out_buffer: AsyncBuffer::new(Some(max_buffer_size)),
            state: BackfillState::Requesting,
            backfill_cursor: None,
        }
    }

    /// event stream occurs in 3 phases
    /// 1. backfill events: events that have been added to the DB since the last time a connection was open.
    /// The outbox is not yet listening for new events from the sequencer
    /// 2. cutover: the outbox has caught up with where the sequencer purports to be,
    /// but the sequencer might already be halfway through sending out a round of updates.
    /// Therefore, we start accepting the sequencer's events in a buffer, while making our own request to the
    /// database to ensure we're caught up. We then dedupe the query & the buffer & stream the events in order
    /// 3. streaming: we're all caught up on historic state, so the sequencer outputs events and we
    /// immediately yield them
    pub fn events<'a>(
        &'a mut self,
        backfill_cursor: Option<i64>,
    ) -> impl Stream<Item = Result<SeqEvt>> + 'a {
        try_stream! {
            // catch up as much as we can
            if let Some(cursor) = backfill_cursor {
                let backfill_stream = self.get_backfill(cursor);
                pin_mut!(backfill_stream);
                while let Some(Ok(evt)) = backfill_stream.next().await {
                    yield evt;
                }
            } else {
                // if not backfill, we don't need to cutover, just start streaming
                self.caught_up = true;
            }

            /* @TODO: Figure out this whole part
            // streams updates from sequencer, but buffers them for cutover as it makes a last request
            let add_to_buffer = |evts: Vec<SeqEvt>| {
                if self.caught_up {
                    let _ = self.out_buffer.push_many(evts);
                } else {
                    self.cutover_buffer.extend(evts);
                }
            };

            if let Some(signal) = signal {
                if signal.notified().await.is_ok() {
                    self.sequencer.off("events", add_to_buffer);
                    return;
                }
            }

            self.sequencer.on("events", add_to_buffer);*/

            // only need to perform cutover if we've been backfilling
            if let Some(cursor) = backfill_cursor {
                let earliest_seq = if self.last_seen > -1 {
                    Some(self.last_seen)
                } else {
                    Some(cursor)
                };
                let cutover_evts = self.sequencer.request_seq_range(RequestSeqRangeOpts {
                    earliest_seq,
                    latest_seq: None,
                    earliest_time: None,
                    limit: Some(PAGE_SIZE),
                }).await?;
                let _ = self.out_buffer.push_many(cutover_evts);
                // don't worry about dupes, we ensure order on yield
                let _ = self.out_buffer.push_many(self.cutover_buffer.drain(..).collect());
                self.caught_up = true;
            } else {
                self.caught_up = true;
            }

            loop {
                while let Some(res) = self.out_buffer.next().await {
                    //if let Some(signal) = signal {
                    //    if signal.notified().await.is_ok() {
                   //         return;
                   //     }
                   // }
                    let evt = res.map_err(|error| {
                        match error.downcast_ref() {
                            Some(AsyncBufferFullError(_)) => anyhow!("Stream consumer too slow.".to_string()),
                            _ => anyhow!(error.to_string())
                        }
                    })?;
                    if evt.seq() > self.last_seen {
                        self.last_seen = evt.seq();
                        yield evt;
                    }
                }
            }
        }
    }

    /// yields only historical events
    pub fn get_backfill<'a>(
        &'a mut self,
        backfill_cursor: i64,
    ) -> impl Stream<Item = Result<SeqEvt>> + 'a {
        try_stream! {
            loop {
                let earliest_seq = if self.last_seen > -1 {
                    Some(self.last_seen)
                } else {
                    Some(backfill_cursor)
                };
                let evts = match self.sequencer.request_seq_range(RequestSeqRangeOpts {
                    earliest_seq,
                    latest_seq: None,
                    earliest_time: None,
                    limit: Some(PAGE_SIZE),
                }).await {
                    Ok(res) => res,
                    Err(_) => break
                };
                for evt in evts.iter() {
                    self.last_seen = evt.seq(); // Technically updating early due to borrow checker
                    yield evt.clone();
                }
                let seq_cursor = self.sequencer.last_seen.unwrap_or(-1);
                // if we're within half a pagesize of the sequencer, we call it good & switch to cutover
                if seq_cursor - self.last_seen < (PAGE_SIZE / 2)  {
                    break;
                }
                if evts.is_empty() {
                    break;
                }
            }
        }
    }
}
