use crate::account_manager::helpers::account::register_actor;
use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::MemoryBlobStore;
use crate::actor_store::ActorStore;
use crate::apis::app::bsky::actor::get_profile::get_profile_munge;
use crate::apis::app::bsky::actor::get_profiles::get_profiles_munge;
use crate::apis::app::bsky::feed::get_actor_likes::get_author_munge as get_actor_likes_munge;
use crate::apis::app::bsky::feed::get_author_feed::get_author_munge as get_author_feed_munge;
use crate::apis::app::bsky::feed::get_post_thread::{get_post_thread_munge, thread_post_view};
use crate::apis::app::bsky::feed::get_timeline::get_timeline_munge;
use crate::background::BackgroundQueue;
use crate::config::ActorStoreConfig;
use crate::read_after_write::feed_json;
use crate::read_after_write::types::{LocalRecords, RecordDescript};
use crate::read_after_write::viewer::LocalViewer;
use chrono::{DateTime, Utc};
use lexicon_cid::Cid;
use rsky_lexicon::app::bsky::actor::{
    GetProfilesOutput, Profile, ProfileViewBasic, ProfileViewDetailed,
};
use rsky_lexicon::app::bsky::embed::external::{External, ExternalObject};
use rsky_lexicon::app::bsky::embed::gallery::{Gallery, GalleryItem, Image as GalleryImage};
use rsky_lexicon::app::bsky::embed::images::{AspectRatio, Image, Images};
use rsky_lexicon::app::bsky::embed::record::Record as RecordEmbed;
use rsky_lexicon::app::bsky::embed::record_with_media::RecordWithMedia;
use rsky_lexicon::app::bsky::embed::video::Video;
use rsky_lexicon::app::bsky::embed::{EmbedViews, Embeds, MediaUnion};
use rsky_lexicon::app::bsky::feed::{
    FeedViewPost, Post, PostView, ReplyRef, ThreadViewPost, ThreadViewPostEnum,
};
use rsky_lexicon::com::atproto::label::Label;
use rsky_lexicon::com::atproto::repo::{Blob, StrongRef};
use rsky_syntax::aturi::AtUri;
use serde_json::{json, Value};
use std::str::FromStr;
use std::sync::Arc;

const TEST_DID: &str = "did:example:alice";
const OTHER_DID: &str = "did:example:bob";
const TEST_HANDLE: &str = "alice.test";
const TEST_HOSTNAME: &str = "pds.example.com";
const TEST_CID: &str = "bafkreibjfgx2gprinfvicegelk5kosd6y2frmqpqzwqkg7usac74l3t2v4";
const TEST_SECRET_HEX: &str = "1d2f8064213bd212453fa93943c084dbbf42104d02f1f02b23a638f9a48f925a";

async fn test_viewer() -> (tempfile::TempDir, LocalViewer) {
    let dir = tempfile::tempdir().unwrap();
    let cfg = ActorStoreConfig {
        directory: dir.path().join("actors").to_string_lossy().to_string(),
        cache_size: 4,
    };
    let lifecycle =
        crate::lifecycle::LifecycleStore::open(dir.path().join("rsky/lifecycle.sqlite"))
            .await
            .unwrap();
    let store = ActorStore::new(&cfg, BackgroundQueue::default(), lifecycle);
    let secp = secp256k1::Secp256k1::new();
    let secret = secp256k1::SecretKey::from_slice(&hex::decode(TEST_SECRET_HEX).unwrap()).unwrap();
    let keypair = secp256k1::Keypair::from_secret_key(&secp, &secret);
    store.create(TEST_DID, &keypair).await.unwrap();
    let reader = store
        .read(TEST_DID.to_owned(), Arc::new(MemoryBlobStore::default()))
        .await
        .unwrap();
    let account_db = crate::account_manager::db::get_migrated_db(dir.path().join("account.sqlite"))
        .await
        .unwrap();
    register_actor(
        TEST_DID.to_owned(),
        TEST_HANDLE.to_owned(),
        None,
        &account_db,
    )
    .await
    .unwrap();
    let account_manager = AccountManager::new(account_db);
    let viewer = LocalViewer::new(
        reader,
        account_manager,
        TEST_HOSTNAME.to_owned(),
        None,
        None,
        None,
        None,
    );
    (dir, viewer)
}

fn test_cid() -> Cid {
    Cid::from_str(TEST_CID).unwrap()
}

fn test_blob() -> Blob {
    Blob {
        r#type: Some("blob".to_owned()),
        r#ref: Some(test_cid()),
        cid: None,
        mime_type: "image/jpeg".to_owned(),
        size: Some(100),
        original: None,
    }
}

fn test_label() -> Label {
    Label {
        ver: Some(1),
        src: OTHER_DID.to_owned(),
        uri: TEST_DID.to_owned(),
        cid: None,
        val: "test-label".to_owned(),
        neg: None,
        cts: fixed_datetime(),
        exp: None,
        sig: None,
    }
}

fn fixed_datetime() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn profile_record(display_name: &str, description: &str) -> Profile {
    Profile {
        display_name: Some(display_name.to_owned()),
        description: Some(description.to_owned()),
        avatar: Some(test_blob()),
        banner: Some(test_blob()),
        labels: None,
        joined_via_starter_pack: None,
        created_at: Some(fixed_datetime()),
    }
}

fn profile_descript(record: Profile) -> RecordDescript<Profile> {
    RecordDescript {
        uri: AtUri::new(format!("at://{TEST_DID}/app.bsky.actor.profile/self"), None).unwrap(),
        cid: test_cid(),
        indexed_at: "2024-01-02T00:00:00.000Z".to_owned(),
        record,
    }
}

