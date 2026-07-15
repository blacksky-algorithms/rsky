use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::Client;
use rsky_crypto::utils::encode_did_key;
use rsky_pds::account_manager::AccountManager;
use rsky_pds::actor_store::ActorStore;
use rsky_pds::config::ServerConfig;
use rsky_pds::space_auth::{mint_space_service_token, NOTIFY_SPACE_DELETED_LXM, NOTIFY_WRITE_LXM};
use rsky_space::car::RepoCarValidator;
use rsky_space::commit::verify_commit;
use serde_json::{json, Value};
use tempfile::TempDir;

mod common;

const AUTHOR_DID: &str = "did:plc:spaceauthoraaaaaaaaaaaaa";
const MEMBER_DID: &str = "did:plc:spacememberbbbbbbbbbbbbb";
const SPACE_TYPE: &str = "com.example.forum";
const COLLECTION: &str = "com.example.post";

async fn create_active_account(client: &Client, did: &str, prefix: &str) -> String {
    let domain = client
        .rocket()
        .state::<ServerConfig>()
        .unwrap()
        .identity
        .service_handle_domains
        .first()
        .unwrap()
        .clone();
    let invite = client
        .post("/xrpc/com.atproto.server.createInviteCode")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", common::get_admin_token()))
        .body(json!({"useCount": 1}).to_string())
        .dispatch()
        .await
        .into_json::<Value>()
        .await
        .unwrap()["code"]
        .as_str()
        .unwrap()
        .to_string();
    let response = client
        .post("/xrpc/com.atproto.server.createAccount")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", common::get_admin_token()))
        .body(
            json!({
                "did": did,
                "email": format!("{prefix}@example.com"),
                "handle": format!("{prefix}{domain}"),
                "password": "password",
                "inviteCode": invite
            })
            .to_string(),
        )
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok, "createAccount for {did}");
    client
        .rocket()
        .state::<AccountManager>()
        .unwrap()
        .activate_account(did)
        .await
        .unwrap();
    let response = client
        .post("/xrpc/com.atproto.server.createSession")
        .header(ContentType::JSON)
        .body(
            json!({
                "identifier": format!("{prefix}{domain}"),
                "password": "password",
            })
            .to_string(),
        )
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok, "createSession for {did}");
    let body: Value = response.into_json().await.unwrap();
    body["accessJwt"].as_str().unwrap().to_string()
}

async fn actor_keypair(client: &Client, did: &str) -> secp256k1::Keypair {
    client
        .rocket()
        .state::<ActorStore>()
        .unwrap()
        .keypair(did)
        .await
        .unwrap()
}

fn bearer(token: &str) -> Header<'static> {
    Header::new("Authorization", format!("Bearer {token}"))
}

async fn post_json(client: &Client, path: &str, token: &str, body: Value) -> (Status, Value) {
    let response = client
        .post(path)
        .header(ContentType::JSON)
        .header(bearer(token))
        .body(body.to_string())
        .dispatch()
        .await;
    let status = response.status();
    let body = response.into_json::<Value>().await.unwrap_or(Value::Null);
    (status, body)
}

async fn get_json(client: &Client, path: &str, token: &str) -> (Status, Value) {
    let response = client.get(path).header(bearer(token)).dispatch().await;
    let status = response.status();
    let body = response.into_json::<Value>().await.unwrap_or(Value::Null);
    (status, body)
}

fn verify_lexicon_commit(commit: &Value, did_key: &str, space_uri: &str, author: &str) {
    use base64::engine::general_purpose::STANDARD_NO_PAD;
    use base64::Engine;
    let bytes = |field: &str| -> Vec<u8> {
        STANDARD_NO_PAD
            .decode(commit[field]["$bytes"].as_str().unwrap())
            .unwrap()
    };
    verify_commit(
        did_key,
        space_uri,
        author,
        commit["rev"].as_str().unwrap(),
        &bytes("ikm"),
        &bytes("sig"),
        &bytes("mac"),
        &bytes("hash"),
    )
    .expect("served commit must verify");
}

struct Setup {
    _dir: TempDir,
    client: Client,
    space: String,
    author_token: String,
    member_token: String,
}

