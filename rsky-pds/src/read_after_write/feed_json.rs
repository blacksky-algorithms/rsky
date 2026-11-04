//! Read-after-write edits applied to the appview's JSON as it arrived.
//!
//! The appview answers with views the typed lexicon structs do not fully
//! describe (viewer state, repost reasons, counters added since they were
//! written), so round-tripping a page through those structs drops fields the
//! client needs. These helpers only touch the keys the PDS actually changes
//! and leave every other key as the appview sent it.

use crate::read_after_write::types::RecordDescript;
use rsky_lexicon::app::bsky::feed::Post;
use serde_json::{Map, Value};

pub const REASON_PIN: &str = "app.bsky.feed.defs#reasonPin";
pub const REASON_REPOST: &str = "app.bsky.feed.defs#reasonRepost";
pub const THREAD_VIEW_POST: &str = "app.bsky.feed.defs#threadViewPost";

pub fn str_at<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

fn is_pinned(item: &Value) -> bool {
    str_at(item, "/reason/$type") == Some(REASON_PIN)
}

/// The account a feed item is attributed to: the reposter for a repost,
/// otherwise the post author (a pin is the author's own post).
fn item_actor(item: &Value) -> Option<&str> {
    match str_at(item, "/reason/$type") {
        Some(REASON_REPOST) => str_at(item, "/reason/by/did"),
        _ => str_at(item, "/post/author/did"),
    }
}

pub fn is_users_feed(feed: &[Value], requester: &str) -> bool {
    feed.first().and_then(item_actor) == Some(requester)
}

/// Insert a freshly formatted item ahead of the first older one. The appview
/// pins a `reasonPin` item at position 0 regardless of age, so a leading pin
/// stays first.
pub fn insert_feed_item(feed: &mut Vec<Value>, item: Value) {
    let start = usize::from(feed.first().is_some_and(is_pinned));
    let indexed_at = str_at(&item, "/post/indexedAt").unwrap_or_default();
    let idx = feed
        .iter()
        .enumerate()
        .skip(start)
        .find(|(_, existing)| str_at(existing, "/post/indexedAt").unwrap_or_default() < indexed_at)
        .map(|(idx, _)| idx);
    match idx {
        None => feed.push(item),
        Some(idx) => feed.insert(idx, item),
    }
}

pub fn overlay_author(item: &mut Value, display_name: Option<&str>, avatar: Option<&str>) {
    if let Some(author) = item
        .pointer_mut("/post/author")
        .and_then(Value::as_object_mut)
    {
        set_or_remove(author, "displayName", display_name);
        set_or_remove(author, "avatar", avatar);
    }
}

fn set_or_remove(object: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    match value {
        Some(value) => {
            object.insert(key.to_owned(), Value::String(value.to_owned()));
        }
        None => {
            object.remove(key);
        }
    }
}

pub fn is_thread_view_post(node: &Value) -> bool {
    str_at(node, "/$type") == Some(THREAD_VIEW_POST)
}

/// Local replies that belong to this thread, each with the uri of the post
/// it answers: replies rooted at the anchor, or sharing the anchor's own root
/// when the anchor is itself a reply.
pub fn find_posts_in_thread(
    thread: &Value,
    posts: Vec<RecordDescript<Post>>,
) -> Vec<(String, RecordDescript<Post>)> {
    let anchor = str_at(thread, "/post/uri");
    let root = str_at(thread, "/post/record/reply/root/uri");
    posts
        .into_iter()
        .filter_map(|post| {
            let reply = post.record.reply.as_ref()?;
            let reply_root = Some(reply.root.uri.as_str());
            if reply_root != anchor && reply_root != root {
                return None;
            }
            let parent_uri = reply.parent.uri.clone();
            Some((parent_uri, post))
        })
        .collect()
}

/// Prepend `node` to the replies of every node whose post is `parent_uri`,
/// so a just-written reply shows ahead of the siblings the appview knew.
pub fn graft_reply(thread: &mut Value, parent_uri: &str, node: &Value) {
    if str_at(thread, "/post/uri") == Some(parent_uri) {
        if let Some(object) = thread.as_object_mut() {
            let mut replies = vec![node.clone()];
            replies.extend(
                object
                    .get("replies")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
            object.insert("replies".to_owned(), Value::Array(replies));
        }
        return;
    }
    if let Some(replies) = thread.get_mut("replies").and_then(Value::as_array_mut) {
        for reply in replies
            .iter_mut()
            .filter(|reply| is_thread_view_post(reply))
        {
            graft_reply(reply, parent_uri, node);
        }
    }
}

/// The uri of the first parent the thread does not already contain.
pub fn highest_parent(thread: &Value) -> Option<String> {
    match thread.get("parent") {
        Some(parent) if is_thread_view_post(parent) => highest_parent(parent),
        Some(parent) if !parent.is_null() => None,
        _ => str_at(thread, "/post/record/reply/parent/uri").map(str::to_owned),
    }
}