fn post_record(text: &str) -> Post {
    Post {
        created_at: fixed_datetime(),
        text: text.to_owned(),
        entities: None,
        facets: None,
        langs: None,
        labels: None,
        embed: None,
        reply: None,
        tags: None,
    }
}

fn post_descript(rkey: &str, record: Post, indexed_at: &str) -> RecordDescript<Post> {
    RecordDescript {
        uri: AtUri::new(format!("at://{TEST_DID}/app.bsky.feed.post/{rkey}"), None).unwrap(),
        cid: test_cid(),
        indexed_at: indexed_at.to_owned(),
        record,
    }
}

fn basic_view(did: &str, display_name: &str) -> ProfileViewBasic {
    ProfileViewBasic {
        did: did.to_owned(),
        handle: TEST_HANDLE.to_owned(),
        display_name: Some(display_name.to_owned()),
        avatar: None,
        associated: None,
        viewer: None,
        labels: None,
        created_at: None,
    }
}

fn detailed_view(did: &str) -> ProfileViewDetailed {
    ProfileViewDetailed {
        did: did.to_owned(),
        handle: TEST_HANDLE.to_owned(),
        display_name: Some("Old Name".to_owned()),
        description: Some("old bio".to_owned()),
        avatar: None,
        banner: None,
        followers_count: Some(3),
        follows_count: Some(4),
        posts_count: Some(10),
        associated: None,
        joined_via_starter_pack: None,
        viewer: None,
        labels: vec![test_label()],
        indexed_at: Some("2024-01-01T00:00:00.000Z".to_owned()),
        created_at: None,
    }
}

fn post_view(did: &str, uri: &str, indexed_at: &str) -> PostView {
    PostView {
        uri: uri.to_owned(),
        cid: TEST_CID.to_owned(),
        author: basic_view(did, "Old Name"),
        record: serde_json::to_value(post_record("upstream post")).unwrap(),
        embed: None,
        reply_count: Some(1),
        repost_count: Some(2),
        like_count: Some(3),
        indexed_at: indexed_at.to_owned(),
        viewer: None,
        labels: None,
    }
}

fn feed_item(did: &str, uri: &str, indexed_at: &str) -> FeedViewPost {
    FeedViewPost {
        post: post_view(did, uri, indexed_at),
        reply: None,
        reason: None,
        feed_context: None,
    }
}

fn local_records(
    profile: Option<RecordDescript<Profile>>,
    posts: Vec<RecordDescript<Post>>,
) -> LocalRecords {
    LocalRecords {
        count: profile.iter().count() as i64 + posts.len() as i64,
        profile,
        posts,
    }
}

// getProfile munge
// ----------------