/// Author account owns a member-list space with MEMBER_DID enrolled.
async fn setup() -> Setup {
    let (_dir, client) = common::get_client().await;
    let author_token = create_active_account(&client, AUTHOR_DID, "spcauthor").await;
    let member_token = create_active_account(&client, MEMBER_DID, "spcmember").await;
    let (status, body) = post_json(
        &client,
        "/xrpc/com.atproto.simplespace.createSpace",
        &author_token,
        json!({"spaceType": SPACE_TYPE, "skey": "main"}),
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    let space = body["space"].as_str().unwrap().to_string();
    assert_eq!(space, format!("at://{AUTHOR_DID}/space/{SPACE_TYPE}/main"));
    let (status, _) = post_json(
        &client,
        "/xrpc/com.atproto.simplespace.addMember",
        &author_token,
        json!({"space": space, "did": MEMBER_DID}),
    )
    .await;
    assert_eq!(status, Status::Ok);
    Setup {
        _dir,
        client,
        space,
        author_token,
        member_token,
    }
}

/// getDelegationToken then getSpaceCredential as MEMBER_DID.
async fn mint_credential(setup: &Setup) -> String {
    let (status, body) = get_json(
        &setup.client,
        &format!(
            "/xrpc/com.atproto.space.getDelegationToken?space={}",
            setup.space
        ),
        &setup.member_token,
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    let delegation = body["token"].as_str().unwrap().to_string();
    let (status, body) = post_json(
        &setup.client,
        "/xrpc/com.atproto.space.getSpaceCredential",
        &setup.member_token,
        json!({"space": setup.space, "delegationToken": delegation}),
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    body["credential"].as_str().unwrap().to_string()
}

async fn create_post(setup: &Setup, rkey: &str, text: &str) -> Value {
    let (status, body) = post_json(
        &setup.client,
        "/xrpc/com.atproto.space.createRecord",
        &setup.author_token,
        json!({
            "space": setup.space,
            "collection": COLLECTION,
            "rkey": rkey,
            "record": {"text": text}
        }),
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    body
}

#[tokio::test]
async fn simplespace_management() {
    let s = setup().await;

    // duplicate creation is rejected
    let (status, body) = post_json(
        &s.client,
        "/xrpc/com.atproto.simplespace.createSpace",
        &s.author_token,
        json!({"spaceType": SPACE_TYPE, "skey": "main"}),
    )
    .await;
    assert_eq!(status, Status::BadRequest);
    assert_eq!(body["error"], "SpaceExists");

    // invalid space type / skey are rejected
    for bad in [
        json!({"spaceType": "notannsid"}),
        json!({"spaceType": SPACE_TYPE, "skey": "bad/key"}),
    ] {
        let (status, _) = post_json(
            &s.client,
            "/xrpc/com.atproto.simplespace.createSpace",
            &s.author_token,
            bad,
        )
        .await;
        assert_eq!(status, Status::BadRequest);
    }

    // omitted skey defaults to a TID
    let (status, body) = post_json(
        &s.client,
        "/xrpc/com.atproto.simplespace.createSpace",
        &s.author_token,
        json!({"spaceType": "com.example.other"}),
    )
    .await;
    assert_eq!(status, Status::Ok);
    assert!(body["space"]
        .as_str()
        .unwrap()
        .starts_with(&format!("at://{AUTHOR_DID}/space/com.example.other/")));

    // non-authority cannot manage
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.simplespace.addMember",
        &s.member_token,
        json!({"space": s.space, "did": MEMBER_DID}),
    )
    .await;
    assert_ne!(status, Status::Ok);
    // invalid member did
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.simplespace.addMember",
        &s.author_token,
        json!({"space": s.space, "did": "not-a-did"}),
    )
    .await;
    assert_eq!(status, Status::BadRequest);

    let (status, body) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.simplespace.listMembers?space={}",
            s.space
        ),
        &s.author_token,
    )
    .await;
    assert_eq!(status, Status::Ok);
    assert_eq!(body["members"], json!([{"did": MEMBER_DID}]));

    // removeMember empties the list
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.simplespace.removeMember",
        &s.author_token,
        json!({"space": s.space, "did": MEMBER_DID}),
    )
    .await;
    assert_eq!(status, Status::Ok);
    let (_, body) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.simplespace.listMembers?space={}",
            s.space
        ),
        &s.author_token,
    )
    .await;
    assert_eq!(body["members"], json!([]));

    // managing-app policy requires a managingApp
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.simplespace.updateSpace",
        &s.author_token,
        json!({"space": s.space, "config": {"policy": "managing-app"}}),
    )
    .await;
    assert_eq!(status, Status::BadRequest);
    // a valid update round-trips through getSpace (checked in host tests)
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.simplespace.updateSpace",
        &s.author_token,
        json!({"space": s.space, "config": {"policy": "public"}}),
    )
    .await;
    assert_eq!(status, Status::Ok);
}

#[tokio::test]
async fn credential_mint_flow() {
    let s = setup().await;
    let stranger_token =
        create_active_account(&s.client, "did:plc:spacestrangercccccccccc", "spcstranger").await;

    let (status, body) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.getDelegationToken?space={}",
            s.space
        ),
        &s.member_token,
    )
    .await;
    assert_eq!(status, Status::Ok);
    let delegation = body["token"].as_str().unwrap().to_string();

    // unauthenticated delegation requests are rejected
    let response = s
        .client
        .get(format!(
            "/xrpc/com.atproto.space.getDelegationToken?space={}",
            s.space
        ))
        .dispatch()
        .await;
    assert_ne!(response.status(), Status::Ok);

    // the member-list policy mints for the member
    let (status, body) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.getSpaceCredential",
        &s.member_token,
        json!({"space": s.space, "delegationToken": delegation}),
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    assert!(!body["credential"].as_str().unwrap().is_empty());

    // the delegation token is single-use
    let (status, body) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.getSpaceCredential",
        &s.member_token,
        json!({"space": s.space, "delegationToken": delegation}),
    )
    .await;
    assert_eq!(status, Status::BadRequest);
    assert!(body["message"]
        .as_str()
        .unwrap_or_default()
        .contains("replayed"));

    // a non-member's delegation is refused
    let (_, body) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.getDelegationToken?space={}",
            s.space
        ),
        &stranger_token,
    )
    .await;
    let stranger_delegation = body["token"].as_str().unwrap().to_string();
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.getSpaceCredential",
        &stranger_token,
        json!({"space": s.space, "delegationToken": stranger_delegation}),
    )
    .await;
    assert_ne!(status, Status::Ok);

    // garbage delegation tokens are rejected
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.getSpaceCredential",
        &s.member_token,
        json!({"space": s.space, "delegationToken": "not.a.jwt"}),
    )
    .await;
    assert_ne!(status, Status::Ok);

    // a space this host does not answer for is refused
    let (status, body) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.getSpaceCredential",
        &s.member_token,
        json!({
            "space": "at://did:plc:elsewhere/space/com.example.forum/main",
            "delegationToken": delegation
        }),
    )
    .await;
    assert_eq!(status, Status::BadRequest);
    assert_eq!(body["error"], "SpaceNotFound");

    // allow-list app access requires an attestation
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.simplespace.updateSpace",
        &s.author_token,
        json!({
            "space": s.space,
            "config": {
                "appAccess": {
                    "$type": "com.atproto.simplespace.defs#appAccessAllowList",
                    "allowed": ["https://app.example.com/client-metadata.json"]
                }
            }
        }),
    )
    .await;
    assert_eq!(status, Status::Ok);
    let (_, body) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.getDelegationToken?space={}",
            s.space
        ),
        &s.member_token,
    )
    .await;
    let delegation = body["token"].as_str().unwrap().to_string();
    let (status, body) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.getSpaceCredential",
        &s.member_token,
        json!({"space": s.space, "delegationToken": delegation}),
    )
    .await;
    assert_eq!(status, Status::BadRequest);
    assert_eq!(body["error"], "AttestationRequired");
}

