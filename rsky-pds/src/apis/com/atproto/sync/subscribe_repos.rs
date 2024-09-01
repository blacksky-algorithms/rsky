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
                true => yield Message::Text(json!({
                    "$type": "#error",
                    "name": "FutureCursor",
                    "message": "Cursor in the future."
                }).to_string()),
                false => match next {
                    Some(next) if next.sequenced_at < backfill_time => {
                        yield Message::Text(json!({
                            "$type": "#info",
                            "name": "OutdatedCursor",
                            "message": "Requested cursor exceeded limit. Possibly missing events"
                        }).to_string());
                        match sequencer_lock.earliest_after_time(backfill_time).await {
                            Ok(Some(start_evt)) if start_evt.seq.is_some() => outbox_cursor = Some(start_evt.seq.unwrap() - 1),
                            Ok(None) => outbox_cursor = None,
                            _ => {
                                yield Message::Text(json!({
                                    "$type": "#error",
                                    "name": "EarliestAfterTimeError",
                                    "message": "Failed to fetch earliest event after backfill time."
                                }).to_string());
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
        loop {
            select! {
                evt = event_stream.next() => {
                    let evt = match evt {
                        Some(Ok(evt)) => evt,
                        Some(Err(err)) => {
                            yield Message::Text(json!({
                                "$type": "#error",
                                "name": "EventStreamError",
                                "message": err.to_string()
                            }).to_string());
                            return;
                        },
                        None => {
                            yield Message::Text(json!({
                                "$type": "#error",
                                "name": "EventStreamError",
                                "message": "Failed to fetch event from stream."
                            }).to_string());
                            return;
                        }
                    };

                    match evt {
                        SeqEvt::TypedCommitEvt(commit) => {
                            let TypedCommitEvt { r#type, seq, time, evt } = commit;
                            let CommitEvt { rebase, too_big, repo, commit, prev, rev, since, blocks, ops, blobs } = evt;
                            let subscribe_commit_evt = SubscribeReposCommit {
                                r#type,
                                seq,
                                time: from_str_to_utc(&time),
                                rebase,
                                too_big,
                                repo,
                                commit: commit.to_string(),
                                prev: match prev {
                                    None => None,
                                    Some(prev) => Some(prev.to_string())
                                },
                                rev,
                                since,
                                blocks,
                                ops: ops.into_iter().map(|op| SubscribeReposCommitOperation {
                                    path: op.path,
                                    cid: match op.cid {
                                        None => None,
                                        Some(cid) => Some(cid.to_string())
                                    },
                                    action: op.action.to_string()
                                }).collect::<Vec<SubscribeReposCommitOperation>>(),
                                blobs: blobs.into_iter().map(|blob| blob.to_string()).collect::<Vec<String>>(),
                            };
                            let json_string = match serde_json::to_string(&subscribe_commit_evt) {
                                Ok(json_string) => json_string,
                                Err(_) => {
                                    yield Message::Text(json!({
                                        "$type": "#error",
                                        "name": "SerializationError",
                                        "message": "Failed to serialize event to JSON."
                                    }).to_string());
                                    return;
                                }
                            };
                            yield Message::Text(json_string);
                        },
                        SeqEvt::TypedHandleEvt(handle) => {
                            let TypedHandleEvt { r#type, seq, time, evt } = handle;
                            let HandleEvt { did, handle } = evt;
                            let subscribe_handle_evt = SubscribeReposHandle {
                                r#type,
                                did,
                                handle,
                                seq,
                                time: from_str_to_utc(&time),
                            };
                            let json_string = match serde_json::to_string(&subscribe_handle_evt) {
                                Ok(json_string) => json_string,
                                Err(_) => {
                                    yield Message::Text(json!({
                                        "$type": "#error",
                                        "name": "SerializationError",
                                        "message": "Failed to serialize event to JSON."
                                    }).to_string());
                                    return;
                                }
                            };
                            yield Message::Text(json_string);
                        },
                        SeqEvt::TypedIdentityEvt(identity) => {
                            let TypedIdentityEvt { r#type, seq, time, evt } = identity;
                            let IdentityEvt { did, handle } = evt;
                            let subscribe_identity_evt = SubscribeReposIdentity {
                                r#type,
                                did,
                                seq,
                                handle,
                                time: from_str_to_utc(&time),
                            };
                            let json_string = match serde_json::to_string(&subscribe_identity_evt) {
                                Ok(json_string) => json_string,
                                Err(_) => {
                                    yield Message::Text(json!({
                                        "$type": "#error",
                                        "name": "SerializationError",
                                        "message": "Failed to serialize event to JSON."
                                    }).to_string());
                                    return;
                                }
                            };
                            yield Message::Text(json_string);
                        },
                        SeqEvt::TypedAccountEvt(account) => {
                            let TypedAccountEvt { r#type, seq, time, evt } = account;
                            let AccountEvt { did, active, status } = evt;
                            let subscribe_account_evt = SubscribeReposAccount {
                                r#type,
                                did,
                                seq,
                                status,
                                active,
                                time: from_str_to_utc(&time),
                            };
                            let json_string = match serde_json::to_string(&subscribe_account_evt) {
                                Ok(json_string) => json_string,
                                Err(_) => {
                                    yield Message::Text(json!({
                                        "$type": "#error",
                                        "name": "SerializationError",
                                        "message": "Failed to serialize event to JSON."
                                    }).to_string());
                                    return;
                                }
                            };
                            yield Message::Text(json_string);
                        },
                        SeqEvt::TypedTombstoneEvt(tombstone) => {
                            let TypedTombstoneEvt { r#type, seq, time, evt } = tombstone;
                            let TombstoneEvt { did } = evt;
                            let subscribe_tombstone_evt = SubscribeReposTombstone {
                                r#type,
                                did,
                                seq,
                                time: from_str_to_utc(&time),
                            };
                            let json_string = match serde_json::to_string(&subscribe_tombstone_evt) {
                                Ok(json_string) => json_string,
                                Err(_) => {
                                    yield Message::Text(json!({
                                        "$type": "#error",
                                        "name": "SerializationError",
                                        "message": "Failed to serialize event to JSON."
                                    }).to_string());
                                    return;
                                }
                            };
                            yield Message::Text(json_string);
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
                _ = &mut shutdown => break
            }
        }
    }
}
