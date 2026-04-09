use crate::app::bsky::graph::{ListViewBasic, StarterPackViewBasic};
use crate::com::atproto::label::{Label, SelfLabels};
use crate::com::atproto::repo::{Blob, StrongRef};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct GetPreferencesOutput {
    pub preferences: Vec<RefPreferences>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PutPreferencesInput {
    pub preferences: Vec<RefPreferences>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
#[serde(rename = "app.bsky.actor.profile")]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Small image to be displayed next to posts from account. AKA, 'profile picture'
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<Blob>,
    /// Larger horizontal image to display behind profile view.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banner: Option<Blob>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<ProfileLabels>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub joined_via_starter_pack: Option<StrongRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
pub enum ProfileLabels {
    #[serde(rename = "com.atproto.label.defs#selfLabels")]
    SelfLabels(SelfLabels),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileViewBasic {
    pub did: String,
    pub handle: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub associated: Option<RefProfileAssociated>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewer: Option<ViewerState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<Label>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    pub did: String,
    pub handle: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    pub labels: Vec<Label>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileViewDetailed {
    pub did: String,
    pub handle: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub followers_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follows_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub posts_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub associated: Option<RefProfileAssociated>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub joined_via_starter_pack: Option<StarterPackViewBasic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewer: Option<ViewerState>,
    pub labels: Vec<Label>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetProfilesOutput {
    pub profiles: Vec<ProfileViewDetailed>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefProfileAssociated {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lists: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedgens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starter_packs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labeler: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat: Option<RefProfileAssociatedChat>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefProfileAssociatedChat {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_incoming: Option<AssociatedChatAllowIncoming>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AssociatedChatAllowIncoming {
    All,
    None,
    Following,
}

/// Metadata about the requesting account's relationship with the subject account.
/// Only has meaningful content for authed requests.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewerState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted_by_list: Option<ListViewBasic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_by: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking_by_list: Option<ListViewBasic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub following: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub followed_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub known_followers: Option<KnownFollowers>,
}

/// The subject's followers whom you also follow
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct KnownFollowers {
    pub count: usize,
    pub followers: Vec<ProfileViewBasic>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
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
    #[serde(rename = "app.bsky.actor.defs#bskyAppStatePref")]
    BskyAppStatePref(BskyAppStatePref),
    #[serde(rename = "app.bsky.actor.defs#labelersPref")]
    LabelersPref(LabelersPref),
}

impl RefPreferences {
    pub fn get_type(&self) -> String {
        let r#type = match self {
            RefPreferences::AdultContentPref(_) => "app.bsky.actor.defs#adultContentPref",
            RefPreferences::ContentLabelPref(_) => "app.bsky.actor.defs#contentLabelPref",
            RefPreferences::SavedFeedsPref(_) => "app.bsky.actor.defs#savedFeedsPref",
            RefPreferences::SavedFeedsPrefV2(_) => "app.bsky.actor.defs#savedFeedsPrefV2",
            RefPreferences::PersonalDetailsPref(_) => "app.bsky.actor.defs#personalDetailsPref",
            RefPreferences::FeedViewPref(_) => "app.bsky.actor.defs#feedViewPref",
            RefPreferences::ThreadViewPref(_) => "app.bsky.actor.defs#threadViewPref",
            RefPreferences::InterestsPref(_) => "app.bsky.actor.defs#interestsPref",
            RefPreferences::MutedWordsPref(_) => "app.bsky.actor.defs#mutedWordsPref",
            RefPreferences::HiddenPostsPref(_) => "app.bsky.actor.defs#hiddenPostsPref",
            RefPreferences::BskyAppStatePref(_) => "app.bsky.actor.defs#bskyAppStatePref",
            RefPreferences::LabelersPref(_) => "app.bsky.actor.defs#labelersPref",
        };
        r#type.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentLabelPref {
    // Which labeler does this preference apply to? If undefined, applies globally.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labeler_did: Option<String>,
    pub label: String,
    pub visibility: ContentLabelVisibility,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentLabelVisibility {
    Ignore,
    Show,
    Warn,
    Hide,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SavedFeed {
    pub id: String,
    #[serde(rename = "type")]
    pub r#type: SavedFeedType,
    pub value: String,
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SavedFeedType {
    Feed,
    List,
    Timeline,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SavedFeedsPrefV2 {
    pub items: Vec<SavedFeed>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedFeedsPref {
    pub pinned: Vec<String>,
    pub saved: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeline_index: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalDetailsPref {
    pub birth_date: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedViewPref {
    // The URI of the feed, or an identifier which describes the feed.
    pub feed: String,
    // Hide replies in the feed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hide_replies: Option<bool>,
    // Hide replies in the feed if they are not by followed users.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hide_replies_by_unfollowed: Option<bool>,
    // Hide replies in the feed if they do not have this number of likes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hide_replies_by_like_count: Option<i64>,
    // Hide reposts in the feed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hide_reposts: Option<bool>,
    // Hide quote posts in the feed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hide_quote_posts: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadViewPref {
    // Sorting mode for threads.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<ThreadViewSort>,
    // Show followed users at the top of all replies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prioritize_followed_users: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ThreadViewSort {
    Oldest,
    Newest,
    #[serde(rename = "most-likes")]
    MostLikes,
    Random,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct InterestsPref {
    // A list of tags which describe the account owner's interests gathered during onboarding.
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MutedWordTarget {
    Content,
    Tag,
}

/// A word that the account owner has muted.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct MutedWord {
    // The muted word itself.
    pub value: String,
    // The intended targets of the muted word.
    pub targets: Vec<MutedWordTarget>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct MutedWordsPref {
    // A list of words the account owner has muted.
    pub items: Vec<MutedWord>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct HiddenPostsPref {
    // A list of URIs of posts the account owner has hidden.
    pub items: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct AdultContentPref {
    pub enabled: bool,
}

/// A grab bag of state that's specific to the bsky.app program.
/// Third-party apps shouldn't use this.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BskyAppStatePref {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_progress_guide: Option<BskyAppProgressGuide>,
    // An array of tokens which identify nudges (modals, popups, tours, highlight dots)
    // that should be shown to the user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued_nudges: Option<Vec<String>>,
}

/// If set, an active progress guide. Once completed, can be set to undefined.
/// Should have unspecced fields tracking progress.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct BskyAppProgressGuide {
    pub guide: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct LabelersPref {
    pub labelers: Vec<LabelersPrefItem>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct LabelersPrefItem {
    pub did: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_view_basic_omits_none_fields() {
        let view = ProfileViewBasic {
            did: "did:plc:abc".to_string(),
            handle: "alice.bsky.social".to_string(),
            display_name: None,
            avatar: None,
            associated: None,
            viewer: None,
            labels: None,
            created_at: None,
        };
        let json = serde_json::to_value(&view).unwrap();
        assert!(!json.as_object().unwrap().contains_key("displayName"));
        assert!(!json.as_object().unwrap().contains_key("avatar"));
        assert!(!json.as_object().unwrap().contains_key("associated"));
        assert!(!json.as_object().unwrap().contains_key("viewer"));
        assert!(!json.as_object().unwrap().contains_key("labels"));
        assert!(!json.as_object().unwrap().contains_key("createdAt"));
        assert_eq!(json["did"], "did:plc:abc");
        assert_eq!(json["handle"], "alice.bsky.social");
    }

    #[test]
    fn profile_view_detailed_omits_none_fields() {
        let view = ProfileViewDetailed {
            did: "did:plc:abc".to_string(),
            handle: "alice.bsky.social".to_string(),
            display_name: None,
            description: None,
            avatar: None,
            banner: None,
            followers_count: None,
            follows_count: None,
            posts_count: None,
            associated: None,
            joined_via_starter_pack: None,
            viewer: None,
            labels: vec![],
            indexed_at: None,
            created_at: None,
        };
        let json = serde_json::to_value(&view).unwrap();
        let obj = json.as_object().unwrap();
        for key in &[
            "displayName",
            "description",
            "avatar",
            "banner",
            "followersCount",
            "followsCount",
            "postsCount",
            "associated",
            "joinedViaStarterPack",
            "viewer",
            "indexedAt",
            "createdAt",
        ] {
            assert!(!obj.contains_key(*key), "expected key `{key}` to be absent");
        }
    }

    #[test]
    fn viewer_state_omits_none_fields() {
        let viewer = ViewerState {
            muted: None,
            muted_by_list: None,
            blocked_by: None,
            blocking_by_list: None,
            following: None,
            followed_by: None,
            known_followers: None,
        };
        let json = serde_json::to_value(&viewer).unwrap();
        let obj = json.as_object().unwrap();
        assert!(obj.is_empty(), "expected empty object, got: {obj:?}");
    }

    #[test]
    fn ref_profile_associated_omits_none_fields() {
        let associated = RefProfileAssociated {
            lists: None,
            feedgens: None,
            starter_packs: None,
            labeler: None,
            chat: None,
        };
        let json = serde_json::to_value(&associated).unwrap();
        let obj = json.as_object().unwrap();
        assert!(obj.is_empty(), "expected empty object, got: {obj:?}");
    }

    #[test]
    fn profile_view_basic_includes_present_fields() {
        let view = ProfileViewBasic {
            did: "did:plc:abc".to_string(),
            handle: "alice.bsky.social".to_string(),
            display_name: Some("Alice".to_string()),
            avatar: Some("https://cdn.example.com/avatar.jpg".to_string()),
            associated: None,
            viewer: None,
            labels: None,
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["displayName"], "Alice");
        assert_eq!(json["avatar"], "https://cdn.example.com/avatar.jpg");
        assert_eq!(json["createdAt"], "2024-01-01T00:00:00Z");
        assert!(!json.as_object().unwrap().contains_key("associated"));
        assert!(!json.as_object().unwrap().contains_key("viewer"));
    }
}
