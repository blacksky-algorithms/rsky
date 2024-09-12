#![allow(unused_imports)]

use dotenvy::dotenv;
use futures::StreamExt as _;
use libipld::Cid;
use rsky_lexicon::app::bsky::feed::like::Like;
use rsky_lexicon::app::bsky::feed::Post;
use rsky_lexicon::app::bsky::graph::follow::Follow;
use rsky_lexicon::com::atproto::sync::SubscribeRepos;
use serde::Deserialize;
use std::env;
use std::io::Cursor;
use std::str::FromStr;
use std::{thread, time::Duration};
use tokio::net::TcpStream;
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
    let default_subscriber_path =
        env::var("FEEDGEN_SUBSCRIPTION_ENDPOINT").unwrap_or("wss://bsky.social".into());

    match rsky_firehose::firehose::read(&message) {
        Ok((_header, body)) => {
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
                    // update stored cursor every 20 events or so
                    if (&commit.seq).rem_euclid(20) == 0 {
                        let cursor_endpoint = format!("{}/cursor", default_queue_path);
                        let resp = update_cursor(
                            cursor_endpoint,
                            default_subscriber_path,
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
        Err(error) => eprintln!(
            "@LOG: Error unwrapping message and header: {}",
            error.to_string()
        ),
    }
}

#[tokio::main]
async fn main() {
    match dotenvy::dotenv() {
        _ => (),
    };

    let default_subscriber_path =
        env::var("FEEDGEN_SUBSCRIPTION_ENDPOINT").unwrap_or("wss://bsky.social".into());
    let client = reqwest::Client::new();
    loop {
        match tokio_tungstenite::connect_async(
            Url::parse(
                format!(
                    "{}/xrpc/com.atproto.sync.subscribeRepos?cursor=1670190430",
                    default_subscriber_path
                )
                .as_str(),
            )
            .unwrap(),
        )
        .await
        {
            Ok((mut socket, _response)) => {
                println!("Connected to {default_subscriber_path:?}.");
                while let Some(Ok(Message::Binary(message))) = socket.next().await {
                    let client = client.clone();
                    tokio::spawn(async move {
                        process(message, &client).await;
                    });
                    thread::sleep(Duration::from_millis(8));
                }
            }
            Err(error) => {
                eprintln!("Error connecting to {default_subscriber_path:?}. Waiting to reconnect: {error:?}");
                thread::sleep(Duration::from_millis(500));
                continue;
            }
        }
    }
}
