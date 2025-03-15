use crate::db::save_post;
use crate::models::Post;
use chrono::Utc;
use futures::StreamExt as _;
use reqwest::Url;
use rsky_lexicon::app::bsky::embed::{Embeds, MediaUnion};
use rsky_lexicon::app::bsky::feed::{Post as AppBskyFeedPost, PostLabels};
use rsky_lexicon::com::atproto::sync::SubscribeRepos;
use std::env;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use tokio::sync::{Semaphore, broadcast};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::protocol::Message;

async fn process(message: Vec<u8>, surreal: &Surreal<Db>, tx: &broadcast::Sender<Post>) {
    let mut posts_to_create = Vec::new();

    match rsky_firehose::firehose::read(&message) {
        Ok((_header, body)) => {
            match body {
                SubscribeRepos::Commit(commit) => {
                    if commit.ops.is_empty() {
                        tracing::debug!("Operations empty.");
                    }
                    if commit.too_big {
                        tracing::debug!("Too big.");
                    }
                    commit
                        .ops
                        .into_iter()
                        .filter(|operation| operation.path.starts_with("app.bsky.feed.post/"))
                        .map(|operation| {
                            let record_uri = format!("at://{}/{}", commit.repo, operation.path);
                            match operation.action.as_str() {
                                "update" => {}
                                "create" => {
                                    if let Some(cid) = operation.cid {
                                        let mut car_reader = Cursor::new(&commit.blocks);
                                        let _car_header =
                                            rsky_firehose::car::read_header(&mut car_reader)
                                                .unwrap();
                                        let car_blocks =
                                            rsky_firehose::car::read_blocks(&mut car_reader)
                                                .unwrap();

                                        let record_reader =
                                            Cursor::new(car_blocks.get(&cid).unwrap());
                                        if let Ok(post_record) = serde_cbor::from_reader::<
                                            AppBskyFeedPost,
                                            Cursor<&Vec<u8>>,
                                        >(
                                            record_reader
                                        ) {
                                            let mut post = Post {
                                                uri: record_uri.clone(),
                                                cid: cid.to_string(),
                                                reply_parent: match post_record.reply {
                                                    None => None,
                                                    Some(ref reply) => {
                                                        Some(reply.parent.uri.clone())
                                                    }
                                                },
                                                reply_root: match post_record.reply {
                                                    None => None,
                                                    Some(ref reply) => Some(reply.root.uri.clone()),
                                                },
                                                indexed_at: Utc::now(),
                                                prev: match commit.prev {
                                                    None => None,
                                                    Some(ref prev) => Some(prev.to_string()),
                                                },
                                                sequence: commit.seq,
                                                text: post_record.text,
                                                langs: post_record.langs,
                                                author: commit.repo.clone(), // the DID of the author
                                                external_uri: None,
                                                external_title: None,
                                                external_description: None,
                                                external_thumb: None,
                                                quote_uri: None,
                                                quote_cid: None,
                                                created_at: post_record.created_at,
                                                labels: None,
                                                local_only: false,
                                            };
                                            if let Some(PostLabels::SelfLabels(self_labels)) =
                                                post_record.labels
                                            {
                                                post.labels = Some(
                                                    self_labels
                                                        .values
                                                        .into_iter()
                                                        .map(|self_label| self_label.val)
                                                        .collect::<Vec<String>>(),
                                                );
                                            }
                                            if let Some(embed) = post_record.embed {
                                                match embed {
                                                    Embeds::RecordWithMedia(e) => {
                                                        post.quote_cid = Some(e.record.record.cid);
                                                        post.quote_uri = Some(e.record.record.uri);
                                                        match e.media {
                                                            MediaUnion::External(e) => {
                                                                post.external_uri =
                                                                    Some(e.external.uri);
                                                                post.external_title =
                                                                    Some(e.external.title);
                                                                post.external_description =
                                                                    Some(e.external.description);
                                                                if let Some(thumb_blob) =
                                                                    e.external.thumb
                                                                {
                                                                    if let Some(thumb_cid) =
                                                                        thumb_blob.cid
                                                                    {
                                                                        post.external_thumb =
                                                                            Some(thumb_cid);
                                                                    };
                                                                };
                                                            }
                                                            _ => (),
                                                        }
                                                    }
                                                    Embeds::External(e) => {
                                                        post.external_uri = Some(e.external.uri);
                                                        post.external_title =
                                                            Some(e.external.title);
                                                        post.external_description =
                                                            Some(e.external.description);
                                                        if let Some(thumb_blob) = e.external.thumb {
                                                            if let Some(thumb_cid) = thumb_blob.cid
                                                            {
                                                                post.external_thumb =
                                                                    Some(thumb_cid);
                                                            };
                                                        };
                                                    }
                                                    Embeds::Record(e) => {
                                                        post.quote_cid = Some(e.record.cid);
                                                        post.quote_uri = Some(e.record.uri);
                                                    }
                                                    _ => (),
                                                }
                                            }
                                            posts_to_create.push(post);
                                        }
                                    }
                                }
                                "delete" => {}
                                _ => {}
                            }
                        })
                        .for_each(drop);
                }
                _ => tracing::debug!("@LOG: Saw non-commit event: {body:?}"),
            }
        }
        Err(error) => tracing::error!(
            "@LOG: Error unwrapping message and header: {}",
            error.to_string()
        ),
    }
    for post in posts_to_create {
        save_post(surreal, post.clone()).await.ok();
        let _ = tx.send(post).ok();
    }
}

