#![allow(unused_imports)]

use dotenvy::dotenv;
use futures::StreamExt as _;
use lexicon_cid::Cid;
use rsky_lexicon::app::bsky::feed::like::Like;
use rsky_lexicon::app::bsky::feed::Post;
use rsky_lexicon::app::bsky::graph::follow::Follow;
use rsky_lexicon::com::atproto::sync::SubscribeRepos;
use serde::Deserialize;
use std::env;
use std::io::Cursor;
use std::str::FromStr;
use std::sync::Arc;
use std::{thread, time::Duration};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use url::Url;

#[derive(Debug, Deserialize)]
#[serde(tag = "$type")]
enum Lexicon {
    #[serde(rename(deserialize = "app.bsky.feed.post"))]
    AppBskyFeedPost(Post),
    #[serde(rename(deserialize = "app.bsky.feed.like"))]
    AppBskyFeedLike(Like),
    #[serde(rename(deserialize = "app.bsky.graph.follow"))]
    AppBskyFeedFollow(Follow),
}

async fn queue_delete(
    url: String,
    records: Vec<rsky_firehose::models::DeleteOp>,
    client: &reqwest::Client,
) -> Result<(), Box<dyn std::error::Error>> {
    let token = env::var("RSKY_API_KEY").map_err(|_| {
        "Pass a valid preshared token via `RSKY_API_KEY` environment variable.".to_string()
    })?;
    client
        .put(url)
        .json(&records)
        .header("X-RSKY-KEY", token)
        .header("Connection", "Keep-Alive")
        .header("Keep-Alive", "timeout=5, max=1000")
        .send()
        .await?;
    Ok(())
}

async fn queue_create<T: serde::ser::Serialize>(
    url: String,
    records: Vec<rsky_firehose::models::CreateOp<T>>,
    client: &reqwest::Client,
) -> Result<(), Box<dyn std::error::Error>> {
    let token = env::var("RSKY_API_KEY").map_err(|_| {
        "Pass a valid preshared token via `RSKY_API_KEY` environment variable.".to_string()
    })?;
    client
        .put(url)
        .json(&records)
        .header("X-RSKY-KEY", token)
        .header("Connection", "Keep-Alive")
        .header("Keep-Alive", "timeout=5, max=1000")
        .send()
        .await?;
    Ok(())
}

async fn update_cursor(
    url: String,
    service: String,
    sequence: &i64,
    client: &reqwest::Client,
) -> Result<(), Box<dyn std::error::Error>> {
    let token = env::var("RSKY_API_KEY").map_err(|_| {
        "Pass a valid preshared token via `RSKY_API_KEY` environment variable.".to_string()
    })?;
    let query = vec![("service", service), ("sequence", sequence.to_string())];
    client
        .put(url)
        .query(&query)
        .header("X-RSKY-KEY", token)
        .header("Accept", "application/json")
        .send()
        .await?;
    Ok(())
}

