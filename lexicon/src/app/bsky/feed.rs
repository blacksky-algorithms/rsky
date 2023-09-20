use super::{
    actor::ProfileView,
    embed::{External, Image},
};
use crate::com::atproto::repo::StrongRef;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct ImagesEmbed {
    pub images: Vec<Image>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Record {
    pub uri: String,
    pub cid: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RecordEmbed {
    pub record: Record,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "$type")]
pub enum Media {
    #[serde(rename(
        deserialize = "app.bsky.embed.images",
        serialize = "app.bsky.embed.images"
    ))]
    Images(ImagesEmbed),

    #[serde(rename(
        deserialize = "app.bsky.embed.external",
        serialize = "app.bsky.embed.external"
    ))]
    External(External),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RecordWithMedia {
    pub record: RecordEmbed,
    pub media: Media
}

// "app.bsky.embed.images",
// "app.bsky.embed.external",
// "app.bsky.embed.record",
// "app.bsky.embed.recordWithMedia"
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "$type")]
pub enum Embeds {
    #[serde(rename(
        deserialize = "app.bsky.embed.images",
        serialize = "app.bsky.embed.images"
    ))]
    Images(ImagesEmbed),

    #[serde(
        alias="app.bsky.embed.external", 
        alias="app.bsky.embed.external#main"
    )]
    External(External),

    #[serde(rename(
        deserialize = "app.bsky.embed.record",
        serialize = "app.bsky.embed.record"
    ))]
    Record(RecordEmbed),

    #[serde(rename(
        deserialize = "app.bsky.embed.recordWithMedia",
        serialize = "app.bsky.embed.recordWithMedia",
    ))]
    RecordWithMedia(RecordWithMedia),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Post {
    #[serde(rename(deserialize = "createdAt", serialize = "createdAt"))]
    pub created_at: DateTime<Utc>,
    #[serde(rename(deserialize = "$type", serialize = "$type"))]
    pub rust_type: Option<String>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub langs: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed: Option<Embeds>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply: Option<ReplyRef>,
}

#[derive(Debug, Deserialize)]
pub struct ProfileViewBasic {
    pub did: String,
    pub handle: String,
}

#[derive(Debug, Deserialize)]
pub struct PostView {
    pub uri: String,
    pub cid: String,
    pub author: ProfileViewBasic,
    pub record: Post,
    #[serde(rename(deserialize = "indexedAt"))]
    pub indexed_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct ReasonRepost {
    pub by: ProfileViewBasic,
    #[serde(rename(deserialize = "indexedAt"))]
    pub indexed_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct FeedViewPost {
    pub post: PostView,
    pub reason: Option<ReasonRepost>,
}

#[derive(Debug, Deserialize)]
pub struct AuthorFeed {
    pub cursor: Option<String>,
    pub feed: Vec<FeedViewPost>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Like {
    #[serde(rename(deserialize = "$type", serialize = "$type"))]
    pub rust_type: Option<String>,
    #[serde(rename(deserialize = "createdAt"))]
    #[serde(rename(serialize = "createdAt"))]
    pub created_at: String,
    pub subject: StrongRef,
}

///like from app.bsky.feed.getLikes
#[derive(Debug, Serialize, Deserialize)]
pub struct GetLikesLike {
    #[serde(rename(deserialize = "createdAt"))]
    #[serde(rename(serialize = "createdAt"))]
    pub created_at: DateTime<Utc>,
    #[serde(rename(deserialize = "indexedAt"))]
    #[serde(rename(serialize = "indexedAt"))]
    pub indexed_at: DateTime<Utc>,
    pub actor: ProfileView,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Repost {
    #[serde(rename(deserialize = "createdAt"))]
    #[serde(rename(serialize = "createdAt"))]
    pub created_at: DateTime<Utc>,
    pub subject: StrongRef,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReplyRef {
    pub root: StrongRef,
    pub parent: StrongRef,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetLikes {
    pub uri: String,
    pub cid: Option<String>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct GetLikesOutput {
    pub uri: String,
    pub cid: Option<String>,
    pub likes: Vec<GetLikesLike>,
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ThreadViewPost {
    pub post: PostView,
}

#[derive(Debug, Deserialize)]
pub struct NotFoundPost {
    pub uri: String,
    #[serde(rename(deserialize = "notFound"))]
    pub not_found: bool,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "$type")]
pub enum ThreadViewPostEnum {
    #[serde(rename(deserialize = "app.bsky.feed.defs#threadViewPost"))]
    ThreadViewPost(ThreadViewPost),
    #[serde(rename(deserialize = "app.bsky.feed.defs#notFoundPost"))]
    NotFoundPost(NotFoundPost),
}

///api.bsky.feed.getPostThread
#[derive(Debug, Serialize)]
pub struct GetPostThread {
    pub uri: String,
    pub depth: Option<usize>,
}
#[derive(Debug, Deserialize)]
pub struct GetPostThreadOutput {
    pub thread: ThreadViewPostEnum,
}
