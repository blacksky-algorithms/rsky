use crate::common::time::from_str_to_utc;
use crate::common::RFC3339_VARIANT;
use crate::config::ServerConfig;
use crate::crawlers::Crawlers;
use crate::sequencer::events::{
    AccountEvt, CommitEvt, HandleEvt, IdentityEvt, SeqEvt, TombstoneEvt, TypedAccountEvt,
    TypedCommitEvt, TypedHandleEvt, TypedIdentityEvt, TypedTombstoneEvt,
};
use crate::sequencer::outbox::{Outbox, OutboxOpts};
use crate::sequencer::Sequencer;
use crate::xrpc_server::stream::frames::{ErrorFrame, Frame, MessageFrame, MessageFrameOpts};
use crate::xrpc_server::stream::types::ErrorFrameBody;
use chrono::offset::Utc as UtcOffset;
use chrono::{DateTime, Duration};
use futures::{pin_mut, StreamExt};
use rocket::tokio::select;
use rocket::{Shutdown, State};
use rsky_lexicon::com::atproto::sync::{
    SubscribeReposAccount, SubscribeReposCommit, SubscribeReposCommitOperation,
    SubscribeReposHandle, SubscribeReposIdentity, SubscribeReposTombstone,
};
use serde_json::json;
use std::time::SystemTime;
use tokio::time::{interval, Duration as TokioDuration};
use ws::Message;

fn get_backfill_limit(ms: u64) -> String {
    let system_time = SystemTime::now();
    let mut dt: DateTime<UtcOffset> = system_time.into();
    dt = dt - Duration::milliseconds(ms as i64);
    format!("{}", dt.format(RFC3339_VARIANT))
}