#[tokio::test]
async fn record_write_and_read_flow() {
    let s = setup().await;
    let credential = mint_credential(&s).await;
    let author_key = encode_did_key(&actor_keypair(&s.client, AUTHOR_DID).await.public_key());

    let body = create_post(&s, "3kfirst", "hello space").await;
    assert_eq!(
        body["uri"],
        format!("{}/{AUTHOR_DID}/{COLLECTION}/3kfirst", s.space)
    );
    let first_commit_hash = body["commit"]["hash"].as_str().unwrap().to_string();
    let first_cid = body["cid"].as_str().unwrap().to_string();

    // member reads through the space credential
    let (status, body) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.getRecord?space={}&did={AUTHOR_DID}&collection={COLLECTION}&rkey=3kfirst",
            s.space
        ),
        &credential,
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    assert_eq!(body["value"], json!({"text": "hello space"}));
    assert_eq!(body["cid"], first_cid);

    // wrong-cid param and missing records are not found
    for suffix in ["&cid=bafyreiawrong", ""] {
        let rkey = if suffix.is_empty() {
            "missing"
        } else {
            "3kfirst"
        };
        let (status, _) = get_json(
            &s.client,
            &format!(
                "/xrpc/com.atproto.space.getRecord?space={}&did={AUTHOR_DID}&collection={COLLECTION}&rkey={rkey}{suffix}",
                s.space
            ),
            &credential,
        )
        .await;
        assert_ne!(status, Status::Ok);
    }

    // listRecords without values
    let (status, body) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.listRecords?space={}&did={AUTHOR_DID}&excludeValues=true",
            s.space
        ),
        &credential,
    )
    .await;
    assert_eq!(status, Status::Ok);
    assert_eq!(body["records"].as_array().unwrap().len(), 1);
    assert!(body["records"][0].get("value").is_none());

    // session-side reads: own repo works, another's repo does not
    let (status, _) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.getRecord?space={}&did={AUTHOR_DID}&collection={COLLECTION}&rkey=3kfirst",
            s.space
        ),
        &s.author_token,
    )
    .await;
    assert_eq!(status, Status::Ok);
    for bad_token in [&s.member_token, &"garbage-token".to_string()] {
        let (status, _) = get_json(
            &s.client,
            &format!(
                "/xrpc/com.atproto.space.getRecord?space={}&did={AUTHOR_DID}&collection={COLLECTION}&rkey=3kfirst",
                s.space
            ),
            bad_token,
        )
        .await;
        assert_ne!(status, Status::Ok);
    }

    // deleting a record returns the hash to the prior state
    create_post(&s, "3ksecond", "short lived").await;
    let (status, body) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.deleteRecord",
        &s.author_token,
        json!({"space": s.space, "collection": COLLECTION, "rkey": "3ksecond"}),
    )
    .await;
    assert_eq!(status, Status::Ok);
    assert_eq!(body["commit"]["hash"].as_str().unwrap(), first_commit_hash);
    // deleting a missing record errors
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.deleteRecord",
        &s.author_token,
        json!({"space": s.space, "collection": COLLECTION, "rkey": "3ksecond"}),
    )
    .await;
    assert_ne!(status, Status::Ok);

    // the terminal listRepoOps page carries a verifiable commit
    let (status, body) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.listRepoOps?space={}&did={AUTHOR_DID}",
            s.space
        ),
        &credential,
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    let ops = body["ops"].as_array().unwrap();
    assert_eq!(ops.len(), 3);
    assert_eq!(ops[0]["cid"], first_cid);
    assert_eq!(ops[0]["value"], json!({"text": "hello space"}));
    assert!(ops[0].get("prev").is_none());
    // the deleted record's create is stale: no value inlined
    assert!(ops[1].get("value").is_none());
    assert!(ops[2].get("cid").is_none());
    verify_lexicon_commit(&body["commit"], &author_key, &s.space, AUTHOR_DID);

    // pagination: non-terminal pages omit the commit
    let (status, body) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.listRepoOps?space={}&did={AUTHOR_DID}&limit=2",
            s.space
        ),
        &credential,
    )
    .await;
    assert_eq!(status, Status::Ok);
    assert!(body.get("commit").is_none());
    let cursor = body["cursor"].as_str().unwrap().to_string();
    let (status, body) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.listRepoOps?space={}&did={AUTHOR_DID}&cursor={cursor}",
            s.space
        ),
        &credential,
    )
    .await;
    assert_eq!(status, Status::Ok);
    verify_lexicon_commit(&body["commit"], &author_key, &s.space, AUTHOR_DID);
    // a malformed cursor is rejected
    let (status, _) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.listRepoOps?space={}&did={AUTHOR_DID}&cursor=nope",
            s.space
        ),
        &credential,
    )
    .await;
    assert_eq!(status, Status::BadRequest);

    // getLatestCommit verifies
    let (status, body) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.getLatestCommit?space={}&did={AUTHOR_DID}",
            s.space
        ),
        &credential,
    )
    .await;
    assert_eq!(status, Status::Ok);
    verify_lexicon_commit(&body["commit"], &author_key, &s.space, AUTHOR_DID);

    // listSpaces shows the author's repo
    let (status, body) = get_json(
        &s.client,
        "/xrpc/com.atproto.space.listSpaces",
        &s.author_token,
    )
    .await;
    assert_eq!(status, Status::Ok);
    assert_eq!(body["spaces"], json!([s.space]));
}

