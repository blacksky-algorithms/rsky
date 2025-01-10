use atrium_api::agent::store::MemorySessionStore;
use atrium_api::agent::AtpAgent;
use atrium_api::com::atproto::admin::defs::{RepoRef, RepoRefData};
use atrium_api::com::atproto::moderation::create_report::{
    Input as ComAtprotoModerationCreateReportInput,
    InputData as ComAtprotoModerationCreateReportData,
    InputSubjectRefs as CreateReportInputSubjectRefs,
};
use atrium_api::com::atproto::repo::strong_ref::{Main as StrongRef, MainData as StrongRefData};
use atrium_api::tools::ozone::moderation::defs::{
    ModEventLabel, ModEventLabelData, ModEventTag, ModEventTagData,
};
use atrium_api::tools::ozone::moderation::emit_event::InputEventRefs::ToolsOzoneModerationDefsModEventTag;
use atrium_api::tools::ozone::moderation::emit_event::{
    Input as ToolsOzoneModerationEmitEventInput, InputData as ToolsOzoneModerationEmitEventData,
    InputSubjectRefs,
};
use atrium_api::tools::ozone::moderation::emit_event::{
    InputEventRefs::ToolsOzoneModerationDefsModEventLabel,
    InputSubjectRefs as EmitEventInputSubjectRefs,
};
use atrium_api::tools::ozone::moderation::get_record::{Parameters, ParametersData};
use atrium_api::types::string::Did;
use atrium_api::types::Union;
use atrium_api::xrpc::http::HeaderMap;
use atrium_ipld::ipld::Ipld as AtriumIpld;
use atrium_xrpc_client::reqwest::{ReqwestClient, ReqwestClientBuilder};
use dotenvy::dotenv;
use futures::StreamExt as _;
use rsky_common::env::{env_bool, env_list};
use rsky_common::explicit_slurs::contains_explicit_slurs;
use rsky_labeler::APP_USER_AGENT;
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

#[derive(Debug, Clone)]
struct AutoReport {
    subject_ref: EmitEventInputSubjectRefs,
    reason: Option<String>,
}

fn get_agent(
) -> Result<Arc<AtpAgent<MemorySessionStore, ReqwestClient>>, Box<dyn std::error::Error>> {
    let agent_url =
        env::var("BSKY_AGENT_URL").unwrap_or_else(|_| "https://bsky.social".to_string());
    let mut headers = HeaderMap::new();
    headers.insert(
        "atproto-proxy",
        format!(
            "{}#atproto_labeler",
            env::var("MOD_SERVICE_DID").expect("Mod service DID should be set.")
        )
        .parse()?,
    );
    let client = ReqwestClientBuilder::new(agent_url)
        .client(
            reqwest::ClientBuilder::new()
                .user_agent(APP_USER_AGENT)
                .timeout(Duration::from_millis(1000))
                .default_headers(headers)
                .build()?,
        )
        .build();
    let agent = Arc::new(AtpAgent::new(client, MemorySessionStore::default()));
    Ok(agent)
}

async fn get_labels(
    agent: &AtpAgent<MemorySessionStore, ReqwestClient>,
    subject_ref: &EmitEventInputSubjectRefs,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let cid = match subject_ref {
        InputSubjectRefs::ComAtprotoAdminDefsRepoRef(_) => None,
        InputSubjectRefs::ComAtprotoRepoStrongRefMain(ref strong_ref) => {
            Some(strong_ref.cid.clone())
        }
    };
    let uri = match subject_ref {
        InputSubjectRefs::ComAtprotoAdminDefsRepoRef(ref repo_ref) => {
            repo_ref.did.clone().to_string()
        }
        InputSubjectRefs::ComAtprotoRepoStrongRefMain(ref strong_ref) => strong_ref.uri.clone(),
    };
    match agent
        .api
        .tools
        .ozone
        .moderation
        .get_record(Parameters {
            data: ParametersData { cid, uri },
            extra_data: AtriumIpld::Null,
        })
        .await
    {
        Ok(result) => match result.labels {
            None => Ok(vec![]),
            Some(ref labels) => Ok(labels.iter().map(|l| l.val.clone()).collect()),
        },
        Err(error) => {
            eprintln!("@LOG: Failed to fetch mod record: {error:?}");
            Ok(vec![])
        }
    }
}

async fn label_subject(
    agent: &AtpAgent<MemorySessionStore, ReqwestClient>,
    subject_ref: EmitEventInputSubjectRefs,
) -> Result<(), Box<dyn std::error::Error>> {
    let label_result = agent
        .api
        .tools
        .ozone
        .moderation
        .emit_event(ToolsOzoneModerationEmitEventInput {
            data: ToolsOzoneModerationEmitEventData {
                created_by: Did::new(
                    env::var("MOD_SERVICE_DID").expect("Mod service DID should be set."),
                )?,
                event: Union::Refs(ToolsOzoneModerationDefsModEventLabel(Box::new(
                    ModEventLabel {
                        data: ModEventLabelData {
                            comment: Some(
                                env::var("MOD_SERVICE_LABEL_REASON")
                                    .unwrap_or("Explicit slur filter".to_string()),
                            ),
                            create_label_vals: vec![env::var("MOD_SERVICE_LABEL")
                                .unwrap_or("antiblack-harassment".to_string())],
                            negate_label_vals: vec![],
                        },
                        extra_data: AtriumIpld::Null,
                    },
                ))),
                subject: Union::Refs(subject_ref),
                subject_blob_cids: None,
            },
            extra_data: AtriumIpld::Null,
        })
        .await?;
    println!("@LOG: Label result {label_result:?}");
    Ok(())
}

