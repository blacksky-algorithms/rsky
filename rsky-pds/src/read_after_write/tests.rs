use crate::account_manager::helpers::account::register_actor;
use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::MemoryBlobStore;
use crate::actor_store::ActorStore;
use crate::apis::app::bsky::actor::get_profile::get_profile_munge;
use crate::apis::app::bsky::actor::get_profiles::get_profiles_munge;
use crate::apis::app::bsky::feed::get_actor_likes::get_author_munge as get_actor_likes_munge;
use crate::apis::app::bsky::feed::get_author_feed::get_author_munge as get_author_feed_munge;
use crate::apis::app::bsky::feed::get_post_thread::get_post_thread_munge;
use crate::apis::app::bsky::feed::get_timeline::get_timeline_munge;
use crate::background::BackgroundQueue;
use crate::config::ActorStoreConfig;
use crate::read_after_write::types::{LocalRecords, RecordDescript};
use crate::read_after_write::viewer::LocalViewer;
use chrono::{DateTime, Utc};
use lexicon_cid::Cid;
use rsky_lexicon::app::bsky::actor::{
    GetProfilesOutput, Profile, ProfileViewBasic, ProfileViewDetailed,
};
use rsky_lexicon::app::bsky::embed::external::{External, ExternalObject};
use rsky_lexicon::app::bsky::embed::images::{Image, Images};
use rsky_lexicon::app::bsky::embed::record::Record as RecordEmbed;
use rsky_lexicon::app::bsky::embed::record_with_media::RecordWithMedia;
use rsky_lexicon::app::bsky::embed::video::Video;
use rsky_lexicon::app::bsky::embed::{EmbedViews, Embeds, MediaUnion};
use rsky_lexicon::app::bsky::feed::{
    AuthorFeed, FeedViewPost, GetPostThreadOutput, NotFoundPost, Post, PostView, ReplyRef,
    ThreadViewPost, ThreadViewPostEnum,
};
use rsky_lexicon::com::atproto::label::Label;
use rsky_lexicon::com::atproto::repo::{Blob, StrongRef};
use rsky_syntax::aturi::AtUri;
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
    let store = ActorStore::new(&cfg, BackgroundQueue::default());
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

// getAuthorFeed munge
// -------------------