async fn process(message: Vec<u8>, client: &reqwest::Client) {
    let default_queue_path =
        env::var("FEEDGEN_QUEUE_ENDPOINT").unwrap_or("https://[::1]:8081".into());
    let subscriber_base_path =
        env::var("FEEDGEN_SUBSCRIPTION_PATH").unwrap_or("wss://bsky.network".into());

    match rsky_firehose::firehose::read(&message) {
        Ok(Some((_header, body))) => {
            let mut posts_to_delete = Vec::new();
            let mut posts_to_create = Vec::new();
            let mut likes_to_delete = Vec::new();
            let mut likes_to_create = Vec::new();
            let mut follows_to_delete = Vec::new();
            let mut follows_to_create = Vec::new();

            match body {
                SubscribeRepos::Commit(commit) => {
                    if commit.ops.is_empty() {
                        println!("Operations empty.");
                    }
                    if commit.too_big {
                        println!("Too big.");
                    }
                    // update stored cursor every 20 events or so
                    if (&commit.seq).rem_euclid(20) == 0 {
                        let cursor_endpoint = format!("{}/cursor", default_queue_path);
                        let resp = update_cursor(
                            cursor_endpoint,
                            subscriber_base_path,
                            &commit.seq,
                            client,
                        )
                        .await;
                        match resp {
                            Ok(()) => (),
                            Err(error) => eprintln!("@LOG: Failed to update cursor: {error:?}"),
                        };
                    }
                    commit.ops
                        .into_iter()
                        .filter(|operation|
                        operation.path.starts_with("app.bsky.feed.post/") ||
                            operation.path.starts_with("app.bsky.feed.like/") ||
                            operation.path.starts_with("app.bsky.graph.follow/"))
                        .map(|operation| {
                            let uri = format!("at://{}/{}",commit.repo,operation.path);
                            match operation.action.as_str() {
                                "update" => {},
                                "create" => {
                                    if let Some(cid) = operation.cid {
                                        let mut car_reader = Cursor::new(&commit.blocks);
                                        let _car_header = rsky_firehose::car::read_header(&mut car_reader).unwrap();
                                        let car_blocks = rsky_firehose::car::read_blocks(&mut car_reader).unwrap();

                                        let record_reader = Cursor::new(car_blocks.get(&cid).unwrap());
                                        match serde_cbor::from_reader(record_reader) {
                                            Ok(Lexicon::AppBskyFeedPost(r)) => {
                                                let post: Post = r;
                                                let mut create = rsky_firehose::models::CreateOp {
                                                    uri: uri.to_owned(),
                                                    cid: cid.to_string(),
                                                    sequence: commit.seq,
                                                    prev: None,
                                                    author: commit.repo.to_owned(),
                                                    record: post
                                                };
                                                if let Some(ref prev) = commit.prev {
                                                    create.prev = Some(prev.to_string());
                                                }
                                                posts_to_create.push(create);
                                            },
                                            Ok(Lexicon::AppBskyFeedLike(r)) => {
                                                let like: Like = r;
                                                let mut create = rsky_firehose::models::CreateOp {
                                                    uri: uri.to_owned(),
                                                    cid: cid.to_string(),
                                                    sequence: commit.seq,
                                                    prev: None,
                                                    author: commit.repo.to_owned(),
                                                    record: like
                                                };
                                                if let Some(ref prev) = commit.prev {
                                                    create.prev = Some(prev.to_string());
                                                }
                                                likes_to_create.push(create);
                                            },
                                            Ok(Lexicon::AppBskyFeedFollow(r)) => {
                                                let follow: Follow = r;
                                                let mut create = rsky_firehose::models::CreateOp {
                                                    uri: uri.to_owned(),
                                                    cid: cid.to_string(),
                                                    sequence: commit.seq,
                                                    prev: None,
                                                    author: commit.repo.to_owned(),
                                                    record: follow
                                                };
                                                if let Some(ref prev) = commit.prev {
                                                    create.prev = Some(prev.to_string());
                                                }
                                                follows_to_create.push(create);
                                            },
                                            Err(error) => {
                                                eprintln!("@LOG: Failed to deserialize record: {:?}. Received error {:?}. Sequence {:?}", uri, error, commit.seq);
                                            }
                                        }
                                    }
                                },
                                "delete" => {
                                    let del = rsky_firehose::models::DeleteOp {
                                        uri: uri.to_owned()
                                    };
                                    let collection = &operation.path
                                        .split("/")
                                        .map(String::from)
                                        .collect::<Vec<_>>()[0];
                                    if collection == "app.bsky.feed.post" {
                                        posts_to_delete.push(del);
                                    } else if collection == "app.bsky.feed.like" {
                                        likes_to_delete.push(del);
                                    } else if collection == "app.bsky.graph.follow" {
                                        follows_to_delete.push(del);
                                    }
                                },
                                _ => {}
                            }
                        })
                        .for_each(drop);
                }
                _ => println!("@LOG: Saw non-commit event: {body:?}"),
            }
            if posts_to_create.len() > 0 {
                let queue_endpoint = format!("{}/queue/{}/create", default_queue_path, "posts");
                let resp = queue_create(queue_endpoint, posts_to_create, client).await;
                match resp {
                    Ok(()) => (),
                    Err(error) => eprintln!("Records failed to queue: {error:?}"),
                };
            }
            if posts_to_delete.len() > 0 {
                let queue_endpoint = format!("{}/queue/{}/delete", default_queue_path, "posts");
                let resp = queue_delete(queue_endpoint, posts_to_delete, client).await;
                match resp {
                    Ok(()) => (),
                    Err(error) => eprintln!("Records failed to queue: {error:?}"),
                };
            }
            if likes_to_create.len() > 0 {
                let queue_endpoint = format!("{}/queue/{}/create", default_queue_path, "likes");
                let resp = queue_create(queue_endpoint, likes_to_create, client).await;
                match resp {
                    Ok(()) => (),
                    Err(error) => eprintln!("Records failed to queue: {error:?}"),
                };
            }
            if likes_to_delete.len() > 0 {
                let queue_endpoint = format!("{}/queue/{}/delete", default_queue_path, "likes");
                let resp = queue_delete(queue_endpoint, likes_to_delete, client).await;
                match resp {
                    Ok(()) => (),
                    Err(error) => eprintln!("Records failed to queue: {error:?}"),
                };
            }
            if follows_to_create.len() > 0 {
                let queue_endpoint = format!("{}/queue/{}/create", default_queue_path, "follows");
                let resp = queue_create(queue_endpoint, follows_to_create, client).await;
                match resp {
                    Ok(()) => (),
                    Err(error) => eprintln!("Records failed to queue: {error:?}"),
                };
            }
            if follows_to_delete.len() > 0 {
                let queue_endpoint = format!("{}/queue/{}/delete", default_queue_path, "follows");
                let resp = queue_delete(queue_endpoint, follows_to_delete, client).await;
                match resp {
                    Ok(()) => (),
                    Err(error) => eprintln!("Records failed to queue: {error:?}"),
                };
            }
        }
        Ok(None) => (),
        Err(error) => eprintln!(
            "@LOG: Error unwrapping message and header: {}",
            error.to_string()
        ),
    }
}