async fn tag_subject(
    agent: &AtpAgent<MemorySessionStore, ReqwestClient>,
    subject_ref: EmitEventInputSubjectRefs,
) -> Result<(), Box<dyn std::error::Error>> {
    let _ = agent
        .api
        .tools
        .ozone
        .moderation
        .emit_event(ToolsOzoneModerationEmitEventInput {
            data: ToolsOzoneModerationEmitEventData {
                created_by: Did::new(
                    env::var("MOD_SERVICE_DID").expect("Mod service DID should be set."),
                )?,
                event: Union::Refs(ToolsOzoneModerationDefsModEventTag(Box::new(ModEventTag {
                    data: ModEventTagData {
                        add: env_list("MOD_SERVICE_AUTOLABEL_TAGS"),
                        comment: None,
                        remove: vec![],
                    },
                    extra_data: AtriumIpld::Null,
                }))),
                subject: Union::Refs(subject_ref),
                subject_blob_cids: None,
            },
            extra_data: AtriumIpld::Null,
        })
        .await?;
    Ok(())
}

async fn create_report(
    agent: &AtpAgent<MemorySessionStore, ReqwestClient>,
    subject_ref: EmitEventInputSubjectRefs,
    reason: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let report_result = agent
        .api
        .com
        .atproto
        .moderation
        .create_report(ComAtprotoModerationCreateReportInput {
            data: ComAtprotoModerationCreateReportData {
                reason,
                reason_type: env::var("MOD_SERVICE_REASON")
                    .unwrap_or("com.atproto.moderation.defs#reasonRude".to_string()),
                subject: match subject_ref {
                    EmitEventInputSubjectRefs::ComAtprotoAdminDefsRepoRef(repo_ref) => Union::Refs(
                        CreateReportInputSubjectRefs::ComAtprotoAdminDefsRepoRef(repo_ref),
                    ),
                    EmitEventInputSubjectRefs::ComAtprotoRepoStrongRefMain(strong_ref) => {
                        Union::Refs(CreateReportInputSubjectRefs::ComAtprotoRepoStrongRefMain(
                            strong_ref,
                        ))
                    }
                },
            },
            extra_data: AtriumIpld::Null,
        })
        .await?;
    println!("@LOG: Report result {report_result:?}");
    Ok(())
}