#[tokio::test]
async fn car_export_validates() {
    let s = setup().await;
    let credential = mint_credential(&s).await;
    let author_key = encode_did_key(&actor_keypair(&s.client, AUTHOR_DID).await.public_key());
    create_post(&s, "3kfirst", "hello space").await;
    create_post(&s, "3ksecond", "more").await;

    let response = s
        .client
        .get(format!(
            "/xrpc/com.atproto.space.getRepo?space={}&did={AUTHOR_DID}",
            s.space
        ))
        .header(bearer(&credential))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    assert_eq!(
        response.headers().get_one("content-type"),
        Some("application/vnd.ipld.car")
    );
    let car = response.into_bytes().await.unwrap();
    let validator = RepoCarValidator::new(car.as_slice()).await.unwrap();
    let commit = validator.commit().clone();
    verify_commit(
        &author_key,
        &s.space,
        AUTHOR_DID,
        &commit.rev,
        &commit.ikm,
        &commit.sig,
        &commit.mac,
        &commit.hash,
    )
    .unwrap();
    let records = validator.into_records(&commit.hash).await.unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].0, format!("{COLLECTION}/3kfirst"));

    // a repo that does not exist in the space 404s
    let (status, _) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.getRepo?space={}&did={MEMBER_DID}",
            s.space
        ),
        &credential,
    )
    .await;
    assert_ne!(status, Status::Ok);
}

#[tokio::test]
async fn put_and_apply_writes() {
    let s = setup().await;

    // putRecord: create then update with swap
    let (status, body) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.putRecord",
        &s.author_token,
        json!({
            "space": s.space,
            "collection": COLLECTION,
            "rkey": "3kput",
            "record": {"text": "v1"}
        }),
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    let v1_cid = body["cid"].as_str().unwrap().to_string();
    let (status, body) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.putRecord",
        &s.author_token,
        json!({
            "space": s.space,
            "collection": COLLECTION,
            "rkey": "3kput",
            "record": {"text": "v2"},
            "swapRecord": v1_cid
        }),
    )
    .await;
    assert_eq!(status, Status::Ok);
    assert_ne!(body["cid"].as_str().unwrap(), v1_cid);
    // a stale swap fails
    let (status, body) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.putRecord",
        &s.author_token,
        json!({
            "space": s.space,
            "collection": COLLECTION,
            "rkey": "3kput",
            "record": {"text": "v3"},
            "swapRecord": v1_cid
        }),
    )
    .await;
    assert_eq!(status, Status::BadRequest);
    assert_eq!(body["error"], "InvalidSwap");

    // invalid collection / rkey
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.putRecord",
        &s.author_token,
        json!({"space": s.space, "collection": "nodots", "rkey": "x", "record": {}}),
    )
    .await;
    assert_eq!(status, Status::BadRequest);
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.createRecord",
        &s.author_token,
        json!({"space": s.space, "collection": COLLECTION, "rkey": "bad/rkey", "record": {}}),
    )
    .await;
    assert_eq!(status, Status::BadRequest);

    // applyWrites: batch under a single rev
    let (status, body) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.applyWrites",
        &s.author_token,
        json!({
            "space": s.space,
            "writes": [
                {"$type": "com.atproto.space.applyWrites#create",
                 "collection": COLLECTION, "rkey": "3kbatch1", "value": {"text": "b1"}},
                {"$type": "com.atproto.space.applyWrites#update",
                 "collection": COLLECTION, "rkey": "3kput", "value": {"text": "v3"}},
                {"$type": "com.atproto.space.applyWrites#delete",
                 "collection": COLLECTION, "rkey": "3kbatch1"}
            ]
        }),
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    let results = body["results"].as_array().unwrap();
    assert_eq!(results.len(), 3);
    assert_eq!(
        results[0]["$type"],
        "com.atproto.space.applyWrites#createResult"
    );
    assert_eq!(
        results[1]["$type"],
        "com.atproto.space.applyWrites#updateResult"
    );
    assert_eq!(
        results[2]["$type"],
        "com.atproto.space.applyWrites#deleteResult"
    );
    // creates without an rkey get a TID
    let (status, body) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.applyWrites",
        &s.author_token,
        json!({
            "space": s.space,
            "writes": [
                {"$type": "com.atproto.space.applyWrites#create",
                 "collection": COLLECTION, "value": {"text": "auto"}}
            ]
        }),
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
}

