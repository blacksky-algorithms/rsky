use chrono::{DateTime, Utc};

#[derive(Debug, Serialize, Deserialize)]
pub struct GetPreferencesOutput {
    pub preferences: Vec<RefPreferences>,
}

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

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "$type")]
pub enum RefPreferences {
    #[serde(rename = "app.bsky.actor.defs#adultContentPref")]
    AdultContentPref(AdultContentPref),
    #[serde(rename = "app.bsky.actor.defs#contentLabelPref")]
    ContentLabelPref(ContentLabelPref),
    #[serde(rename = "app.bsky.actor.defs#savedFeedsPref")]
    SavedFeedsPref(SavedFeedsPref),
    #[serde(rename = "app.bsky.actor.defs#savedFeedsPrefV2")]
    SavedFeedsPrefV2(SavedFeedsPrefV2),
    #[serde(rename = "app.bsky.actor.defs#personalDetailsPref")]
    PersonalDetailsPref(PersonalDetailsPref),
    #[serde(rename = "app.bsky.actor.defs#feedViewPref")]
    FeedViewPref(FeedViewPref),
    #[serde(rename = "app.bsky.actor.defs#threadViewPref")]
    ThreadViewPref(ThreadViewPref),
    #[serde(rename = "app.bsky.actor.defs#interestsPref")]
    InterestsPref(InterestsPref),
    #[serde(rename = "app.bsky.actor.defs#mutedWordsPref")]
    MutedWordsPref(MutedWordsPref),
    #[serde(rename = "app.bsky.actor.defs#hiddenPostsPref")]
    HiddenPostsPref(HiddenPostsPref),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContentLabelPref {
    // Which labeler does this preference apply to? If undefined, applies globally.
    #[serde(rename = "labelerDid")]
    pub labeler_did: Option<String>,
    pub label: String,
    pub visibility: ContentLabelVisibility,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentLabelVisibility {
    Ignore,
    Show,
    Warn,
    Hide,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SavedFeed {
    pub id: String,
    #[serde(rename = "type")]
    pub r#type: SavedFeedType,
    pub value: String,
    pub pinned: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SavedFeedType {
    Feed,
    List,
    Timeline,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SavedFeedsPrefV2 {
    pub items: Vec<SavedFeed>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SavedFeedsPref {
    pub pinned: Vec<String>,
    pub saved: Vec<String>,
    #[serde(rename = "timelineIndex")]
    pub timeline_index: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PersonalDetailsPref {
    #[serde(rename = "birthDate")]
    pub birth_date: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FeedViewPref {
    // The URI of the feed, or an identifier which describes the feed.
    pub feed: String,
    // Hide replies in the feed.
    #[serde(rename = "hideReplies")]
    pub hide_replies: Option<bool>,
    // Hide replies in the feed if they are not by followed users.
    #[serde(rename = "hideRepliesByUnfollowed")]
    pub hide_replies_by_unfollowed: Option<bool>,
    // Hide replies in the feed if they do not have this number of likes.
    #[serde(rename = "hideRepliesByLikeCount")]
    pub hide_replies_by_like_count: Option<i64>,
    // Hide reposts in the feed.
    #[serde(rename = "hideReposts")]
    pub hide_reposts: Option<bool>,
    // Hide quote posts in the feed.
    #[serde(rename = "hideQuotePosts")]
    pub hide_quote_posts: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ThreadViewPref {
    // Sorting mode for threads.
    pub sort: Option<ThreadViewSort>,
    // Show followed users at the top of all replies.
    #[serde(rename = "prioritizeFollowedUsers")]
    pub prioritize_followed_users: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThreadViewSort {
    Oldest,
    Newest,
    #[serde(rename = "most-likes")]
    MostLikes,
    Random,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InterestsPref {
    // A list of tags which describe the account owner's interests gathered during onboarding.
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MutedWordTarget {
    Content,
    Tag,
}

/// A word that the account owner has muted.
#[derive(Debug, Serialize, Deserialize)]
pub struct MutedWord {
    // The muted word itself.
    pub value: String,
    // The intended targets of the muted word.
    pub targets: Vec<MutedWordTarget>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MutedWordsPref {
    // A list of words the account owner has muted.
    pub items: Vec<MutedWord>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HiddenPostsPref {
    // A list of URIs of posts the account owner has hidden.
    pub items: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdultContentPref {
    pub enabled: bool,
}
