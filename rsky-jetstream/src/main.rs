use chrono::DateTime;
use dotenvy::dotenv;
use futures::StreamExt as _;
use rsky_jetstream::jetstream::{
    read, JetstreamRepoAccount, JetstreamRepoAccountMessage, JetstreamRepoCommit,
    JetstreamRepoCommitMessage, JetstreamRepoIdentity, JetstreamRepoIdentityMessage,
    JetstreamRepoMessage, Lexicon,
};
use rsky_lexicon::app::bsky::feed::like::Like;
use rsky_lexicon::app::bsky::feed::{Post, Repost};
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

async fn queue_delete(
    url: String,
    records: Vec<rsky_jetstream::models::DeleteOp>,
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
    records: Vec<rsky_jetstream::models::CreateOp<T>>,
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

#[tracing::instrument]
async fn process(message: String, client: &reqwest::Client) {
    let default_queue_path =
        env::var("FEEDGEN_QUEUE_ENDPOINT").unwrap_or("http://127.0.0.1:8000".into());
    let default_subscriber_path = env::var("JETSTREAM_SUBSCRIPTION_ENDPOINT")
        .unwrap_or("wss://jetstream1.us-west.bsky.network".into());

    match read(&message) {
        Ok(body) => {
            let mut posts_to_delete = Vec::new();
            let mut posts_to_create = Vec::new();
            let mut reposts_to_delete = Vec::new();
            let mut reposts_to_create = Vec::new();
            let mut likes_to_delete = Vec::new();
            let mut likes_to_create = Vec::new();
            let mut follows_to_delete = Vec::new();
            let mut follows_to_create = Vec::new();

            match body {
                JetstreamRepoMessage::Commit(commit) => {
                    if commit.kind.is_empty() {
                        tracing::info!("Operations empty.");
                    }
                    // update stored cursor every 20 events or so
                    if (&commit.time_us).rem_euclid(20) == 0 {
                        let cursor_endpoint = format!("{}/cursor", default_queue_path);
                        let resp = update_cursor(
                            cursor_endpoint,
                            default_subscriber_path,
                            &commit.time_us,
                            client,
                        )
                        .await;
                        match resp {
                            Ok(()) => (),
                            Err(error) => {
                                tracing::error!("@LOG: Failed to update cursor: {error:?}")
                            }
                        };
                    }

                    match commit.commit.operation.as_str() {
                        "update" => {}
                        "create" => {
                            let cid = commit.commit.cid;
                            match commit.commit.record {
                                Some(Lexicon::AppBskyFeedPost(r)) => {
                                    let post: Post = r;
                                    let uri = String::from("at://")
                                        + commit.did.as_str()
                                        + "/app.bsky.feed.post/"
                                        + commit.commit.rkey.as_str();
                                    let create = rsky_jetstream::models::CreateOp {
                                        uri: uri.to_owned(),
                                        cid: cid.unwrap().to_string(),
                                        author: commit.did.to_owned(),
                                        record: post,
                                    };
                                    posts_to_create.push(create);
                                }
                                Some(Lexicon::AppBskyFeedRepost(r)) => {
                                    let repost: Repost = r;
                                    let uri = String::from("at://")
                                        + commit.did.as_str()
                                        + "/app.bsky.feed.repost/"
                                        + commit.commit.rkey.as_str();
                                    let create = rsky_jetstream::models::CreateOp {
                                        uri: uri.to_owned(),
                                        cid: cid.unwrap().to_string(),
                                        author: commit.did.to_owned(),
                                        record: repost,
                                    };
                                    reposts_to_create.push(create);
                                }
                                Some(Lexicon::AppBskyFeedLike(r)) => {
                                    let like: Like = r;
                                    let uri = String::from("at://")
                                        + commit.did.as_str()
                                        + "/app.bsky.feed.like/"
                                        + commit.commit.rkey.as_str();
                                    let create = rsky_jetstream::models::CreateOp {
                                        uri: uri.to_owned(),
                                        cid: cid.unwrap().to_string(),
                                        author: commit.did.to_owned(),
                                        record: like,
                                    };
                                    likes_to_create.push(create);
                                }
                                Some(Lexicon::AppBskyFeedFollow(r)) => {
                                    let follow: Follow = r;
                                    let uri = String::from("at://")
                                        + commit.did.as_str()
                                        + "/app.bsky.graph.follow/"
                                        + commit.commit.rkey.as_str();
                                    let create = rsky_jetstream::models::CreateOp {
                                        uri: uri.to_owned(),
                                        cid: cid.unwrap().to_string(),
                                        author: commit.did.to_owned(),
                                        record: follow,
                                    };
                                    follows_to_create.push(create);
                                }
                                _ => {}
                            }
                        }
                        "delete" => {
                            let collection = commit.commit.collection;
                            if collection == "app.bsky.feed.post" {
                                let uri = String::from("at://")
                                    + commit.did.as_str()
                                    + "/app.bsky.feed.post/"
                                    + commit.commit.rkey.as_str();
                                let del = rsky_jetstream::models::DeleteOp { uri: uri };
                                posts_to_delete.push(del);
                            } else if collection == "app.bsky.feed.repost" {
                                let uri = String::from("at://")
                                    + commit.did.as_str()
                                    + "/app.bsky.feed.repost/"
                                    + commit.commit.rkey.as_str();
                                let del = rsky_jetstream::models::DeleteOp { uri: uri };
                                reposts_to_delete.push(del);
                            } else if collection == "app.bsky.feed.like" {
                                let uri = String::from("at://")
                                    + commit.did.as_str()
                                    + "/app.bsky.feed.like/"
                                    + commit.commit.rkey.as_str();
                                let del = rsky_jetstream::models::DeleteOp { uri: uri };
                                likes_to_delete.push(del);
                            } else if collection == "app.bsky.graph.follow" {
                                let uri = String::from("at://")
                                    + commit.did.as_str()
                                    + "/app.bsky.graph.follow/"
                                    + commit.commit.rkey.as_str();
                                let del = rsky_jetstream::models::DeleteOp { uri: uri };
                                follows_to_delete.push(del);
                            }
                        }
                        _ => {}
                    }
                }
                JetstreamRepoMessage::Identity(_) => {}
                JetstreamRepoMessage::Account(_) => {}
            }

            if posts_to_create.len() > 0 {
                let queue_endpoint = format!("{}/queue/{}/create", default_queue_path, "posts");
                let resp = queue_create(queue_endpoint, posts_to_create, client).await;
                match resp {
                    Ok(()) => (),
                    Err(error) => tracing::error!("Records failed to queue: {error:?}"),
                };
            }
            if posts_to_delete.len() > 0 {
                let queue_endpoint = format!("{}/queue/{}/delete", default_queue_path, "posts");
                let resp = queue_delete(queue_endpoint, posts_to_delete, client).await;
                match resp {
                    Ok(()) => (),
                    Err(error) => tracing::error!("Records failed to queue: {error:?}"),
                };
            }
            if reposts_to_create.len() > 0 {
                let queue_endpoint = format!("{}/queue/{}/create", default_queue_path, "reposts");
                let resp = queue_create(queue_endpoint, reposts_to_create, client).await;
                match resp {
                    Ok(()) => (),
                    Err(error) => tracing::error!("Records failed to queue: {error:?}"),
                };
            }
            if reposts_to_delete.len() > 0 {
                let queue_endpoint = format!("{}/queue/{}/delete", default_queue_path, "reposts");
                let resp = queue_delete(queue_endpoint, reposts_to_delete, client).await;
                match resp {
                    Ok(()) => (),
                    Err(error) => tracing::error!("Records failed to queue: {error:?}"),
                };
            }
            if likes_to_create.len() > 0 {
                let queue_endpoint = format!("{}/queue/{}/create", default_queue_path, "likes");
                let resp = queue_create(queue_endpoint, likes_to_create, client).await;
                match resp {
                    Ok(()) => (),
                    Err(error) => tracing::error!("Records failed to queue: {error:?}"),
                };
            }
            if likes_to_delete.len() > 0 {
                let queue_endpoint = format!("{}/queue/{}/delete", default_queue_path, "likes");
                let resp = queue_delete(queue_endpoint, likes_to_delete, client).await;
                match resp {
                    Ok(()) => (),
                    Err(error) => tracing::error!("Records failed to queue: {error:?}"),
                };
            }
            if follows_to_create.len() > 0 {
                let queue_endpoint = format!("{}/queue/{}/create", default_queue_path, "follows");
                let resp = queue_create(queue_endpoint, follows_to_create, client).await;
                match resp {
                    Ok(()) => (),
                    Err(error) => tracing::error!("Records failed to queue: {error:?}"),
                };
            }
            if follows_to_delete.len() > 0 {
                let queue_endpoint = format!("{}/queue/{}/delete", default_queue_path, "follows");
                let resp = queue_delete(queue_endpoint, follows_to_delete, client).await;
                match resp {
                    Ok(()) => (),
                    Err(error) => tracing::error!("Records failed to queue: {error:?}"),
                };
            }
        }
        Err(error) => tracing::error!(
            "@LOG: Error unwrapping message and header: {}",
            error.to_string()
        ),
    }
}

#[tracing::instrument]
#[tokio::main]
async fn main() {
    let default_subscriber_path = env::var("JETSTREAM_SERVER_ENDPOINT")
        .unwrap_or("wss://jetstream1.us-west.bsky.network".into());
    let wanted_collections = env::var("FILTER_PARAM")
        .unwrap_or("".into());
    let client = reqwest::Client::new();
    let subscriber = tracing_subscriber::FmtSubscriber::new();
    tracing::subscriber::set_global_default(subscriber).unwrap();
    loop {
        match tokio_tungstenite::connect_async(
            Url::parse(
                format!(
                    "{sub}/subscribe?{filter}",
                    sub = default_subscriber_path,
                    filter = wanted_collections
                )
                .as_str(),
            )
            .unwrap(),
        )
        .await
        {
            Ok((mut socket, _response)) => {
                tracing::info!("Connected to {default_subscriber_path:?}.");
                while let Some(Ok(Message::Text(message))) = socket.next().await {
                    let client = client.clone();
                    tokio::spawn(async move {
                        process(message, &client).await;
                    });
                }
            }
            Err(error) => {
                tracing::error!("Error connecting to {default_subscriber_path:?}. Waiting to reconnect: {error:?}");
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        }
    }
}