#[tokio::test]
async fn host_methods_and_notifications() {
    let s = setup().await;
    let credential = mint_credential(&s).await;
    create_post(&s, "3kfirst", "hello space").await;

    // getSpace surfaces the simplespace config
    let (status, body) = get_json(
        &s.client,
        &format!("/xrpc/com.atproto.space.getSpace?space={}", s.space),
        &credential,
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    assert_eq!(body["config"]["policy"], "member-list");
    assert_eq!(
        body["config"]["appAccess"]["$type"],
        "com.atproto.simplespace.defs#appAccessOpen"
    );
    // a session token is not a space credential
    let (status, _) = get_json(
        &s.client,
        &format!("/xrpc/com.atproto.space.getSpace?space={}", s.space),
        &s.member_token,
    )
    .await;
    assert_ne!(status, Status::Ok);
    // a credential for one space does not open another
    let (status, _) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.getSpace?space=at://{AUTHOR_DID}/space/{SPACE_TYPE}/other"
        ),
        &credential,
    )
    .await;
    assert_ne!(status, Status::Ok);

    // the writer set was maintained by the author's write
    let (status, body) = get_json(
        &s.client,
        &format!("/xrpc/com.atproto.space.listRepos?space={}", s.space),
        &credential,
    )
    .await;
    assert_eq!(status, Status::Ok);
    let repos = body["repos"].as_array().unwrap();
    assert_eq!(repos.len(), 1);
    assert_eq!(repos[0]["did"], AUTHOR_DID);
    assert!(!repos[0]["rev"].as_str().unwrap().is_empty());

    // registerNotify: repo-level and space-level
    let (status, body) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.registerNotify",
        &credential,
        json!({"space": s.space, "endpoint": "https://sync.example.invalid", "repo": AUTHOR_DID}),
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    assert!(body["expiresAt"].as_str().is_some());
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.registerNotify",
        &credential,
        json!({"space": s.space, "endpoint": "https://sync.example.invalid"}),
    )
    .await;
    assert_eq!(status, Status::Ok);
    // invalid endpoint
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.registerNotify",
        &credential,
        json!({"space": s.space, "endpoint": "ftp://nope"}),
    )
    .await;
    assert_eq!(status, Status::BadRequest);

    // inbound notifyWrite from a member's repo host
    let member_keypair = actor_keypair(&s.client, MEMBER_DID).await;
    let service_token = mint_space_service_token(
        &member_keypair,
        MEMBER_DID,
        &format!("{AUTHOR_DID}#atproto_space_host"),
        NOTIFY_WRITE_LXM,
    )
    .unwrap();
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.notifyWrite",
        &service_token,
        json!({"space": s.space, "did": MEMBER_DID, "rev": "3kmemberrev"}),
    )
    .await;
    assert_eq!(status, Status::Ok);
    let (_, body) = get_json(
        &s.client,
        &format!("/xrpc/com.atproto.space.listRepos?space={}", s.space),
        &credential,
    )
    .await;
    assert_eq!(body["repos"].as_array().unwrap().len(), 2);

    // an iss/did mismatch is rejected
    let bad_token =
        mint_space_service_token(&member_keypair, MEMBER_DID, AUTHOR_DID, NOTIFY_WRITE_LXM)
            .unwrap();
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.notifyWrite",
        &bad_token,
        json!({"space": s.space, "did": AUTHOR_DID, "rev": "3kforged"}),
    )
    .await;
    assert_ne!(status, Status::Ok);
    // missing auth is rejected
    let response = s
        .client
        .post("/xrpc/com.atproto.space.notifyWrite")
        .header(ContentType::JSON)
        .body(json!({"space": s.space, "did": MEMBER_DID, "rev": "3k"}).to_string())
        .dispatch()
        .await;
    assert_ne!(response.status(), Status::Ok);

    // drain the notification queue so best-effort deliveries are attempted
    s.client
        .rocket()
        .state::<ActorStore>()
        .unwrap()
        .background_queue
        .process_all()
        .await;
}

#[tokio::test]
async fn delete_space_flow() {
    let s = setup().await;
    let credential = mint_credential(&s).await;
    create_post(&s, "3kfirst", "hello space").await;
    // register a syncer so deletion has someone to notify
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.registerNotify",
        &credential,
        json!({"space": s.space, "endpoint": "https://sync.example.invalid"}),
    )
    .await;
    assert_eq!(status, Status::Ok);

    // only the authority can delete
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.simplespace.deleteSpace",
        &s.member_token,
        json!({"space": s.space}),
    )
    .await;
    assert_ne!(status, Status::Ok);

    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.simplespace.deleteSpace",
        &s.author_token,
        json!({"space": s.space}),
    )
    .await;
    assert_eq!(status, Status::Ok);

    // the space no longer answers, and no new credentials are minted
    let (status, body) = get_json(
        &s.client,
        &format!("/xrpc/com.atproto.space.getSpace?space={}", s.space),
        &credential,
    )
    .await;
    assert_eq!(status, Status::BadRequest, "{body}");
    assert_eq!(body["error"], "SpaceDeleted");
    let (_, body) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.getDelegationToken?space={}",
            s.space
        ),
        &s.member_token,
    )
    .await;
    let delegation = body["token"].as_str().unwrap().to_string();
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.getSpaceCredential",
        &s.member_token,
        json!({"space": s.space, "delegationToken": delegation}),
    )
    .await;
    assert_ne!(status, Status::Ok);

    // the author's repo is flagged, not erased: reads fail with SpaceDeleted
    let (status, body) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.getRecord?space={}&did={AUTHOR_DID}&collection={COLLECTION}&rkey=3kfirst",
            s.space
        ),
        &credential,
    )
    .await;
    assert_eq!(status, Status::BadRequest);
    assert_eq!(body["error"], "SpaceDeleted");
    // writes are refused
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.createRecord",
        &s.author_token,
        json!({
            "space": s.space,
            "collection": COLLECTION,
            "rkey": "3kafter",
            "record": {"text": "no"}
        }),
    )
    .await;
    assert_ne!(status, Status::Ok);
    // listSpaces no longer includes it
    let (_, body) = get_json(
        &s.client,
        "/xrpc/com.atproto.space.listSpaces",
        &s.author_token,
    )
    .await;
    assert_eq!(body["spaces"], json!([]));
    // deleting twice fails cleanly
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.simplespace.deleteSpace",
        &s.author_token,
        json!({"space": s.space}),
    )
    .await;
    assert_ne!(status, Status::Ok);
    // drain the queue: the syncer notification attempt runs (and is logged)
    s.client
        .rocket()
        .state::<ActorStore>()
        .unwrap()
        .background_queue
        .process_all()
        .await;
}