#[tokio::test(flavor = "multi_thread")]
async fn author_feed_munge_leaves_foreign_feeds_untouched() {
    let (_dir, viewer) = test_viewer().await;
    let original = AuthorFeed {
        cursor: None,
        feed: vec![feed_item(
            OTHER_DID,
            &format!("at://{OTHER_DID}/app.bsky.feed.post/1"),
            "2024-01-01T00:00:00.000Z",
        )],
    };
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
async fn author_feed_munge_updates_author_and_inserts_local_posts() {
    let (_dir, viewer) = test_viewer().await;
    let upstream_uri = format!("at://{TEST_DID}/app.bsky.feed.post/upstream");
    let original = AuthorFeed {
        cursor: Some("cursor".to_owned()),
        feed: vec![feed_item(
            TEST_DID,
            &upstream_uri,
            "2024-01-01T00:00:00.000Z",
        )],
    };
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
    assert_eq!(munged.cursor, Some("cursor".to_owned()));
    assert_eq!(munged.feed.len(), 2);
    // the newer local post is inserted ahead of the upstream one
    assert_eq!(munged.feed[0].post.uri, local_uri);
    assert_eq!(munged.feed[0].post.author.did, TEST_DID);
    assert_eq!(munged.feed[0].post.reply_count, Some(0));
    assert_eq!(munged.feed[0].post.repost_count, Some(0));
    assert_eq!(munged.feed[0].post.like_count, Some(0));
    // the upstream item had its author refreshed from the local profile
    assert_eq!(munged.feed[1].post.uri, upstream_uri);
    assert_eq!(
        munged.feed[1].post.author.display_name,
        Some("New Name".to_owned())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn author_feed_munge_inserts_posts_without_local_profile() {
    let (_dir, viewer) = test_viewer().await;
    let original = AuthorFeed {
        cursor: None,
        feed: vec![feed_item(
            TEST_DID,
            &format!("at://{TEST_DID}/app.bsky.feed.post/upstream"),
            "2024-01-01T00:00:00.000Z",
        )],
    };
    let local = local_records(
        None,
        vec![post_descript(
            "local",
            post_record("fresh"),
            "2024-01-02T00:00:00.000Z",
        )],
    );
    let munged = get_author_feed_munge(viewer, original, local, TEST_DID.to_owned()).unwrap();
    assert_eq!(munged.feed.len(), 2);
    assert_eq!(
        munged.feed[0].post.uri,
        format!("at://{TEST_DID}/app.bsky.feed.post/local")
    );
}

// getActorLikes munge
// -------------------

#[tokio::test(flavor = "multi_thread")]
async fn actor_likes_munge_updates_profile_but_never_inserts_posts() {
    let (_dir, viewer) = test_viewer().await;
    let original = AuthorFeed {
        cursor: None,
        feed: vec![feed_item(
            TEST_DID,
            &format!("at://{TEST_DID}/app.bsky.feed.post/liked"),
            "2024-01-01T00:00:00.000Z",
        )],
    };
    let local = local_records(
        Some(profile_descript(profile_record("New Name", "new bio"))),
        vec![post_descript(
            "local",
            post_record("fresh"),
            "2024-01-02T00:00:00.000Z",
        )],
    );
    let munged = get_actor_likes_munge(viewer, original, local, TEST_DID.to_owned()).unwrap();
    assert_eq!(munged.feed.len(), 1);
    assert_eq!(
        munged.feed[0].post.author.display_name,
        Some("New Name".to_owned())
    );
}

// getTimeline munge
// -----------------

#[tokio::test(flavor = "multi_thread")]
async fn timeline_munge_inserts_local_posts_in_order() {
    let (_dir, viewer) = test_viewer().await;
    let original = AuthorFeed {
        cursor: Some("cursor".to_owned()),
        feed: vec![feed_item(
            OTHER_DID,
            &format!("at://{OTHER_DID}/app.bsky.feed.post/1"),
            "2024-01-01T00:00:00.000Z",
        )],
    };
    let local = local_records(
        None,
        vec![
            post_descript("a", post_record("older"), "2024-01-02T00:00:00.000Z"),
            post_descript("b", post_record("newer"), "2024-01-03T00:00:00.000Z"),
        ],
    );
    let munged = get_timeline_munge(viewer, original, local, TEST_DID.to_owned()).unwrap();
    assert_eq!(munged.cursor, Some("cursor".to_owned()));
    assert_eq!(munged.feed.len(), 3);
    assert_eq!(
        munged.feed[0].post.uri,
        format!("at://{TEST_DID}/app.bsky.feed.post/b")
    );
    assert_eq!(
        munged.feed[1].post.uri,
        format!("at://{TEST_DID}/app.bsky.feed.post/a")
    );
    assert_eq!(
        munged.feed[2].post.uri,
        format!("at://{OTHER_DID}/app.bsky.feed.post/1")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn timeline_munge_skips_posts_older_than_the_feed_window() {
    let (_dir, viewer) = test_viewer().await;
    let original = AuthorFeed {
        cursor: None,
        feed: vec![feed_item(
            OTHER_DID,
            &format!("at://{OTHER_DID}/app.bsky.feed.post/1"),
            "2024-01-05T00:00:00.000Z",
        )],
    };
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

// getPostThread munge
// -------------------

#[tokio::test(flavor = "multi_thread")]
async fn post_thread_munge_grafts_local_replies_onto_the_thread() {
    let (_dir, viewer) = test_viewer().await;
    let root_uri = format!("at://{TEST_DID}/app.bsky.feed.post/root");
    let original = GetPostThreadOutput {
        thread: ThreadViewPostEnum::ThreadViewPost(ThreadViewPost {
            post: post_view(TEST_DID, &root_uri, "2024-01-01T00:00:00.000Z"),
            parent: None,
            replies: None,
        }),
    };
    let mut reply = post_record("local reply");
    reply.reply = Some(ReplyRef {
        root: StrongRef {
            uri: root_uri.clone(),
            cid: TEST_CID.to_owned(),
        },
        parent: StrongRef {
            uri: root_uri.clone(),
            cid: TEST_CID.to_owned(),
        },
    });
    let local = local_records(
        None,
        vec![post_descript("reply", reply, "2024-01-02T00:00:00.000Z")],
    );
    let munged = get_post_thread_munge(viewer, original, local, TEST_DID.to_owned()).unwrap();
    let ThreadViewPostEnum::ThreadViewPost(thread) = munged.thread else {
        panic!("expected thread view post");
    };
    assert_eq!(thread.post.uri, root_uri);
    let replies = thread.replies.expect("grafted replies");
    assert_eq!(replies.len(), 1);
    let ThreadViewPostEnum::ThreadViewPost(ref grafted) = **replies.first().unwrap() else {
        panic!("expected grafted thread view post");
    };
    assert_eq!(
        grafted.post.uri,
        format!("at://{TEST_DID}/app.bsky.feed.post/reply")
    );
    assert_eq!(grafted.post.author.did, TEST_DID);
}

#[tokio::test(flavor = "multi_thread")]
async fn post_thread_munge_passes_through_not_found_threads() {
    let (_dir, viewer) = test_viewer().await;
    let original = GetPostThreadOutput {
        thread: ThreadViewPostEnum::NotFoundPost(NotFoundPost {
            uri: format!("at://{TEST_DID}/app.bsky.feed.post/missing"),
            not_found: true,
        }),
    };
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