async fn process_labels(message: Vec<u8>, client: &reqwest::Client) {
    let default_queue_path =
        env::var("FEEDGEN_QUEUE_ENDPOINT").unwrap_or("https://[::1]:8081".into());
    let subscriber_base_path =
        env::var("FEEDGEN_SUBSCRIPTION_PATH").unwrap_or("wss://bsky.network".into());

    match rsky_firehose::firehose::read_labels(&message) {
        Ok((_header, body)) => {
            let mut labels_to_create = Vec::new();

            // update stored cursor every 20 events or so
            if (&body.seq).rem_euclid(20) == 0 {
                let cursor_endpoint = format!("{}/cursor", default_queue_path);
                let resp =
                    update_cursor(cursor_endpoint, subscriber_base_path, &body.seq, client).await;
                match resp {
                    Ok(()) => (),
                    Err(error) => eprintln!("@LOG: Failed to update cursor: {error:?}"),
                };
            }
            body.labels
                .into_iter()
                .filter(|label| {
                    label.uri.contains("app.bsky.feed.post") || label.uri.starts_with("did:plc:")
                })
                .map(|label| {
                    let create = rsky_firehose::models::CreateOp {
                        uri: label.uri.clone(),
                        cid: match label.cid {
                            None => "".to_string(),
                            Some(ref cid) => cid.clone(),
                        },
                        sequence: body.seq,
                        prev: None,
                        author: label.src.clone(),
                        record: label,
                    };
                    labels_to_create.push(create);
                })
                .for_each(drop);
            if labels_to_create.len() > 0 {
                let queue_endpoint = format!("{}/queue/{}/create", default_queue_path, "labels");
                let resp = queue_create(queue_endpoint, labels_to_create, client).await;
                match resp {
                    Ok(()) => (),
                    Err(error) => eprintln!("Records failed to queue: {error:?}"),
                };
            }
        }
        Err(error) => eprintln!(
            "@LOG: Error unwrapping message and header: {}",
            error.to_string()
        ),
    }
}