#[tokio::test]
async fn inbound_notify_space_deleted_flags_local_repos() {
    let (_dir, client) = common::get_client().await;
    let author_token = create_active_account(&client, AUTHOR_DID, "spcauthor2").await;

    // A repo in a space anchored on a REMOTE authority: the local account
    // simply writes into it.
    let remote_space = "at://did:plc:remoteauthority/space/com.example.forum/main";
    let (status, _) = post_json(
        &client,
        "/xrpc/com.atproto.space.createRecord",
        &author_token,
        json!({
            "space": remote_space,
            "collection": COLLECTION,
            "record": {"text": "in a remote space"}
        }),
    )
    .await;
    assert_eq!(status, Status::Ok);
    client
        .rocket()
        .state::<ActorStore>()
        .unwrap()
        .background_queue
        .process_all()
        .await;

    // A personal space anchored on the local account exercises the
    // service-auth verification path hermetically.
    let local_space = format!("at://{AUTHOR_DID}/space/{SPACE_TYPE}/self");
    let (status, _) = post_json(
        &client,
        "/xrpc/com.atproto.space.createRecord",
        &author_token,
        json!({
            "space": local_space,
            "collection": COLLECTION,
            "record": {"text": "personal"}
        }),
    )
    .await;
    assert_eq!(status, Status::Ok);

    let keypair = client
        .rocket()
        .state::<ActorStore>()
        .unwrap()
        .keypair(AUTHOR_DID)
        .await
        .unwrap();
    let token = mint_space_service_token(
        &keypair,
        AUTHOR_DID,
        "did:web:somesyncer.example",
        NOTIFY_SPACE_DELETED_LXM,
    )
    .unwrap();
    let (status, _) = post_json(
        &client,
        "/xrpc/com.atproto.space.notifySpaceDeleted",
        &token,
        json!({"space": local_space}),
    )
    .await;
    assert_eq!(status, Status::Ok);
    // the repo is flagged
    let (status, body) = get_json(
        &client,
        &format!("/xrpc/com.atproto.space.listRepoOps?space={local_space}&did={AUTHOR_DID}"),
        &author_token,
    )
    .await;
    assert_eq!(status, Status::BadRequest);
    assert_eq!(body["error"], "SpaceDeleted");

    // a non-authority issuer cannot delete someone else's space
    let token = mint_space_service_token(
        &keypair,
        AUTHOR_DID,
        "did:web:somesyncer.example",
        NOTIFY_SPACE_DELETED_LXM,
    )
    .unwrap();
    let (status, _) = post_json(
        &client,
        "/xrpc/com.atproto.space.notifySpaceDeleted",
        &token,
        json!({"space": remote_space}),
    )
    .await;
    assert_ne!(status, Status::Ok);
    // the remote-space repo is untouched
    let (status, _) = get_json(
        &client,
        &format!("/xrpc/com.atproto.space.listRepoOps?space={remote_space}&did={AUTHOR_DID}"),
        &author_token,
    )
    .await;
    assert_eq!(status, Status::Ok);

    // wrong lxm is rejected
    let token = mint_space_service_token(
        &keypair,
        AUTHOR_DID,
        "did:web:somesyncer.example",
        NOTIFY_WRITE_LXM,
    )
    .unwrap();
    let (status, _) = post_json(
        &client,
        "/xrpc/com.atproto.space.notifySpaceDeleted",
        &token,
        json!({"space": local_space}),
    )
    .await;
    assert_ne!(status, Status::Ok);
    // missing auth is rejected
    let response = client
        .post("/xrpc/com.atproto.space.notifySpaceDeleted")
        .header(ContentType::JSON)
        .body(json!({"space": local_space}).to_string())
        .dispatch()
        .await;
    assert_ne!(response.status(), Status::Ok);
}

#[tokio::test]
async fn blob_upload_and_space_get_blob() {
    let s = setup().await;
    let credential = mint_credential(&s).await;

    // upload a blob as the author, then reference it from a space record
    let response = s
        .client
        .post("/xrpc/com.atproto.repo.uploadBlob")
        .header(ContentType::PNG)
        .header(bearer(&s.author_token))
        .body(vec![0x89u8, 0x50, 0x4E, 0x47, 1, 2, 3, 4])
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let body: Value = response.into_json().await.unwrap();
    let blob = body["blob"].clone();
    let blob_cid = blob["ref"]["$link"].as_str().unwrap().to_string();

    // a record referencing a blob that was never uploaded is rejected
    let (status, body) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.createRecord",
        &s.author_token,
        json!({
            "space": s.space,
            "collection": COLLECTION,
            "rkey": "3kmissingblob",
            "record": {
                "text": "bad blob",
                "embed": {"$type": "blob",
                          "ref": {"$link": "bafkreihdwdcefgh4dqkjv67uzcmw7ojee6xedzdetojuzjevtenxquvyku"},
                          "mimeType": "image/png", "size": 8}
            }
        }),
    )
    .await;
    assert_eq!(status, Status::BadRequest, "{body}");

    let (status, body) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.createRecord",
        &s.author_token,
        json!({
            "space": s.space,
            "collection": COLLECTION,
            "rkey": "3kwithblob",
            "record": {
                "text": "with blob",
                "embed": blob,
                "legacy": {"cid": "not-a-cid", "mimeType": "image/png"}
            }
        }),
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");

    // the member fetches the blob through the space credential
    let response = s
        .client
        .get(format!(
            "/xrpc/com.atproto.space.getBlob?space={}&did={AUTHOR_DID}&cid={blob_cid}",
            s.space
        ))
        .header(bearer(&credential))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    assert_eq!(
        response.into_bytes().await.unwrap(),
        vec![0x89u8, 0x50, 0x4E, 0x47, 1, 2, 3, 4]
    );

    // an unreferenced blob is not served through the space
    let (status, _) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.getBlob?space={}&did={AUTHOR_DID}&cid=bafkreihdwdcefgh4dqkjv67uzcmw7ojee6xedzdetojuzjevtenxquvyku",
            s.space
        ),
        &credential,
    )
    .await;
    assert_ne!(status, Status::Ok);
    // a referenced-but-unparseable cid is a bad request
    let (status, _) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.getBlob?space={}&did={AUTHOR_DID}&cid=not-a-cid",
            s.space
        ),
        &credential,
    )
    .await;
    assert_eq!(status, Status::BadRequest);
}

