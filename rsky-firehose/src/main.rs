#![allow(unused_imports)]

use rsky_firehose::lexicon::app::bsky::feed::Post;
use rsky_firehose::lexicon::com::atproto::sync::SubscribeRepos;
use futures::StreamExt as _;
use std::io::Cursor;
use tokio_tungstenite::tungstenite::protocol::Message;
use url::Url;
use dotenvy::dotenv;
use std::env;


async fn queue_delete(
    url: String,
    records: Vec<rsky_firehose::models::DeleteOp>
) -> Result<(), Box<dyn std::error::Error>> {
    let token = env::var("RSKY_API_KEY").map_err(|_| {
        "Pass a valid preshared token via `RSKY_API_KEY` environment variable.".to_string()
    })?;
    reqwest::Client::new()
        .put(url)
        .json(&records)
        .header("X-RSKY-KEY", token)
        .send()
        .await?;
    Ok(())
}

async fn queue_create<T: serde::ser::Serialize>(
    url: String,
    records: Vec<rsky_firehose::models::CreateOp<T>>
) -> Result<(), Box<dyn std::error::Error>> {
    let token = env::var("RSKY_API_KEY").map_err(|_| {
        "Pass a valid preshared token via `RSKY_API_KEY` environment variable.".to_string()
    })?;
    reqwest::Client::new()
        .put(url)
        .json(&records)
        .header("X-RSKY-KEY", token)
        .send()
        .await?;
    Ok(())
}


#[tokio::main]
async fn main() {
    match dotenvy::dotenv() {
        _ => ()
    };

    let mut default_subscriber_path = "wss://bsky.social".to_owned();
    let mut default_queue_path = "https://[::1]:8081".to_owned();

    if let Ok(subscription_endpoint) = env::var("FEEDGEN_SUBSCRIPTION_ENDPOINT") {
        default_subscriber_path = subscription_endpoint.clone();
    }
    if let Ok(queue_endpoint) = env::var("FEEDGEN_QUEUE_ENDPOINT") {
        default_queue_path = queue_endpoint.clone();
    }

    let (mut socket, _response) = tokio_tungstenite::connect_async(
        Url::parse(
            format!(
                "{}/xrpc/com.atproto.sync.subscribeRepos",
                default_subscriber_path
            )
            .as_str()
        )
        .unwrap(),
    )
    .await
    .unwrap();

    while let Some(Ok(Message::Binary(message))) = socket.next().await {
        let (_header, body) = rsky_firehose::firehose::read(&message).unwrap();
        let mut posts_to_delete = Vec::new();
        let mut posts_to_create = Vec::new();

        match body {
            SubscribeRepos::Commit(commit) => {
                if commit.operations.is_empty() {
                    continue;
                }
                commit.operations
                    .into_iter()
                    .filter(|operation| operation.path.starts_with("app.bsky.feed.post/"))
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
                                        Ok(post) => {
                                            let post: Post = post;
                                            let _collection = &operation.path // Placeholder. Can be used to filter by lexicon
                                                .split("/")
                                                .map(String::from)
                                                .collect::<Vec<_>>()[0];
                                            let create = rsky_firehose::models::CreateOp {
                                                uri: uri.to_owned(),
                                                cid: cid.to_string(),
                                                author: commit.repo.to_owned(),
                                                record: post
                                            };
                                            posts_to_create.push(create);
                                        },
                                        Err(error) => {
                                            eprintln!("Failed to deserialize record: {uri:?}. Received error {error:?}");
                                        }
                                    }
                                }
                            },
                            "delete" => {
                                let del = rsky_firehose::models::DeleteOp {
                                    uri: uri.to_owned()
                                };
                                posts_to_delete.push(del);
                            },
                            _ => {}
                        }
                    })
                    .for_each(drop);
            }
            _ => {}
        }
        if posts_to_create.len() > 0 {
            println!("Create: {posts_to_create:?}");
            let queue_endpoint = format!("{}/queue/create",default_queue_path);
            let resp = queue_create(queue_endpoint, posts_to_create).await;
            match resp {
                Ok(()) => println!("Records queued succesfully."),
                Err(error) => eprintln!("Records failed to queue: {error:?}")
            };
        }
        if posts_to_delete.len() > 0 {
            println!("Delete: {posts_to_delete:?}");
            let queue_endpoint = format!("{}/queue/delete",default_queue_path);
            let resp = queue_delete(queue_endpoint, posts_to_delete).await;
            match resp {
                Ok(()) => println!("Records queued succesfully."),
                Err(error) => eprintln!("Records failed to queue: {error:?}")
            };
        }
    }
}