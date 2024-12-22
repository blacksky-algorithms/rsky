use dotenvy::dotenv;
use futures::StreamExt as _;
use rsky_common::explicit_slurs::contains_explicit_slurs;
use rsky_lexicon::app::bsky::actor::Profile;
use rsky_lexicon::app::bsky::feed::Post;
use rsky_lexicon::com::atproto::sync::SubscribeRepos;
use serde_derive::Deserialize;
use std::env;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message;

#[derive(Debug, Deserialize)]
#[serde(tag = "$type")]
enum Lexicon {
    #[serde(rename(deserialize = "app.bsky.feed.post"))]
    AppBskyFeedPost(Post),
    #[serde(rename(deserialize = "app.bsky.actor.profile"))]
    AppBskyActorProfile(Profile),
}

async fn process(message: Vec<u8>) {
    match rsky_labeler::firehose::read(&message) {
        Ok((_header, body)) => {
            let mut posts_to_label = Vec::new();
            let mut profiles_to_label = Vec::new();
            let mut handles_to_label = Vec::new();

            match body {
                SubscribeRepos::Commit(commit) => {
                    if commit.too_big {
                        println!("Too big.");
                    }
                    commit.ops
                        .into_iter()
                        .filter(|operation|
                        operation.path.starts_with("app.bsky.feed.post/") ||
                            operation.path.starts_with("app.bsky.actor.profile/"))
                        .map(|operation| {
                            let uri = format!("at://{}/{}",commit.repo,operation.path);
                            match operation.action.as_str() {
                                "create" | "update" => {
                                    if let Some(cid) = operation.cid {
                                        let mut car_reader = Cursor::new(&commit.blocks);
                                        let _car_header = rsky_labeler::car::read_header(&mut car_reader).unwrap();
                                        let car_blocks = rsky_labeler::car::read_blocks(&mut car_reader).unwrap();

                                        let record_reader = Cursor::new(car_blocks.get(&cid).unwrap());
                                        match serde_cbor::from_reader(record_reader) {
                                            Ok(Lexicon::AppBskyFeedPost(r)) => {
                                                let post: Post = r;
                                                if contains_explicit_slurs(post.text.as_str()) {
                                                    posts_to_label.push(post);
                                                }
                                            },
                                            Ok(Lexicon::AppBskyActorProfile(r)) => {
                                                let profile: Profile = r;
                                                let mut profile_should_be_labeled = false;
                                                if let Some(ref display_name) = profile.display_name {
                                                    if contains_explicit_slurs(display_name.as_str()) {
                                                        profile_should_be_labeled = true;
                                                    }
                                                }
                                                if let Some(ref description) = profile.description {
                                                    if contains_explicit_slurs(description.as_str()) {
                                                        profile_should_be_labeled = true;
                                                    }
                                                }
                                                if profile_should_be_labeled {
                                                    profiles_to_label.push(profile);
                                                }
                                            },
                                            Err(_) => ()
                                        }
                                    }
                                },
                                _ => {}
                            }
                        })
                        .for_each(drop);
                }
                SubscribeRepos::Identity(identity) => {
                    if let Some(ref handle) = identity.handle {
                        if contains_explicit_slurs(handle.as_str()) {
                            handles_to_label.push(identity);
                        }
                    }
                }
                _ => (),
            }
            if posts_to_label.len() > 0 {
                println!("Count posts to label {}", posts_to_label.len());
                let text = posts_to_label
                    .iter()
                    .map(|p| &p.text)
                    .collect::<Vec<&String>>();
                println!("Posts to label {text:?}");
            }
            if profiles_to_label.len() > 0 {
                println!("Count profiles to label {}", profiles_to_label.len());
                println!("Profiles to label {profiles_to_label:#?}");
            }
            if handles_to_label.len() > 0 {
                println!("Count handles to label {}", handles_to_label.len());
                let handles = handles_to_label
                    .into_iter()
                    .filter_map(|h| h.handle)
                    .collect::<Vec<String>>();
                println!("Posts to label {handles:?}");
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
    // Load environment variables from .env file
    dotenv().ok();

    // Retrieve the subscription endpoint from environment variables or use default
    let subscriber_base_path =
        env::var("FEEDGEN_SUBSCRIPTION_PATH").unwrap_or_else(|_| "wss://bsky.network".to_string());
    let subscriber_endpoint = env::var("FEEDGEN_SUBSCRIPTION_ENDPOINT")
        .unwrap_or_else(|_| "com.atproto.sync.subscribeRepos".to_string());

    // Create a semaphore to limit the number of concurrent processing tasks
    let semaphore = Arc::new(Semaphore::new(100)); // Adjust the limit as needed

    loop {
        // Construct the WebSocket URL
        let ws_url = format!("{}/xrpc/{}", subscriber_base_path, subscriber_endpoint)
            .into_client_request()
            .expect("Invalid WebSocket URL");

        // Attempt to establish a WebSocket connection
        match connect_async(ws_url).await {
            Ok((mut socket, _response)) => {
                println!("Connected to {}", subscriber_base_path);

                // Listen for incoming messages
                while let Some(msg_result) = socket.next().await {
                    match msg_result {
                        Ok(Message::Binary(message)) => {
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
                                        process(message.to_vec()).await
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