pub async fn run_firehose(surreal: Surreal<Db>, tx: broadcast::Sender<Post>) -> anyhow::Result<()> {
    let subscriber_base_path =
        env::var("FIREHOSE_SUBSCRIPTION_PATH").unwrap_or_else(|_| "wss://bsky.network".to_string());

    // Create a semaphore to limit the number of concurrent processing tasks
    let semaphore = Arc::new(Semaphore::new(100)); // Adjust the limit as needed
    let surreal = Arc::new(surreal);
    let tx = Arc::new(tx);

    loop {
        // Construct the WebSocket URL
        let url = format!(
            "{}/xrpc/com.atproto.sync.subscribeRepos",
            subscriber_base_path
        );
        let ws_url = Url::parse(&url).expect("Invalid WebSocket URL");

        // Attempt to establish a WebSocket connection
        match connect_async(ws_url).await {
            Ok((mut socket, _response)) => {
                println!("Connected to {}", subscriber_base_path);

                // Listen for incoming messages
                while let Some(msg_result) = socket.next().await {
                    match msg_result {
                        Ok(Message::Binary(message)) => {
                            let semaphore = Arc::clone(&semaphore);
                            let surreal = Arc::clone(&surreal);
                            let tx = Arc::clone(&tx);

                            // Acquire a permit before spawning a new task
                            let permit = match semaphore.clone().acquire_owned().await {
                                Ok(permit) => permit,
                                Err(_) => {
                                    tracing::error!("Semaphore closed");
                                    break;
                                }
                            };

                            // Spawn a new asynchronous task to process the message
                            tokio::spawn(async move {
                                process(message, &surreal, &tx).await;
                                // Permit is automatically released when it goes out of scope
                                drop(permit);
                            });
                        }
                        Ok(Message::Close(_)) => {
                            tracing::debug!("WebSocket connection closed by server.");
                            break;
                        }
                        Ok(_) => {
                            // Handle other message types if necessary
                        }
                        Err(e) => {
                            tracing::error!("WebSocket error: {}", e);
                            break;
                        }
                    }
                }
            }
            Err(error) => {
                tracing::error!(
                    "Error connecting to {}. Waiting to reconnect: {:?}",
                    subscriber_base_path,
                    error
                );
                // Use asynchronous sleep to avoid blocking the thread
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        }

        // Optionally, wait before attempting to reconnect
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
