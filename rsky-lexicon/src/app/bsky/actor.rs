use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct Label {
    pub src: String,
    pub uri: String,
    pub val: String,
    pub neg: bool,
    pub cts: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProfileViewBasic {
    pub did: String,
    pub handle: String,
    #[serde(rename(deserialize = "displayName"))]
    pub display_name: Option<String>,
    pub avatar: Option<String>,
    pub labels: Vec<Label>,
    pub indexed_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProfileView {
    pub did: String,
    pub handle: String,
    #[serde(rename(deserialize = "displayName"))]
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub avatar: Option<String>,
    pub labels: Vec<Label>,
    pub indexed_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProfileViewDetailed {
    pub did: String,
    pub handle: String,
    #[serde(rename(deserialize = "displayName"))]
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub avatar: Option<String>,
    pub banner: Option<String>,
    #[serde(rename(deserialize = "followersCount"))]
    pub followers_count: Option<usize>,
    #[serde(rename(deserialize = "followsCount"))]
    pub follows_count: Option<usize>,
    #[serde(rename(deserialize = "postsCount"))]
    pub posts_count: Option<usize>,
    pub labels: Vec<Label>,
    pub indexed_at: Option<String>,
}
