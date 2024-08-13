use crate::common::r#async::{AsyncBuffer, AsyncBufferFullError};
use crate::sequencer::events::SeqEvt;
use crate::sequencer::{RequestSeqRangeOpts, Sequencer};
use crate::EVENT_EMITTER;
use anyhow::{anyhow, Result};
use futures::stream::Stream;
use futures::{pin_mut, StreamExt};
use rocket::async_stream::try_stream;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{timeout, Duration};

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
    caught_up: Arc<RwLock<bool>>,
    pub last_seen: i64,
    pub cutover_buffer: Arc<RwLock<Vec<SeqEvt>>>,
    pub out_buffer: Arc<RwLock<AsyncBuffer<SeqEvt>>>,
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
            caught_up: Arc::new(RwLock::new(false)),
            last_seen: -1,
            cutover_buffer: Arc::new(RwLock::new(vec![])),
            out_buffer: Arc::new(RwLock::new(AsyncBuffer::new(Some(max_buffer_size)))),
            state: BackfillState::Requesting,
            backfill_cursor: None,
        }
    }

    pub async fn events<'a>(
        &'a mut self,
        backfill_cursor: Option<i64>,
    ) -> impl Stream<Item = Result<SeqEvt>> + 'a {
        println!("@LOG: Trying stream");
        try_stream! {
            if let Some(cursor) = backfill_cursor {
                let backfill_stream = self.get_backfill(cursor).await;
                pin_mut!(backfill_stream);
                while let Some(Ok(evt)) = backfill_stream.next().await {
                    yield evt;
                }
            } else {
                let mut caught_up_lock = self.caught_up.write().await;
                *caught_up_lock = true;
            }

            let caught_up = Arc::clone(&self.caught_up);
            let out_buffer = Arc::clone(&self.out_buffer);
            let cutover_buffer = Arc::clone(&self.cutover_buffer);

            let add_to_buffer = move |evts: Vec<String>| {
                let evts = evts
                    .into_iter()
                    .map(|evt| serde_json::from_str(evt.as_str()).unwrap())
                    .collect::<Vec<SeqEvt>>();
                let caught_up = Arc::clone(&caught_up);
                let out_buffer = Arc::clone(&out_buffer);
                let cutover_buffer = Arc::clone(&cutover_buffer);

                async move {
                    if *caught_up.read().await {
                        let out_lock = out_buffer.write().await;
                        out_lock.push_many(evts);
                    } else {
                        let mut cut_lock = cutover_buffer.write().await;
                        cut_lock.extend(evts);
                    }
                }
            };

            EVENT_EMITTER.write().await.on("events", move |evts| {
                let value = add_to_buffer.clone();
                async move {
                    tokio::spawn(value(evts));
                    ()
                }
            });

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
                {
                    let out_lock = self.out_buffer.write().await;
                    let mut cutover_lock = self.cutover_buffer.write().await;
                    out_lock.push_many(cutover_evts);
                    out_lock.push_many(cutover_lock.drain(..).collect());
                }
                let mut caught_up_lock = self.caught_up.write().await;
                *caught_up_lock = true;
            } else {
                let mut caught_up_lock = self.caught_up.write().await;
                *caught_up_lock = true;
            }

            while let Ok(Some(res)) = timeout(Duration::from_secs(15),self.out_buffer.write().await.next()).await {
                println!("@LOG: Got event from buffer.");
                let evt = res.map_err(|error| {
                    match error.downcast_ref() {
                        Some(AsyncBufferFullError(_)) => anyhow!("Stream consumer too slow.".to_string()),
                        _ => anyhow!(error.to_string())
                    }
                })?;
                println!("@LOG: Got seqevent {evt:?}.");
                if evt.seq() > self.last_seen {
                    self.last_seen = evt.seq();
                    println!("@LOG: yielding {evt:?}.");
                    yield evt;
                }
            }
        }
    }

    pub async fn get_backfill<'a>(
        &'a mut self,
        backfill_cursor: i64,
    ) -> impl Stream<Item = Result<SeqEvt>> + 'a {
        println!("@LOG: Trying backfill");
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
                    self.last_seen = evt.seq();
                    yield evt.clone();
                }
                let seq_cursor = self.sequencer.last_seen.unwrap_or(-1);
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