fn craft_credential_jwt(iss: &str, sub: &str, keypair: Option<&secp256k1::Keypair>) -> String {
    use rsky_space::credential::{encode, JwtHeader, SpaceClaims, CREDENTIAL_TYP};
    let header = JwtHeader {
        typ: CREDENTIAL_TYP.to_string(),
        alg: "ES256K".to_string(),
        kid: Some("#atproto_space".to_string()),
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let claims = SpaceClaims {
        iss: iss.to_string(),
        sub: sub.to_string(),
        aud: None,
        iat: now,
        exp: now + 3600,
        jti: "crafted".to_string(),
    };
    encode(&header, &claims, |input| match keypair {
        Some(keypair) => rsky_pds::space_auth::sign_with_keypair(keypair, input),
        None => Ok(vec![0u8; 64]),
    })
    .unwrap()
}

#[tokio::test]
async fn crafted_credentials_are_rejected() {
    let s = setup().await;
    // a key that belongs to no account on this host
    let secp = secp256k1::Secp256k1::new();
    let foreign_keypair = secp256k1::Keypair::from_secret_key(
        &secp,
        &secp256k1::SecretKey::from_slice(&[0x42u8; 32]).unwrap(),
    );

    // sub is not a space uri
    let bad_sub = craft_credential_jwt(AUTHOR_DID, "at://not-a-space", None);
    // issuer is not the space authority
    let wrong_iss = craft_credential_jwt(MEMBER_DID, &s.space, Some(&foreign_keypair));
    // right claims, wrong signing key
    let wrong_key = craft_credential_jwt(AUTHOR_DID, &s.space, Some(&foreign_keypair));
    for token in [bad_sub, wrong_iss, wrong_key] {
        let (status, _) = get_json(
            &s.client,
            &format!("/xrpc/com.atproto.space.getSpace?space={}", s.space),
            &token,
        )
        .await;
        assert_ne!(status, Status::Ok);
        // the read guard rejects them identically
        let (status, _) = get_json(
            &s.client,
            &format!(
                "/xrpc/com.atproto.space.listRecords?space={}&did={AUTHOR_DID}",
                s.space
            ),
            &token,
        )
        .await;
        assert_ne!(status, Status::Ok);
    }
}

#[tokio::test]
async fn credential_edge_cases() {
    let s = setup().await;

    // duplicate space.createRecord rkey surfaces RecordExists
    create_post(&s, "3kdup", "one").await;
    let (status, body) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.createRecord",
        &s.author_token,
        json!({"space": s.space, "collection": COLLECTION, "rkey": "3kdup", "record": {"text": "two"}}),
    )
    .await;
    assert_eq!(status, Status::BadRequest);
    assert_eq!(body["error"], "RecordExists");

    // getSpaceCredential for a nonexistent space under a local authority
    let (_, body) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.getDelegationToken?space={}",
            s.space
        ),
        &s.member_token,
    )
    .await;
    let delegation = body["token"].as_str().unwrap().to_string();
    let (status, body) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.getSpaceCredential",
        &s.member_token,
        json!({
            "space": format!("at://{AUTHOR_DID}/space/{SPACE_TYPE}/nonexistent"),
            "delegationToken": delegation
        }),
    )
    .await;
    assert_eq!(status, Status::BadRequest);
    assert_eq!(body["error"], "SpaceNotFound");

    // a tampered delegation signature is refused
    let (_, body) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.getDelegationToken?space={}",
            s.space
        ),
        &s.member_token,
    )
    .await;
    let delegation = body["token"].as_str().unwrap().to_string();
    let mut parts: Vec<String> = delegation.split('.').map(str::to_string).collect();
    {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        let mut sig = URL_SAFE_NO_PAD.decode(&parts[2]).unwrap();
        sig[0] ^= 0xFF;
        parts[2] = URL_SAFE_NO_PAD.encode(sig);
    }
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.getSpaceCredential",
        &s.member_token,
        json!({"space": s.space, "delegationToken": parts.join(".")}),
    )
    .await;
    assert_ne!(status, Status::Ok);

    // a delegation whose issuer's key cannot be resolved is refused (the mock
    // PLC serves DID documents without verification methods)
    let member_keypair = actor_keypair(&s.client, MEMBER_DID).await;
    let foreign = {
        use rsky_space::credential::{encode, JwtHeader, SpaceClaims, DELEGATION_TYP};
        let header = JwtHeader {
            typ: DELEGATION_TYP.to_string(),
            alg: "ES256K".to_string(),
            kid: Some("#atproto".to_string()),
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let claims = SpaceClaims {
            iss: "did:plc:notonthishost".to_string(),
            sub: s.space.clone(),
            aud: Some(format!("{AUTHOR_DID}#atproto_space_host")),
            iat: now,
            exp: now + 60,
            jti: "foreign".to_string(),
        };
        encode(&header, &claims, |input| {
            rsky_pds::space_auth::sign_with_keypair(&member_keypair, input)
        })
        .unwrap()
    };
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.getSpaceCredential",
        &s.member_token,
        json!({"space": s.space, "delegationToken": foreign}),
    )
    .await;
    assert_ne!(status, Status::Ok);

    // managing-app policy: the app's service endpoint is unresolvable here
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.simplespace.updateSpace",
        &s.author_token,
        json!({
            "space": s.space,
            "config": {"policy": "managing-app", "managingApp": "did:plc:someapp#managing_app"}
        }),
    )
    .await;
    assert_eq!(status, Status::Ok);
    // an invalid managingApp identifier is rejected
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.simplespace.updateSpace",
        &s.author_token,
        json!({"space": s.space, "config": {"managingApp": "not-a-service-id"}}),
    )
    .await;
    assert_eq!(status, Status::BadRequest);
    let (_, body) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.getDelegationToken?space={}",
            s.space
        ),
        &s.member_token,
    )
    .await;
    let delegation = body["token"].as_str().unwrap().to_string();
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.getSpaceCredential",
        &s.member_token,
        json!({"space": s.space, "delegationToken": delegation}),
    )
    .await;
    assert_ne!(status, Status::Ok);
}

