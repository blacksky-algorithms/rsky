use rocket::http::Status;
use serde_json::Value;
use std::collections::BTreeSet;

mod common;

/// Every `com.atproto.space.*` method a conformant client checks for before it
/// will talk to a PDS at all.
const REQUIRED_SPACE_METHODS: [&str; 19] = [
    "com.atproto.space.applyWrites",
    "com.atproto.space.createRecord",
    "com.atproto.space.deleteRecord",
    "com.atproto.space.getBlob",
    "com.atproto.space.getDelegationToken",
    "com.atproto.space.getLatestCommit",
    "com.atproto.space.getRecord",
    "com.atproto.space.getRepo",
    "com.atproto.space.getRepoState",
    "com.atproto.space.getSpace",
    "com.atproto.space.getSpaceCredential",
    "com.atproto.space.listRecords",
    "com.atproto.space.listRepoOps",
    "com.atproto.space.listRepos",
    "com.atproto.space.listSpaces",
    "com.atproto.space.notifySpaceDeleted",
    "com.atproto.space.notifyWrite",
    "com.atproto.space.putRecord",
    "com.atproto.space.registerNotify",
];

async fn describe() -> (Status, Value) {
    let (_dir, client) = common::get_client().await;
    let response = client
        .get("/xrpc/community.lexicon.service.describe")
        .dispatch()
        .await;
    let status = response.status();
    (status, response.into_json::<Value>().await.unwrap())
}

fn described_methods(body: &Value) -> BTreeSet<String> {
    body["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            assert_eq!(entry["$type"], "community.lexicon.service.describe#nsid");
            entry["value"].as_str().unwrap().to_string()
        })
        .collect()
}

/// The endpoint answers without credentials: a caller deciding whether it can
/// talk to this server has nothing to authenticate with yet.
#[tokio::test]
async fn describe_is_anonymous_and_names_this_server_a_pds() {
    let (status, body) = describe().await;
    assert_eq!(status, Status::Ok);
    assert_eq!(body["roles"], serde_json::json!(["pds"]));
}

#[tokio::test]
async fn describe_advertises_the_whole_space_surface() {
    let (_, body) = describe().await;
    let described = described_methods(&body);
    let missing: Vec<_> = REQUIRED_SPACE_METHODS
        .iter()
        .filter(|method| !described.contains(**method))
        .collect();
    assert!(
        missing.is_empty(),
        "space methods not advertised: {missing:?}"
    );
}

/// The whole value of this endpoint is that its answer is true, and a
/// hand-kept list drifts the first time a route is added without touching it.
#[tokio::test]
async fn described_methods_match_the_mounted_routes() {
    let (_dir, client) = common::get_client().await;
    let response = client
        .get("/xrpc/community.lexicon.service.describe")
        .dispatch()
        .await;
    let body = response.into_json::<Value>().await.unwrap();
    let described = described_methods(&body);

    let mounted: BTreeSet<String> = client
        .rocket()
        .routes()
        .filter_map(|route| {
            let path = route.uri.path().to_string();
            let nsid = path.strip_prefix("/xrpc/")?.to_string();
            // The proxy forwarders match a path parameter rather than naming a
            // method this server implements.
            (nsid.contains('.') && !nsid.contains('<')).then_some(nsid)
        })
        .collect();

    let undeclared: Vec<_> = mounted.difference(&described).collect();
    assert!(
        undeclared.is_empty(),
        "routed but not described, so the description understates the server: {undeclared:?}"
    );
    let unrouted: Vec<_> = described.difference(&mounted).collect();
    assert!(
        unrouted.is_empty(),
        "described but not routed, so the description claims what is not served: {unrouted:?}"
    );
}