#[tokio::test(flavor = "multi_thread")]
async fn profile_munge_without_local_profile_returns_original() {
    let (_dir, viewer) = test_viewer().await;
    let original = detailed_view(TEST_DID);
    let local = local_records(
        None,
        vec![post_descript(
            "a",
            post_record("hi"),
            "2024-01-02T00:00:00.000Z",
        )],
    );
    let munged = get_profile_munge(viewer, original.clone(), local, TEST_DID.to_owned()).unwrap();
    assert_eq!(munged, original);
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_munge_skips_profiles_of_other_accounts() {
    let (_dir, viewer) = test_viewer().await;
    let original = detailed_view(OTHER_DID);
    let local = local_records(
        Some(profile_descript(profile_record("New", "new bio"))),
        vec![],
    );
    let munged = get_profile_munge(viewer, original.clone(), local, TEST_DID.to_owned()).unwrap();
    assert_eq!(munged, original);
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_munge_overlays_record_fields_and_counts_local_posts() {
    let (_dir, viewer) = test_viewer().await;
    let original = detailed_view(TEST_DID);
    let local = local_records(
        Some(profile_descript(profile_record("New Name", "new bio"))),
        vec![
            post_descript("a", post_record("one"), "2024-01-02T00:00:00.000Z"),
            post_descript("b", post_record("two"), "2024-01-03T00:00:00.000Z"),
        ],
    );
    let munged = get_profile_munge(viewer, original, local, TEST_DID.to_owned()).unwrap();
    assert_eq!(munged.display_name, Some("New Name".to_owned()));
    assert_eq!(munged.description, Some("new bio".to_owned()));
    let avatar = munged.avatar.expect("avatar url");
    assert!(avatar.contains(TEST_HOSTNAME) && avatar.contains(TEST_CID));
    let banner = munged.banner.expect("banner url");
    assert!(banner.contains(TEST_HOSTNAME) && banner.contains(TEST_CID));
    // local writes are added to the upstream count
    assert_eq!(munged.posts_count, Some(12));
    // untouched upstream fields survive the munge
    assert_eq!(munged.followers_count, Some(3));
    assert_eq!(munged.follows_count, Some(4));
    assert_eq!(munged.labels, vec![test_label()]);
    assert_eq!(
        munged.indexed_at,
        Some("2024-01-01T00:00:00.000Z".to_owned())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_munge_clears_fields_missing_from_local_record() {
    let (_dir, viewer) = test_viewer().await;
    let mut original = detailed_view(TEST_DID);
    original.avatar = Some("https://cdn.example.com/old-avatar".to_owned());
    let record = Profile {
        display_name: None,
        description: None,
        avatar: None,
        banner: None,
        labels: None,
        joined_via_starter_pack: None,
        created_at: Some(fixed_datetime()),
    };
    let local = local_records(Some(profile_descript(record)), vec![]);
    let munged = get_profile_munge(viewer, original, local, TEST_DID.to_owned()).unwrap();
    assert_eq!(munged.display_name, None);
    assert_eq!(munged.description, None);
    assert_eq!(munged.avatar, None);
    assert_eq!(munged.posts_count, Some(10));
}

// getProfiles munge
// -----------------

#[tokio::test(flavor = "multi_thread")]
async fn profiles_munge_updates_only_the_requester() {
    let (_dir, viewer) = test_viewer().await;
    let original = GetProfilesOutput {
        profiles: vec![detailed_view(OTHER_DID), detailed_view(TEST_DID)],
    };
    let local = local_records(
        Some(profile_descript(profile_record("New Name", "new bio"))),
        vec![post_descript(
            "a",
            post_record("one"),
            "2024-01-02T00:00:00.000Z",
        )],
    );
    let munged = get_profiles_munge(viewer, original, local, TEST_DID.to_owned()).unwrap();
    assert_eq!(munged.profiles[0].display_name, Some("Old Name".to_owned()));
    assert_eq!(munged.profiles[0].posts_count, Some(10));
    assert_eq!(munged.profiles[1].display_name, Some("New Name".to_owned()));
    assert_eq!(munged.profiles[1].description, Some("new bio".to_owned()));
    assert_eq!(munged.profiles[1].posts_count, Some(11));
}

// Feed fixtures as the appview sends them
// ---------------------------------------

fn feed_item_json(did: &str, uri: &str, indexed_at: &str) -> Value {
    serde_json::to_value(feed_item(did, uri, indexed_at)).unwrap()
}

fn author_feed_json(cursor: Option<&str>, feed: Vec<Value>) -> Value {
    json!({ "cursor": cursor, "feed": feed })
}

/// A page item carrying everything the typed structs used to drop.
fn rich_item(did: &str, uri: &str, indexed_at: &str) -> Value {
    let mut item = feed_item_json(did, uri, indexed_at);
    item["post"]["author"]["viewer"] = json!({ "mutedOnlyReposts": true, "following": "at://x" });
    item["post"]["author"]["verification"] = json!({ "verifiedStatus": "valid" });
    item["post"]["viewer"] = json!({
        "repost": format!("at://{TEST_DID}/app.bsky.feed.repost/r"),
        "like": format!("at://{TEST_DID}/app.bsky.feed.like/l"),
        "threadMuted": true
    });
    item["post"]["quoteCount"] = json!(7);
    item["post"]["bookmarkCount"] = json!(1);
    item["post"]["communitySpace"] = json!("at://did:plc:c/space/community.blacksky.feed/main");
    item["feedContext"] = json!("ctx");
    item["unknownItemField"] = json!({ "nested": [1, 2, 3] });
    item
}

fn repost_reason(by: &str) -> Value {
    json!({
        "$type": "app.bsky.feed.defs#reasonRepost",
        "by": { "did": by, "handle": TEST_HANDLE, "viewer": { "mutedOnlyReposts": true } },
        "uri": format!("at://{by}/app.bsky.feed.repost/3kreason"),
        "cid": TEST_CID,
        "indexedAt": "2024-01-01T00:00:00.000Z"
    })
}

fn reposted_item(by: &str, author: &str, uri: &str, indexed_at: &str) -> Value {
    let mut item = rich_item(author, uri, indexed_at);
    item["reason"] = repost_reason(by);
    item
}

fn pinned_item(did: &str, uri: &str, indexed_at: &str) -> Value {
    let mut item = feed_item_json(did, uri, indexed_at);
    item["reason"] = json!({ "$type": "app.bsky.feed.defs#reasonPin" });
    item
}

fn post_uri(item: &Value) -> &str {
    item.pointer("/post/uri").and_then(Value::as_str).unwrap()
}

fn thread_node(did: &str, uri: &str, indexed_at: &str) -> Value {
    let mut node = serde_json::to_value(ThreadViewPostEnum::ThreadViewPost(ThreadViewPost {
        post: post_view(did, uri, indexed_at),
        parent: None,
        replies: None,
    }))
    .unwrap();
    node["post"]["viewer"] = json!({ "repost": "at://did:plc:v/app.bsky.feed.repost/x" });
    node["post"]["quoteCount"] = json!(9);
    node
}

fn reply_record(text: &str, root_uri: &str, parent_uri: &str) -> Post {
    let mut record = post_record(text);
    record.reply = Some(ReplyRef {
        root: StrongRef {
            uri: root_uri.to_owned(),
            cid: TEST_CID.to_owned(),
        },
        parent: StrongRef {
            uri: parent_uri.to_owned(),
            cid: TEST_CID.to_owned(),
        },
    });
    record
}

fn assert_wire_round_trip(value: &Value) {
    let bytes = serde_json::to_vec(value).unwrap();
    let parsed: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(&parsed, value);
}

// getAuthorFeed munge
// -------------------

#[tokio::test(flavor = "multi_thread")]
async fn author_feed_munge_leaves_foreign_feeds_untouched() {
    let (_dir, viewer) = test_viewer().await;
    let original = author_feed_json(
        None,
        vec![feed_item_json(
            OTHER_DID,
            &format!("at://{OTHER_DID}/app.bsky.feed.post/1"),
            "2024-01-01T00:00:00.000Z",
        )],
    );
    let local = local_records(
        Some(profile_descript(profile_record("New Name", "new bio"))),
        vec![post_descript(
            "a",
            post_record("one"),
            "2024-01-02T00:00:00.000Z",
        )],
    );
    let munged =
        get_author_feed_munge(viewer, original.clone(), local, TEST_DID.to_owned()).unwrap();
    assert_eq!(munged, original);
}

#[tokio::test(flavor = "multi_thread")]
async fn author_feed_munge_passes_through_a_body_without_a_feed() {
    let (_dir, viewer) = test_viewer().await;
    let original = json!({ "unexpected": true });
    let local = local_records(None, vec![]);
    let munged =
        get_author_feed_munge(viewer, original.clone(), local, TEST_DID.to_owned()).unwrap();
    assert_eq!(munged, original);
}

#[tokio::test(flavor = "multi_thread")]
async fn author_feed_munge_updates_author_and_inserts_local_posts() {
    let (_dir, viewer) = test_viewer().await;
    let upstream_uri = format!("at://{TEST_DID}/app.bsky.feed.post/upstream");
    let upstream = rich_item(TEST_DID, &upstream_uri, "2024-01-01T00:00:00.000Z");
    let original = author_feed_json(Some("cursor"), vec![upstream.clone()]);
    let local_uri = format!("at://{TEST_DID}/app.bsky.feed.post/local");
    let local = local_records(
        Some(profile_descript(profile_record("New Name", "new bio"))),
        vec![post_descript(
            "local",
            post_record("fresh"),
            "2024-01-02T00:00:00.000Z",
        )],
    );
    let munged = get_author_feed_munge(viewer, original, local, TEST_DID.to_owned()).unwrap();
    assert_eq!(munged["cursor"], json!("cursor"));
    let feed = munged["feed"].as_array().unwrap();
    assert_eq!(feed.len(), 2);
    // the newer local post is inserted ahead of the upstream one
    assert_eq!(post_uri(&feed[0]), local_uri);
    assert_eq!(feed[0]["post"]["author"]["did"], json!(TEST_DID));
    assert_eq!(feed[0]["post"]["replyCount"], json!(0));
    assert_eq!(feed[0]["post"]["repostCount"], json!(0));
    assert_eq!(feed[0]["post"]["likeCount"], json!(0));
    // the upstream item had exactly two author keys refreshed from the local
    // profile and kept everything else the appview sent
    let mut expected = upstream;
    expected["post"]["author"]["displayName"] = json!("New Name");
    let avatar = feed[1]["post"]["author"]["avatar"].as_str().unwrap();
    assert!(avatar.contains(TEST_HOSTNAME) && avatar.contains(TEST_CID));
    expected["post"]["author"]["avatar"] = json!(avatar);
    assert_eq!(feed[1], expected);
    assert_wire_round_trip(&munged);
}

#[tokio::test(flavor = "multi_thread")]
async fn author_feed_munge_removes_author_keys_the_pending_profile_cleared() {
    let (_dir, viewer) = test_viewer().await;
    let upstream_uri = format!("at://{TEST_DID}/app.bsky.feed.post/upstream");
    let mut upstream = rich_item(TEST_DID, &upstream_uri, "2024-01-01T00:00:00.000Z");
    upstream["post"]["author"]["avatar"] = json!("https://cdn.example/old.jpg");
    let original = author_feed_json(None, vec![upstream]);
    let mut cleared = profile_record("Only Name", "bio");
    cleared.avatar = None;
    let local = local_records(Some(profile_descript(cleared)), vec![]);
    let munged = get_author_feed_munge(viewer, original, local, TEST_DID.to_owned()).unwrap();
    let author = &munged["feed"][0]["post"]["author"];
    assert_eq!(author["displayName"], json!("Only Name"));
    assert!(author.get("avatar").is_none());
    assert_eq!(author["viewer"]["mutedOnlyReposts"], json!(true));
}

#[tokio::test(flavor = "multi_thread")]
async fn author_feed_munge_inserts_posts_without_local_profile() {
    let (_dir, viewer) = test_viewer().await;
    let original = author_feed_json(
        None,
        vec![feed_item_json(
            TEST_DID,
            &format!("at://{TEST_DID}/app.bsky.feed.post/upstream"),
            "2024-01-01T00:00:00.000Z",
        )],
    );
    let local = local_records(
        None,
        vec![post_descript(
            "local",
            post_record("fresh"),
            "2024-01-02T00:00:00.000Z",
        )],
    );
    let munged = get_author_feed_munge(viewer, original, local, TEST_DID.to_owned()).unwrap();
    let feed = munged["feed"].as_array().unwrap();
    assert_eq!(feed.len(), 2);
    assert_eq!(
        post_uri(&feed[0]),
        format!("at://{TEST_DID}/app.bsky.feed.post/local")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn author_feed_munge_keeps_a_pinned_item_first() {
    let (_dir, viewer) = test_viewer().await;
    let pin_uri = format!("at://{TEST_DID}/app.bsky.feed.post/pin");
    let older_uri = format!("at://{TEST_DID}/app.bsky.feed.post/older");
    let original = author_feed_json(
        None,
        vec![
            pinned_item(TEST_DID, &pin_uri, "2023-06-01T00:00:00.000Z"),
            feed_item_json(TEST_DID, &older_uri, "2024-01-01T00:00:00.000Z"),
        ],
    );
    let local = local_records(
        None,
        vec![post_descript(
            "local",
            post_record("fresh"),
            "2024-01-02T00:00:00.000Z",
        )],
    );
    let munged = get_author_feed_munge(viewer, original, local, TEST_DID.to_owned()).unwrap();
    let uris: Vec<&str> = munged["feed"]
        .as_array()
        .unwrap()
        .iter()
        .map(post_uri)
        .collect();
    assert_eq!(
        uris,
        vec![
            pin_uri.as_str(),
            &format!("at://{TEST_DID}/app.bsky.feed.post/local"),
            older_uri.as_str()
        ]
    );
    assert_eq!(
        munged["feed"][0]["reason"]["$type"],
        json!("app.bsky.feed.defs#reasonPin")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn author_feed_munge_preserves_reposts_and_viewer_state() {
    let (_dir, viewer) = test_viewer().await;
    let original = author_feed_json(
        Some("cursor"),
        vec![
            reposted_item(
                TEST_DID,
                OTHER_DID,
                &format!("at://{OTHER_DID}/app.bsky.feed.post/reposted"),
                "2024-01-03T00:00:00.000Z",
            ),
            rich_item(
                TEST_DID,
                &format!("at://{TEST_DID}/app.bsky.feed.post/own"),
                "2024-01-01T00:00:00.000Z",
            ),
        ],
    );
    // a pending repost bumps the local count without carrying a post
    let local = LocalRecords {
        count: 1,
        profile: None,
        posts: vec![],
    };
    let munged =
        get_author_feed_munge(viewer, original.clone(), local, TEST_DID.to_owned()).unwrap();
    assert_eq!(munged, original);
    assert_wire_round_trip(&munged);
}

#[test]
fn insert_feed_item_appends_when_nothing_is_older() {
    let newer = feed_item_json(
        TEST_DID,
        &format!("at://{TEST_DID}/app.bsky.feed.post/newer"),
        "2024-01-05T00:00:00.000Z",
    );
    let older = feed_item_json(
        TEST_DID,
        &format!("at://{TEST_DID}/app.bsky.feed.post/older"),
        "2024-01-01T00:00:00.000Z",
    );
    let mut feed = vec![];
    feed_json::insert_feed_item(&mut feed, older.clone());
    assert_eq!(feed, vec![older.clone()]);
    feed_json::insert_feed_item(&mut feed, newer.clone());
    assert_eq!(feed, vec![newer, older]);
}

#[test]
fn is_users_feed_reads_the_item_actor() {
    let own = feed_item_json(
        TEST_DID,
        &format!("at://{TEST_DID}/app.bsky.feed.post/a"),
        "2024-01-01T00:00:00.000Z",
    );
    let other = feed_item_json(
        OTHER_DID,
        &format!("at://{OTHER_DID}/app.bsky.feed.post/b"),
        "2024-01-01T00:00:00.000Z",
    );
    let reposted = reposted_item(
        TEST_DID,
        OTHER_DID,
        &format!("at://{OTHER_DID}/app.bsky.feed.post/c"),
        "2024-01-01T00:00:00.000Z",
    );
    let pinned = pinned_item(
        TEST_DID,
        &format!("at://{TEST_DID}/app.bsky.feed.post/d"),
        "2024-01-01T00:00:00.000Z",
    );
    assert!(feed_json::is_users_feed(&[own], TEST_DID));
    assert!(feed_json::is_users_feed(&[reposted], TEST_DID));
    assert!(feed_json::is_users_feed(&[pinned], TEST_DID));
    assert!(!feed_json::is_users_feed(&[other], TEST_DID));
    assert!(!feed_json::is_users_feed(&[], TEST_DID));
}

// getActorLikes munge
// -------------------

#[tokio::test(flavor = "multi_thread")]
async fn actor_likes_munge_updates_profile_but_never_inserts_posts() {
    let (_dir, viewer) = test_viewer().await;
    let liked = rich_item(
        TEST_DID,
        &format!("at://{TEST_DID}/app.bsky.feed.post/liked"),
        "2024-01-01T00:00:00.000Z",
    );
    let foreign = rich_item(
        OTHER_DID,
        &format!("at://{OTHER_DID}/app.bsky.feed.post/theirs"),
        "2024-01-01T00:00:00.000Z",
    );
    let original = author_feed_json(None, vec![liked.clone(), foreign.clone()]);
    let local = local_records(
        Some(profile_descript(profile_record("New Name", "new bio"))),
        vec![post_descript(
            "local",
            post_record("fresh"),
            "2024-01-02T00:00:00.000Z",
        )],
    );
    let munged = get_actor_likes_munge(viewer, original, local, TEST_DID.to_owned()).unwrap();
    let feed = munged["feed"].as_array().unwrap();
    assert_eq!(feed.len(), 2);
    assert_eq!(feed[0]["post"]["author"]["displayName"], json!("New Name"));
    assert_eq!(feed[0]["post"]["quoteCount"], json!(7));
    assert_eq!(feed[0]["post"]["viewer"], liked["post"]["viewer"]);
    assert_eq!(feed[0]["unknownItemField"], liked["unknownItemField"]);
    assert_eq!(feed[1], foreign);
}

#[tokio::test(flavor = "multi_thread")]
async fn actor_likes_munge_without_local_profile_returns_original() {
    let (_dir, viewer) = test_viewer().await;
    let original = author_feed_json(
        None,
        vec![rich_item(
            TEST_DID,
            &format!("at://{TEST_DID}/app.bsky.feed.post/liked"),
            "2024-01-01T00:00:00.000Z",
        )],
    );
    let local = local_records(None, vec![]);
    let munged =
        get_actor_likes_munge(viewer, original.clone(), local, TEST_DID.to_owned()).unwrap();
    assert_eq!(munged, original);
}

// getTimeline munge
// -----------------

#[tokio::test(flavor = "multi_thread")]
async fn timeline_munge_inserts_local_posts_in_order() {
    let (_dir, viewer) = test_viewer().await;
    let original = author_feed_json(
        Some("cursor"),
        vec![feed_item_json(
            OTHER_DID,
            &format!("at://{OTHER_DID}/app.bsky.feed.post/1"),
            "2024-01-01T00:00:00.000Z",
        )],
    );
    let local = local_records(
        None,
        vec![
            post_descript("a", post_record("older"), "2024-01-02T00:00:00.000Z"),
            post_descript("b", post_record("newer"), "2024-01-03T00:00:00.000Z"),
        ],
    );
    let munged = get_timeline_munge(viewer, original, local, TEST_DID.to_owned()).unwrap();
    assert_eq!(munged["cursor"], json!("cursor"));
    let uris: Vec<&str> = munged["feed"]
        .as_array()
        .unwrap()
        .iter()
        .map(post_uri)
        .collect();
    assert_eq!(
        uris,
        vec![
            format!("at://{TEST_DID}/app.bsky.feed.post/b"),
            format!("at://{TEST_DID}/app.bsky.feed.post/a"),
            format!("at://{OTHER_DID}/app.bsky.feed.post/1"),
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn timeline_munge_skips_posts_older_than_the_feed_window() {
    let (_dir, viewer) = test_viewer().await;
    let original = author_feed_json(
        None,
        vec![feed_item_json(
            OTHER_DID,
            &format!("at://{OTHER_DID}/app.bsky.feed.post/1"),
            "2024-01-05T00:00:00.000Z",
        )],
    );
    let local = local_records(
        None,
        vec![post_descript(
            "a",
            post_record("stale"),
            "2024-01-02T00:00:00.000Z",
        )],
    );
    let munged = get_timeline_munge(viewer, original.clone(), local, TEST_DID.to_owned()).unwrap();
    assert_eq!(munged, original);
}

#[tokio::test(flavor = "multi_thread")]
async fn timeline_munge_keeps_reposts_pins_and_viewer_state_while_inserting() {
    let (_dir, viewer) = test_viewer().await;
    let reposted = reposted_item(
        OTHER_DID,
        TEST_DID,
        &format!("at://{TEST_DID}/app.bsky.feed.post/reposted"),
        "2024-01-04T00:00:00.000Z",
    );
    let pinned = pinned_item(
        OTHER_DID,
        &format!("at://{OTHER_DID}/app.bsky.feed.post/pinned"),
        "2024-01-01T00:00:00.000Z",
    );
    let original = author_feed_json(None, vec![reposted.clone(), pinned.clone()]);
    let local = local_records(
        None,
        vec![post_descript(
            "local",
            post_record("fresh"),
            "2024-01-02T00:00:00.000Z",
        )],
    );
    let munged = get_timeline_munge(viewer, original, local, TEST_DID.to_owned()).unwrap();
    let feed = munged["feed"].as_array().unwrap();
    assert_eq!(feed.len(), 3);
    assert_eq!(feed[0], reposted);
    assert_eq!(
        post_uri(&feed[1]),
        format!("at://{TEST_DID}/app.bsky.feed.post/local")
    );
    assert_eq!(feed[2], pinned);
    assert_wire_round_trip(&munged);
}

#[tokio::test(flavor = "multi_thread")]
async fn timeline_munge_passes_through_a_body_without_a_feed() {
    let (_dir, viewer) = test_viewer().await;
    let original = json!({ "unexpected": true });
    let local = local_records(
        None,
        vec![post_descript(
            "a",
            post_record("hi"),
            "2024-01-02T00:00:00.000Z",
        )],
    );
    let munged = get_timeline_munge(viewer, original.clone(), local, TEST_DID.to_owned()).unwrap();
    assert_eq!(munged, original);
}

// getPostThread munge
// -------------------

#[tokio::test(flavor = "multi_thread")]
async fn post_thread_munge_grafts_local_replies_onto_the_thread() {
    let (_dir, viewer) = test_viewer().await;
    let root_uri = format!("at://{TEST_DID}/app.bsky.feed.post/root");
    let root = thread_node(TEST_DID, &root_uri, "2024-01-01T00:00:00.000Z");
    let original = json!({ "thread": root.clone() });
    let local = local_records(
        None,
        vec![post_descript(
            "reply",
            reply_record("local reply", &root_uri, &root_uri),
            "2024-01-02T00:00:00.000Z",
        )],
    );
    let munged = get_post_thread_munge(viewer, original, local, TEST_DID.to_owned()).unwrap();
    let thread = &munged["thread"];
    assert_eq!(thread["$type"], json!("app.bsky.feed.defs#threadViewPost"));
    assert_eq!(thread["post"], root["post"]);
    let replies = thread["replies"].as_array().unwrap();
    assert_eq!(replies.len(), 1);
    assert_eq!(
        replies[0]["$type"],
        json!("app.bsky.feed.defs#threadViewPost")
    );
    assert_eq!(
        post_uri(&replies[0]),
        format!("at://{TEST_DID}/app.bsky.feed.post/reply")
    );
    assert_eq!(replies[0]["post"]["author"]["did"], json!(TEST_DID));
    assert_wire_round_trip(&munged);
}

#[tokio::test(flavor = "multi_thread")]
async fn post_thread_munge_prepends_replies_and_reaches_nested_parents() {
    let (_dir, viewer) = test_viewer().await;
    let root_uri = format!("at://{TEST_DID}/app.bsky.feed.post/root");
    let child_uri = format!("at://{OTHER_DID}/app.bsky.feed.post/child");
    let sibling_uri = format!("at://{OTHER_DID}/app.bsky.feed.post/sibling");
    let mut root = thread_node(TEST_DID, &root_uri, "2024-01-01T00:00:00.000Z");
    let child = thread_node(OTHER_DID, &child_uri, "2024-01-01T01:00:00.000Z");
    let sibling = thread_node(OTHER_DID, &sibling_uri, "2024-01-01T02:00:00.000Z");
    root["replies"] = json!([child, sibling]);
    let original = json!({ "thread": root });
    let local = local_records(
        None,
        vec![
            post_descript(
                "to-root",
                reply_record("to root", &root_uri, &root_uri),
                "2024-01-02T00:00:00.000Z",
            ),
            post_descript(
                "to-child",
                reply_record("to child", &root_uri, &child_uri),
                "2024-01-02T00:00:00.000Z",
            ),
        ],
    );
    let munged = get_post_thread_munge(viewer, original, local, TEST_DID.to_owned()).unwrap();
    let replies = munged["thread"]["replies"].as_array().unwrap();
    let uris: Vec<&str> = replies.iter().map(post_uri).collect();
    assert_eq!(
        uris,
        vec![
            format!("at://{TEST_DID}/app.bsky.feed.post/to-root"),
            child_uri.clone(),
            sibling_uri.clone(),
        ]
    );
    let nested = replies[1]["replies"].as_array().unwrap();
    assert_eq!(nested.len(), 1);
    assert_eq!(
        post_uri(&nested[0]),
        format!("at://{TEST_DID}/app.bsky.feed.post/to-child")
    );
    // the anchor and the existing children keep their viewer state and counters
    assert_eq!(munged["thread"]["post"]["quoteCount"], json!(9));
    assert_eq!(
        replies[2]["post"]["viewer"]["repost"],
        json!("at://did:plc:v/app.bsky.feed.repost/x")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn post_thread_munge_passes_through_not_found_threads() {
    let (_dir, viewer) = test_viewer().await;
    let original = json!({
        "thread": {
            "$type": "app.bsky.feed.defs#notFoundPost",
            "uri": format!("at://{TEST_DID}/app.bsky.feed.post/missing"),
            "notFound": true
        }
    });
    let local = local_records(
        None,
        vec![post_descript(
            "a",
            post_record("hi"),
            "2024-01-02T00:00:00.000Z",
        )],
    );
    let munged =
        get_post_thread_munge(viewer, original.clone(), local, TEST_DID.to_owned()).unwrap();
    assert_eq!(munged, original);
}

#[tokio::test(flavor = "multi_thread")]
async fn post_thread_munge_passes_through_a_body_without_a_thread() {
    let (_dir, viewer) = test_viewer().await;
    let original = json!({ "unexpected": true });
    let local = local_records(None, vec![]);
    let munged =
        get_post_thread_munge(viewer, original.clone(), local, TEST_DID.to_owned()).unwrap();
    assert_eq!(munged, original);
}

#[tokio::test(flavor = "multi_thread")]
async fn thread_post_view_carries_the_thread_discriminator() {
    let (_dir, viewer) = test_viewer().await;
    let node = thread_post_view(
        &viewer,
        post_descript("a", post_record("hi"), "2024-01-02T00:00:00.000Z"),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(node["$type"], json!("app.bsky.feed.defs#threadViewPost"));
    assert_eq!(
        post_uri(&node),
        format!("at://{TEST_DID}/app.bsky.feed.post/a")
    );
}

#[test]
fn highest_parent_walks_the_json_parent_chain() {
    let root_uri = format!("at://{OTHER_DID}/app.bsky.feed.post/root");
    let mid_uri = format!("at://{OTHER_DID}/app.bsky.feed.post/mid");
    let leaf_uri = format!("at://{TEST_DID}/app.bsky.feed.post/leaf");
    let mut mid = thread_node(OTHER_DID, &mid_uri, "2024-01-01T00:00:00.000Z");
    mid["post"]["record"] =
        serde_json::to_value(reply_record("mid", &root_uri, &root_uri)).unwrap();
    mid["unknownParentField"] = json!(true);
    let mut leaf = thread_node(TEST_DID, &leaf_uri, "2024-01-02T00:00:00.000Z");
    leaf["post"]["record"] =
        serde_json::to_value(reply_record("leaf", &root_uri, &mid_uri)).unwrap();
    assert_eq!(feed_json::highest_parent(&leaf), Some(mid_uri.clone()));
    leaf["parent"] = mid;
    assert_eq!(feed_json::highest_parent(&leaf), Some(root_uri));
    leaf["parent"] =
        json!({ "$type": "app.bsky.feed.defs#notFoundPost", "uri": mid_uri, "notFound": true });
    assert_eq!(feed_json::highest_parent(&leaf), None);
    let plain = thread_node(TEST_DID, &leaf_uri, "2024-01-02T00:00:00.000Z");
    assert_eq!(feed_json::highest_parent(&plain), None);
}

#[test]
fn graft_reply_creates_a_reply_list_when_the_appview_sent_null() {
    let root_uri = format!("at://{TEST_DID}/app.bsky.feed.post/root");
    let mut root = thread_node(TEST_DID, &root_uri, "2024-01-01T00:00:00.000Z");
    root["replies"] = Value::Null;
    let node = json!({ "$type": "app.bsky.feed.defs#threadViewPost", "post": { "uri": "x" } });
    feed_json::graft_reply(&mut root, &root_uri, &node);
    assert_eq!(root["replies"], json!([node]));
    // a reply nobody in the thread is the parent of is left out
    feed_json::graft_reply(&mut root, "at://nobody", &node);
    assert_eq!(root["replies"].as_array().unwrap().len(), 1);
}

#[test]
fn find_posts_in_thread_matches_the_anchor_or_its_root() {
    let root_uri = format!("at://{OTHER_DID}/app.bsky.feed.post/root");
    let anchor_uri = format!("at://{OTHER_DID}/app.bsky.feed.post/anchor");
    let mut anchor = thread_node(OTHER_DID, &anchor_uri, "2024-01-01T00:00:00.000Z");
    anchor["post"]["record"] =
        serde_json::to_value(reply_record("anchor", &root_uri, &root_uri)).unwrap();
    let posts = vec![
        post_descript(
            "a",
            reply_record("a", &anchor_uri, &anchor_uri),
            "2024-01-02T00:00:00.000Z",
        ),
        post_descript(
            "b",
            reply_record("b", &root_uri, &anchor_uri),
            "2024-01-02T00:00:00.000Z",
        ),
        post_descript(
            "c",
            reply_record("c", "at://elsewhere", "at://elsewhere"),
            "2024-01-02T00:00:00.000Z",
        ),
        post_descript("d", post_record("not a reply"), "2024-01-02T00:00:00.000Z"),
    ];
    let found: Vec<(String, String)> = feed_json::find_posts_in_thread(&anchor, posts)
        .into_iter()
        .map(|(parent, p)| (parent, p.uri.get_rkey()))
        .collect();
    assert_eq!(
        found,
        vec![
            (anchor_uri.clone(), "a".to_owned()),
            (anchor_uri, "b".to_owned())
        ]
    );
}

// Embed formatting
// ----------------

#[tokio::test(flavor = "multi_thread")]
async fn video_embeds_format_to_no_view_instead_of_panicking() {
    let (_dir, viewer) = test_viewer().await;
    let mut post = post_record("video post");
    post.embed = Some(Embeds::Video(Video {
        video: test_blob(),
        captions: None,
        alt: None,
        aspect_ratio: None,
    }));
    assert_eq!(viewer.format_post_embed(post).await.unwrap(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn gallery_embeds_format_to_no_view() {
    let (_dir, viewer) = test_viewer().await;
    let gallery = Gallery {
        items: vec![GalleryItem::Image(GalleryImage {
            image: test_blob(),
            alt: "alt text".to_owned(),
            aspect_ratio: AspectRatio {
                width: 4,
                height: 3,
            },
        })],
    };
    let mut post = post_record("gallery post");
    post.embed = Some(Embeds::Gallery(gallery.clone()));
    assert_eq!(viewer.format_post_embed(post).await.unwrap(), None);
    assert_eq!(
        viewer
            .format_simple_embed(MediaUnion::Gallery(gallery))
            .await,
        None
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn record_with_video_media_formats_to_no_view() {
    let (_dir, viewer) = test_viewer().await;
    let embed = RecordWithMedia {
        record: RecordEmbed {
            record: StrongRef {
                uri: format!("at://{TEST_DID}/app.bsky.feed.post/quoted"),
                cid: TEST_CID.to_owned(),
            },
        },
        media: MediaUnion::Video(Video {
            video: test_blob(),
            captions: None,
            alt: None,
            aspect_ratio: None,
        }),
    };
    assert_eq!(
        viewer.format_record_with_media_embed(embed).await.unwrap(),
        None
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn image_embeds_format_to_blob_urls() {
    let (_dir, viewer) = test_viewer().await;
    let embed = MediaUnion::Images(Images {
        images: vec![Image {
            image: test_blob(),
            alt: "alt text".to_owned(),
            aspect_ratio: None,
        }],
    });
    let Some(EmbedViews::ImagesView(view)) = viewer.format_simple_embed(embed).await else {
        panic!("expected images view");
    };
    assert_eq!(view.images.len(), 1);
    assert!(view.images[0].thumb.contains(TEST_HOSTNAME));
    assert!(view.images[0].thumb.contains(TEST_CID));
    assert!(view.images[0].fullsize.contains(TEST_CID));
    assert_eq!(view.images[0].alt, "alt text");
}

#[tokio::test(flavor = "multi_thread")]
async fn external_embeds_format_thumb_to_blob_url() {
    let (_dir, viewer) = test_viewer().await;
    let embed = MediaUnion::External(External {
        external: ExternalObject {
            uri: "https://example.com".to_owned(),
            title: "Example".to_owned(),
            description: "A link".to_owned(),
            thumb: Some(test_blob()),
        },
    });
    let Some(EmbedViews::ExternalView(view)) = viewer.format_simple_embed(embed).await else {
        panic!("expected external view");
    };
    assert_eq!(view.external.uri, "https://example.com");
    assert!(view.external.thumb.unwrap().contains(TEST_CID));
}

/// The viewer signs with its own actor's key, so it may only issue tokens
/// for that actor.
#[tokio::test]
async fn service_auth_headers_refuse_another_accounts_did() {
    let (_dir, viewer) = test_viewer().await;
    let error = viewer
        .service_auth_headers(OTHER_DID, "app.bsky.feed.getTimeline")
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        format!("Cannot issue a service token for {OTHER_DID} from {TEST_DID}")
    );

    // the requester's own did gets past the guard
    let error = viewer
        .service_auth_headers(TEST_DID, "app.bsky.feed.getTimeline")
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Could not find bsky appview did");
}

#[tokio::test]
async fn service_auth_headers_are_signed_with_the_actors_key() {
    let (dir, viewer) = test_viewer().await;
    let viewer = LocalViewer::new(
        viewer.actor_store,
        viewer.account_manager,
        TEST_HOSTNAME.to_owned(),
        None,
        None,
        Some("did:web:appview.test".to_owned()),
        None,
    );
    let headers = viewer
        .service_auth_headers(TEST_DID, "app.bsky.feed.getTimeline")
        .await
        .unwrap();
    let jwt = headers
        .get(reqwest::header::AUTHORIZATION)
        .unwrap()
        .to_str()
        .unwrap()
        .strip_prefix("Bearer ")
        .unwrap()
        .to_owned();

    let secp = secp256k1::Secp256k1::new();
    let secret = secp256k1::SecretKey::from_slice(&hex::decode(TEST_SECRET_HEX).unwrap()).unwrap();
    let did_key = rsky_crypto::utils::encode_did_key(
        &secp256k1::Keypair::from_secret_key(&secp, &secret).public_key(),
    );
    let payload = crate::xrpc_server::auth::verify_jwt(
        jwt,
        Some("did:web:appview.test".to_owned()),
        Some("app.bsky.feed.getTimeline"),
        move |_iss, _refresh| {
            let did_key = did_key.clone();
            async move { Ok(did_key) }
        },
    )
    .await
    .unwrap();
    assert_eq!(payload.iss, TEST_DID);
    drop(dir);
}