#[tokio::test]
async fn notify_edges_and_allowlist_view() {
    let s = setup().await;
    let credential = mint_credential(&s).await;
    create_post(&s, "3kfirst", "hello").await;

    // registerNotify for a repo this host does not have
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.registerNotify",
        &credential,
        json!({"space": s.space, "endpoint": "https://sync.example.invalid", "repo": "did:plc:nothosted"}),
    )
    .await;
    assert_ne!(status, Status::Ok);

    // notifyWrite with a garbage bearer token
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.notifyWrite",
        "garbage",
        json!({"space": s.space, "did": MEMBER_DID, "rev": "3k"}),
    )
    .await;
    assert_ne!(status, Status::Ok);
    // notifySpaceDeleted signed by a key the resolver cannot vouch for
    // (unknown issuer resolves to a doc without verification methods)
    let member_keypair = actor_keypair(&s.client, MEMBER_DID).await;
    let unknown_iss = mint_space_service_token(
        &member_keypair,
        "did:plc:unknownissuer",
        AUTHOR_DID,
        NOTIFY_SPACE_DELETED_LXM,
    )
    .unwrap();
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.notifySpaceDeleted",
        &unknown_iss,
        json!({"space": s.space}),
    )
    .await;
    assert_ne!(status, Status::Ok);

    // an allow-list config is surfaced through getSpace
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.simplespace.updateSpace",
        &s.author_token,
        json!({
            "space": s.space,
            "config": {
                "appAccess": {
                    "$type": "com.atproto.simplespace.defs#appAccessAllowList",
                    "allowed": ["https://app.example.com/client-metadata.json"]
                }
            }
        }),
    )
    .await;
    assert_eq!(status, Status::Ok);
    let (status, body) = get_json(
        &s.client,
        &format!("/xrpc/com.atproto.space.getSpace?space={}", s.space),
        &credential,
    )
    .await;
    assert_eq!(status, Status::Ok);
    assert_eq!(
        body["config"]["appAccess"]["$type"],
        "com.atproto.simplespace.defs#appAccessAllowList"
    );
    // and switching back to open clears the list
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.simplespace.updateSpace",
        &s.author_token,
        json!({
            "space": s.space,
            "config": {"appAccess": {"$type": "com.atproto.simplespace.defs#appAccessOpen"}}
        }),
    )
    .await;
    assert_eq!(status, Status::Ok);
    let (_, body) = get_json(
        &s.client,
        &format!("/xrpc/com.atproto.space.getSpace?space={}", s.space),
        &credential,
    )
    .await;
    assert_eq!(
        body["config"]["appAccess"]["$type"],
        "com.atproto.simplespace.defs#appAccessOpen"
    );
}

#[tokio::test]
async fn delete_space_notifies_remote_writers() {
    let s = setup().await;
    create_post(&s, "3kfirst", "hello").await;

    // a remote writer is known to the authority (e.g. via inbound notifyWrite)
    let actor_store = s.client.rocket().state::<ActorStore>().unwrap();
    let reader = actor_store
        .read(
            AUTHOR_DID.to_string(),
            s.client
                .rocket()
                .state::<rsky_pds::actor_store::blobstore::BlobstoreFactory>()
                .unwrap()
                .blobstore(AUTHOR_DID.to_string()),
        )
        .await
        .unwrap();
    reader
        .space
        .upsert_writer(&s.space, "did:plc:remotewriterxyz", "3krev", None)
        .await
        .unwrap();

    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.simplespace.deleteSpace",
        &s.author_token,
        json!({"space": s.space}),
    )
    .await;
    assert_eq!(status, Status::Ok);
    // the background fan-out resolves the remote writer's host through the
    // mock PLC (which advertises a PDS endpoint) and delivers best-effort
    actor_store.background_queue.process_all().await;
}

#[tokio::test]
async fn read_guard_scope_boundaries() {
    let s = setup().await;
    let credential = mint_credential(&s).await;
    create_post(&s, "3ka", "one").await;
    create_post(&s, "3kb", "two").await;

    // a credential for one space does not read a repo in another space
    let other_space = format!("at://{AUTHOR_DID}/space/{SPACE_TYPE}/other");
    let (status, _) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.getRecord?space={other_space}&did={AUTHOR_DID}&collection={COLLECTION}&rkey=3ka"
        ),
        &credential,
    )
    .await;
    assert_ne!(status, Status::Ok);

    // a delegation token is not accepted where a credential is required
    let (_, body) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.getDelegationToken?space={}",
            s.space
        ),
        &s.member_token,
    )
    .await;
    let delegation = body["token"].as_str().unwrap().to_string();
    let (status, _) = get_json(
        &s.client,
        &format!("/xrpc/com.atproto.space.getSpace?space={}", s.space),
        &delegation,
    )
    .await;
    assert_ne!(status, Status::Ok);

    // listRecords paginates with a cursor
    let (status, body) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.listRecords?space={}&did={AUTHOR_DID}&limit=1",
            s.space
        ),
        &credential,
    )
    .await;
    assert_eq!(status, Status::Ok);
    assert_eq!(body["records"].as_array().unwrap().len(), 1);
    let cursor = body["cursor"].as_str().unwrap().to_string();
    let (status, body) = get_json(
        &s.client,
        &format!(
            "/xrpc/com.atproto.space.listRecords?space={}&did={AUTHOR_DID}&limit=1&cursor={cursor}",
            s.space
        ),
        &credential,
    )
    .await;
    assert_eq!(status, Status::Ok);
    assert!(body["records"][0]["uri"]
        .as_str()
        .unwrap()
        .ends_with("/3kb"));

    // a write after notify registrations fans out deliveries (one endpoint
    // resolvable through the mock, one unresolvable)
    let (status, _) = post_json(
        &s.client,
        "/xrpc/com.atproto.space.registerNotify",
        &credential,
        json!({"space": s.space, "endpoint": "https://sync.example.invalid", "repo": AUTHOR_DID}),
    )
    .await;
    assert_eq!(status, Status::Ok);
    create_post(&s, "3kc", "three").await;
    s.client
        .rocket()
        .state::<ActorStore>()
        .unwrap()
        .background_queue
        .process_all()
        .await;
}