async fn process(
    message: Vec<u8>,
    agent: &AtpAgent<MemorySessionStore, ReqwestClient>,
) -> Result<(), Box<dyn std::error::Error>> {
    match rsky_labeler::firehose::read(&message) {
        Ok((_header, body)) => {
            let mut auto_reports: Vec<AutoReport> = Vec::new();

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
                                "create" | "update" => { // @TODO: For profile updates, only want to action if changed(?)
                                    if let Some(cid) = operation.cid {
                                        let mut car_reader = Cursor::new(&commit.blocks);
                                        let _car_header = rsky_labeler::car::read_header(&mut car_reader).unwrap();
                                        let car_blocks = rsky_labeler::car::read_blocks(&mut car_reader).unwrap();

                                        let record_reader = Cursor::new(car_blocks.get(&cid).unwrap());
                                        match serde_cbor::from_reader(record_reader) {
                                            Ok(Lexicon::AppBskyFeedPost(r)) => {
                                                let post: Post = r;
                                                if contains_explicit_slurs(post.text.as_str()) {
                                                    auto_reports.push(
                                                        AutoReport {
                                                            subject_ref: EmitEventInputSubjectRefs::ComAtprotoRepoStrongRefMain(Box::new(StrongRef {
                                                                data: StrongRefData {
                                                                    cid: cid.to_string().parse().unwrap(),
                                                                    uri,
                                                                },
                                                                extra_data: AtriumIpld::Null,
                                                            })),
                                                            reason: Some(format!(
                                                                "{}: user made bluesky post `{}`",
                                                                env::var("MOD_SERVICE_COMMENT").unwrap_or("Explicit slur filter".to_string()),
                                                                post.text.as_str()
                                                            ))
                                                        });
                                                }
                                            },
                                            Ok(Lexicon::AppBskyActorProfile(r)) => {
                                                let profile: Profile = r;
                                                let mut reason: Option<String> = None;
                                                if let Some(ref display_name) = profile.display_name {
                                                    if contains_explicit_slurs(display_name.as_str()) {
                                                        reason = Some(format!(
                                                            "{}: user's bluesky display name is `{}`",
                                                            env::var("MOD_SERVICE_COMMENT").unwrap_or("Explicit slur filter".to_string()),
                                                            display_name.as_str()
                                                        ))
                                                    }
                                                }
                                                if let Some(ref description) = profile.description {
                                                    if contains_explicit_slurs(description.as_str()) {
                                                        reason = Some(format!(
                                                            "{}: user's bluesky profile description is `{}`",
                                                            env::var("MOD_SERVICE_COMMENT").unwrap_or("Explicit slur filter".to_string()),
                                                            description.as_str()
                                                        ))
                                                    }
                                                }
                                                if let Some(reason) = reason {
                                                    auto_reports.push(
                                                        AutoReport {
                                                            subject_ref: EmitEventInputSubjectRefs::ComAtprotoRepoStrongRefMain(Box::new(StrongRef {
                                                                data: StrongRefData {
                                                                    cid: cid.to_string().parse().unwrap(),
                                                                    uri,
                                                                },
                                                                extra_data: AtriumIpld::Null,
                                                            })),
                                                            reason: Some(reason)
                                                        }
                                                    );
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
                        if contains_explicit_slurs(handle.as_str())
                            || contains_explicit_slurs(
                                handle
                                    .replace(".", "")
                                    .replace("-", "")
                                    .replace("_", "")
                                    .as_str(),
                            )
                        {
                            auto_reports.push(AutoReport {
                                subject_ref: EmitEventInputSubjectRefs::ComAtprotoAdminDefsRepoRef(
                                    Box::new(RepoRef {
                                        data: RepoRefData {
                                            did: Did::new(identity.did)?,
                                        },
                                        extra_data: AtriumIpld::Null,
                                    }),
                                ),
                                reason: Some(format!(
                                    "{}: account's handle is `{}`",
                                    env::var("MOD_SERVICE_COMMENT")
                                        .unwrap_or("Explicit slur filter".to_string()),
                                    handle.as_str()
                                )),
                            });
                        }
                    }
                }
                _ => (),
            }
            if auto_reports.len() > 0 {
                for auto_report in auto_reports {
                    let label =
                        env::var("MOD_SERVICE_LABEL").unwrap_or("antiblack-harassment".to_string());
                    let existing_labels = get_labels(agent, &auto_report.subject_ref).await?; // Known issue with atrium making this call fail.
                    if existing_labels.contains(&label) {
                        println!(
                            "@LOG: Subject already labeled as {label} {:?}",
                            auto_report.subject_ref
                        );
                        continue;
                    }
                    if env_bool("ENABLE_CREATE_REPORT").unwrap_or(true) {
                        match create_report(
                            agent,
                            auto_report.subject_ref.clone(),
                            auto_report.reason,
                        )
                        .await
                        {
                            Ok(()) => (),
                            Err(error) => {
                                eprintln!("@LOG: Failed to create report for record: {error:?}")
                            }
                        }
                    }
                    if env_bool("ENABLE_CREATE_LABEL").unwrap_or(true) {
                        match label_subject(agent, auto_report.subject_ref.clone()).await {
                            Ok(()) => (),
                            Err(error) => eprintln!("@LOG: Failed to label record: {error:?}"),
                        }
                    }
                    if env_bool("ENABLE_CREATE_TAG").unwrap_or(true) {
                        match tag_subject(agent, auto_report.subject_ref).await {
                            Ok(()) => (),
                            Err(error) => eprintln!("@LOG: Failed to tag record: {error:?}"),
                        }
                    }
                }
            }
        }
        Err(error) => eprintln!(
            "@LOG: Error unwrapping message and header: {}",
            error.to_string()
        ),
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment variables from .env file
    dotenv().ok();

    // Retrieve the subscription endpoint from environment variables or use default
    let subscriber_base_path =
        env::var("FEEDGEN_SUBSCRIPTION_PATH").unwrap_or_else(|_| "wss://bsky.network".to_string());
    let subscriber_endpoint = env::var("FEEDGEN_SUBSCRIPTION_ENDPOINT")
        .unwrap_or_else(|_| "com.atproto.sync.subscribeRepos".to_string());

    let agent = get_agent()?;
    agent
        .login(
            env::var("MOD_SERVICE_EMAIL").expect("Mod service email should be set."),
            env::var("MOD_SERVICE_PASSWORD").expect("Mod service password should be set."),
        )
        .await?;

    // Create a semaphore to limit the number of concurrent processing tasks
    let semaphore = Arc::new(Semaphore::new(100)); // Adjust the limit as needed

    loop {
        // Construct the WebSocket URL
        let ws_url = format!("{}/xrpc/{}", subscriber_base_path, subscriber_endpoint)
            .into_client_request()?;

        // Attempt to establish a WebSocket connection
        match connect_async(ws_url).await {
            Ok((mut socket, _response)) => {
                println!("Connected to {}", subscriber_base_path);

                // Listen for incoming messages
                while let Some(msg_result) = socket.next().await {
                    match msg_result {
                        Ok(Message::Binary(message)) => {
                            let agent = Arc::clone(&agent);
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
                                        process(message.to_vec(), &agent)
                                            .await
                                            .expect("Should have failed gracefully")
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
