use crate::common::r#async::{AsyncBuffer, AsyncBufferFullError};
use crate::sequencer::events::SeqEvt;
use crate::sequencer::{RequestSeqRangeOpts, Sequencer};
use crate::EVENT_EMITTER;
use anyhow::{anyhow, Result};
use futures::stream::Stream;
use futures::{pin_mut, StreamExt};
use rocket::async_stream::try_stream;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{timeout, Duration};

#[derive(Debug, Clone)]
pub struct OutboxOpts {
    pub max_buffer_size: usize,
}

pub struct Outbox {
    caught_up: Arc<Mutex<bool>>,
    pub last_seen: i64,
    pub cutover_buffer: Arc<Mutex<Vec<SeqEvt>>>,
    pub out_buffer: Arc<RwLock<AsyncBuffer<SeqEvt>>>,
    pub sequencer: Sequencer,
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
            caught_up: Arc::new(Mutex::new(false)),
            last_seen: -1,
            cutover_buffer: Arc::new(Mutex::new(vec![])),
            out_buffer: Arc::new(RwLock::new(AsyncBuffer::new(Some(max_buffer_size)))),
            backfill_cursor: None,
        }
    }

    pub async fn events<'a>(
        &'a mut self,
        backfill_cursor: Option<i64>,
    ) -> impl Stream<Item = Result<SeqEvt>> + 'a {
        try_stream! {
            if let Some(cursor) = backfill_cursor {
                let backfill_stream = self.get_backfill(cursor).await;
                pin_mut!(backfill_stream);
                while let Some(Ok(evt)) = backfill_stream.next().await {
                    yield evt;
                }
            } else {
                let mut bool_lock = self.caught_up.lock().await;
                *bool_lock = true;
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
                    if *caught_up.lock().await {
                        out_buffer.read().await.push_many(evts);
                    } else {
                        cutover_buffer.lock().await.extend(evts);
                    }
                }
            };

            EVENT_EMITTER.write().await.on("events", move |evts| {
                let rt = Runtime::new().unwrap();
                // By entering the context, we tie `tokio::spawn` to this executor.
                let _guard = rt.enter();
                rt.block_on(tokio::spawn(
                    add_to_buffer(evts)
                )).unwrap();
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
                    let out_buffer_lock = self.out_buffer.read().await;
                    let mut cutover_lock = self.cutover_buffer.lock().await;
                    out_buffer_lock.push_many(cutover_evts);
                    out_buffer_lock.push_many(cutover_lock.drain(..).collect());
                }
                let mut bool_lock = self.caught_up.lock().await;
                *bool_lock = true;
            } else {
                let mut bool_lock = self.caught_up.lock().await;
                *bool_lock = true;
            }

            loop {
                while let Ok(Some(res)) = timeout(Duration::from_secs(2),self.out_buffer.write().await.next()).await {
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

    pub async fn get_backfill<'a>(
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