fn websocket_url(
    subscriber_base_path: &str,
    subscriber_endpoint: &str,
    subscriber_cursor: Option<&str>,
) -> Url {
    let url = format!("{}/xrpc/{}", subscriber_base_path, subscriber_endpoint);
    let mut ws_url = Url::parse(&url).expect("Invalid WebSocket URL");
    let query = subscriber_cursor
        .as_ref()
        .map(|cursor| format!("cursor={}", cursor));
    ws_url.set_query(query.as_deref());
    ws_url
}

#[tokio::main]
async fn main() {
    // Load environment variables from .env file
    dotenv().ok();

    // Retrieve the subscription endpoint from environment variables or use default
    let subscriber_base_path =
        env::var("FEEDGEN_SUBSCRIPTION_PATH").unwrap_or_else(|_| "wss://bsky.network".to_string());
    let subscriber_endpoint = env::var("FEEDGEN_SUBSCRIPTION_ENDPOINT")
        .unwrap_or_else(|_| "com.atproto.sync.subscribeRepos".to_string());
    let subscriber_cursor = env::var("FEEDGEN_SUBSCRIPTION_CURSOR").ok();

    // Configure the reqwest client with connection pooling settings
    let client = Arc::new(
        reqwest::Client::builder()
            .pool_max_idle_per_host(10) // Max idle connections per host
            .pool_idle_timeout(Duration::from_secs(30)) // Idle timeout
            .timeout(Duration::from_secs(10)) // Request timeout
            .build()
            .expect("Failed to build reqwest client"),
    );

    // Create a semaphore to limit the number of concurrent processing tasks
    let semaphore = Arc::new(Semaphore::new(100)); // Adjust the limit as needed

    // Construct the WebSocket URL
    let ws_url = websocket_url(
        &subscriber_base_path,
        &subscriber_endpoint,
        subscriber_cursor.as_deref(),
    );

    loop {
        // Attempt to establish a WebSocket connection
        match connect_async(&ws_url).await {
            Ok((mut socket, _response)) => {
                println!("Connected to {}", subscriber_base_path);

                // Listen for incoming messages
                while let Some(msg_result) = socket.next().await {
                    match msg_result {
                        Ok(Message::Binary(message)) => {
                            let client = Arc::clone(&client);
                            let semaphore = Arc::clone(&semaphore);

                            // Acquire a permit before spawning a new task
                            let permit = match semaphore.clone().acquire_owned().await {
                                Ok(permit) => permit,
                                Err(_) => {
                                    eprintln!("Semaphore closed");
                                    break;
                                }
                            };

                            // Spawn a new asynchronous task to process the message
                            tokio::spawn(async move {
                                // The permit is held for the duration of the task
                                let subscriber_endpoint = env::var("FEEDGEN_SUBSCRIPTION_ENDPOINT")
                                    .unwrap_or_else(|_| {
                                        "com.atproto.sync.subscribeRepos".to_string()
                                    });

                                match subscriber_endpoint.as_str() {
                                    "com.atproto.sync.subscribeRepos" => {
                                        process(message, &client).await
                                    }
                                    "com.atproto.label.subscribeLabels" => {
                                        process_labels(message, &client).await
                                    }
                                    _ => panic!(
                                        "Unexpected subscription endpoint: {subscriber_endpoint}"
                                    ),
                                };
                                // Permit is automatically released when it goes out of scope
                                drop(permit);
                            });
                        }
                        Ok(Message::Close(_)) => {
                            println!("WebSocket connection closed by server.");
                            break;
                        }
                        Ok(_) => {
                            // Handle other message types if necessary
                        }
                        Err(e) => {
                            eprintln!("WebSocket error: {}", e);
                            break;
                        }
                    }
                }
            }
            Err(error) => {
                eprintln!(
                    "Error connecting to {}. Waiting to reconnect: {:?}",
                    subscriber_base_path, error
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