/// Repository event stream, aka Firehose endpoint. Outputs repo commits with diff data,
/// and identity update events, for all repositories on the current server. See the atproto
/// specifications for details around stream sequencing, repo versioning, CAR diff format, and more.
/// Public and does not require auth; implemented by PDS and Relay.
#[rocket::get("/xrpc/com.atproto.sync.subscribeRepos?<cursor>")]
#[allow(unused_variables)]
pub async fn subscribe_repos<'a>(
    cursor: Option<i64>,
    cfg: &'a State<ServerConfig>,
    mut shutdown: Shutdown,
    ws: ws::WebSocket,
) -> ws::Stream!['a] {
    ws::Stream! { ws =>
        let sequencer_lock = Sequencer::new(
            Crawlers::new(cfg.service.hostname.clone(), cfg.crawlers.clone()),
            None,
        );
        let mut outbox = Outbox::new(
            sequencer_lock.clone(),
            Some(OutboxOpts {
                max_buffer_size: cfg.subscription.repo_backfill_limit_ms as usize,
            })
        );

        println!("@LOG DEBUG: request to com.atproto.sync.subscribeRepos; Cursor={cursor:?}");
        let backfill_time = get_backfill_limit(cfg.subscription.repo_backfill_limit_ms);

        let mut outbox_cursor: Option<i64> = None;
        if let Some(cursor) = cursor {
            let next = match sequencer_lock.next_seq(cursor).await {
                Ok(next) => next,
                Err(_) => {
                    yield Message::Text(json!({
                        "$type": "#error",
                        "name": "NextError",
                        "message": "Failed to fetch next event."
                    }).to_string());
                    return;
                }
            };
            let curr = match sequencer_lock.curr().await {
                Ok(curr) => curr,
                Err(_) => {
                    yield Message::Text(json!({
                        "$type": "#error",
                        "name": "CurrError",
                        "message": "Failed to fetch current event."
                    }).to_string());
                    return;
                }
            };
            match cursor > curr.unwrap_or(0) {
                true => {
                    let error_frame = ErrorFrame::new(ErrorFrameBody {
                        error: "FutureCursor".to_string(),
                        message: Some("Cursor in the future.".to_string()),
                    });
                    yield Message::Binary(error_frame.to_bytes().expect("couldn't translate error to binary."));
                },
                false => match next {
                    Some(next) if next.sequenced_at < backfill_time => {
                        let error_frame = ErrorFrame::new(ErrorFrameBody {
                            error: "OutdatedCursor".to_string(),
                            message: Some("Requested cursor exceeded limit. Possibly missing events.".to_string()),
                        });
                        yield Message::Binary(error_frame.to_bytes().expect("couldn't translate error to binary."));
                        match sequencer_lock.earliest_after_time(backfill_time).await {
                            Ok(Some(start_evt)) if start_evt.seq.is_some() => outbox_cursor = Some(start_evt.seq.unwrap() - 1),
                            Ok(None) => outbox_cursor = None,
                            _ => {
                                let error_frame = ErrorFrame::new(ErrorFrameBody {
                                    error: "EarliestAfterTimeError".to_string(),
                                    message: Some("Failed to fetch earliest event after backfill time.".to_string()),
                                });
                                yield Message::Binary(error_frame.to_bytes().expect("couldn't translate error to binary."));
                                return;
                            }
                        }
                    },
                    _ => outbox_cursor = Some(cursor)
                }
            }
        }

        let event_stream = outbox.events(outbox_cursor).await;
        pin_mut!(ws);
        pin_mut!(event_stream);

        // Initialize the ping interval
        let mut ping_interval = interval(TokioDuration::from_secs(30));

        loop {
            select! {
                evt = event_stream.next() => {
                    let evt = match evt {
                        Some(Ok(evt)) => evt,
                        Some(Err(err)) => {
                            let error_frame = ErrorFrame::new(ErrorFrameBody {
                                error: "EventStreamError".to_string(),
                                message: Some(err.to_string()),
                            });
                            yield Message::Binary(error_frame.to_bytes().expect("couldn't translate error to binary."));
                            return;
                        },
                        None => {
                            let error_frame = ErrorFrame::new(ErrorFrameBody {
                                error: "EventStreamError".to_string(),
                                message: Some("Failed to fetch event from stream.".to_string()),
                            });
                            yield Message::Binary(error_frame.to_bytes().expect("couldn't translate error to binary."));
                            return;
                        }
                    };

                    match evt {
                        SeqEvt::TypedCommitEvt(commit) => {
                            let TypedCommitEvt { r#type, seq, time, evt } = commit;
                            let CommitEvt { rebase, too_big, repo, commit, prev, rev, since, blocks, ops, blobs } = evt;
                            let subscribe_commit_evt = SubscribeReposCommit {
                                seq,
                                time: from_str_to_utc(&time),
                                rebase,
                                too_big,
                                repo,
                                commit,
                                prev,
                                rev,
                                since,
                                blocks,
                                ops: ops.into_iter().map(|op| SubscribeReposCommitOperation {
                                    path: op.path,
                                    cid: match op.cid {
                                        None => None,
                                        Some(cid) => Some(cid)
                                    },
                                    action: op.action.to_string()
                                }).collect::<Vec<SubscribeReposCommitOperation>>(),
                                blobs: blobs.into_iter().map(|blob| blob.to_string()).collect::<Vec<String>>(),
                            };
                            let message_frame = MessageFrame::new(subscribe_commit_evt, Some(MessageFrameOpts { r#type: Some(format!("#{0}",r#type)) }));
                            let binary = match message_frame.to_bytes() {
                                Ok(binary) => binary,
                                Err(_) => {
                                    let error_frame = ErrorFrame::new(ErrorFrameBody {
                                        error: "SerializationError".to_string(),
                                        message: Some("Failed to serialize event to message frame.".to_string()),
                                    });
                                    yield Message::Binary(error_frame.to_bytes().expect("couldn't translate error to binary."));
                                    return;
                                }
                            };
                            yield Message::Binary(binary);
                        },
                        SeqEvt::TypedHandleEvt(handle) => {
                            let TypedHandleEvt { r#type, seq, time, evt } = handle;
                            let HandleEvt { did, handle } = evt;
                            let subscribe_handle_evt = SubscribeReposHandle {
                                did,
                                handle,
                                seq,
                                time: from_str_to_utc(&time),
                            };
                            let message_frame = MessageFrame::new(subscribe_handle_evt, Some(MessageFrameOpts { r#type: Some(format!("#{0}",r#type)) }));
                            let binary = match message_frame.to_bytes() {
                                Ok(binary) => binary,
                                Err(_) => {
                                    let error_frame = ErrorFrame::new(ErrorFrameBody {
                                        error: "SerializationError".to_string(),
                                        message: Some("Failed to serialize event to message frame.".to_string()),
                                    });
                                    yield Message::Binary(error_frame.to_bytes().expect("couldn't translate error to binary."));
                                    return;
                                }
                            };
                            yield Message::Binary(binary);
                        },
                        SeqEvt::TypedIdentityEvt(identity) => {
                            let TypedIdentityEvt { r#type, seq, time, evt } = identity;
                            let IdentityEvt { did, handle } = evt;
                            let subscribe_identity_evt = SubscribeReposIdentity {
                                did,
                                seq,
                                handle,
                                time: from_str_to_utc(&time),
                            };
                            let message_frame = MessageFrame::new(subscribe_identity_evt, Some(MessageFrameOpts { r#type: Some(format!("#{0}",r#type)) }));
                            let binary = match message_frame.to_bytes() {
                                Ok(binary) => binary,
                                Err(_) => {
                                    let error_frame = ErrorFrame::new(ErrorFrameBody {
                                        error: "SerializationError".to_string(),
                                        message: Some("Failed to serialize event to message frame.".to_string()),
                                    });
                                    yield Message::Binary(error_frame.to_bytes().expect("couldn't translate error to binary."));
                                    return;
                                }
                            };
                            yield Message::Binary(binary);
                        },
                        SeqEvt::TypedAccountEvt(account) => {
                            let TypedAccountEvt { r#type, seq, time, evt } = account;
                            let AccountEvt { did, active, status } = evt;
                            let subscribe_account_evt = SubscribeReposAccount {
                                did,
                                seq,
                                status,
                                active,
                                time: from_str_to_utc(&time),
                            };
                            let message_frame = MessageFrame::new(subscribe_account_evt, Some(MessageFrameOpts { r#type: Some(format!("#{0}",r#type)) }));
                            let binary = match message_frame.to_bytes() {
                                Ok(binary) => binary,
                                Err(_) => {
                                    let error_frame = ErrorFrame::new(ErrorFrameBody {
                                        error: "SerializationError".to_string(),
                                        message: Some("Failed to serialize event to message frame.".to_string()),
                                    });
                                    yield Message::Binary(error_frame.to_bytes().expect("couldn't translate error to binary."));
                                    return;
                                }
                            };
                            yield Message::Binary(binary);
                        },
                        SeqEvt::TypedTombstoneEvt(tombstone) => {
                            let TypedTombstoneEvt { r#type, seq, time, evt } = tombstone;
                            let TombstoneEvt { did } = evt;
                            let subscribe_tombstone_evt = SubscribeReposTombstone {
                                did,
                                seq,
                                time: from_str_to_utc(&time),
                            };
                            let message_frame = MessageFrame::new(subscribe_tombstone_evt, Some(MessageFrameOpts { r#type: Some(format!("#{0}",r#type)) }));
                            let binary = match message_frame.to_bytes() {
                                Ok(binary) => binary,
                                Err(_) => {
                                    let error_frame = ErrorFrame::new(ErrorFrameBody {
                                        error: "SerializationError".to_string(),
                                        message: Some("Failed to serialize event to message frame.".to_string()),
                                    });
                                    yield Message::Binary(error_frame.to_bytes().expect("couldn't translate error to binary."));
                                    return;
                                }
                            };
                            yield Message::Binary(binary);
                        }
                    }
                }
               message = ws.next() => {
                    match message {
                        Some(Ok(message)) => {
                            match message {
                                ws::Message::Close(close_frame) => {
                                    // Handle Close message
                                    println!("Received Close message: {:?}", close_frame);
                                    let close_frame = ws::frame::CloseFrame {
                                        code: ws::frame::CloseCode::Normal,
                                        reason: "Client disconnected".to_string().into(),
                                    };
                                    break;
                                },
                                ws::Message::Ping(payload) => {
                                    // Respond to Ping with Pong
                                    println!("Received Ping message");
                                    let pong_message = ws::Message::Pong(payload);
                                    yield pong_message;
                                },
                                ws::Message::Pong(_) => {
                                    // Received Pong, can log or ignore
                                    println!("Received Pong message");
                                },
                                _ => {
                                    println!("Received other message: {:?}", message);
                                }
                            }
                        },
                        Some(Err(err)) => {
                            println!("WebSocket error: {:?}", err);
                            break;
                        },
                        None => {
                            println!("WebSocket closed.");
                            break;
                        }
                    }
                },
                // Add the ping interval tick arm
                _ = ping_interval.tick() => {
                    // Send a Ping message to the client
                    yield ws::Message::Ping(vec![]);
                },
                _ = &mut shutdown => break
            }
        }
    }
}
