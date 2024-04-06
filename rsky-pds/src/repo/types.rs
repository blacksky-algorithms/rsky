use crate::repo::blob_refs::BlobRef;
use crate::repo::block_map::BlockMap;
use crate::repo::cid_set::CidSet;
use crate::storage::Ipld;
use libipld::Cid;
use std::collections::BTreeMap;

// Repo nodes
// ---------------

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct UnsignedCommit {
    pub did: String,
    pub version: u8, // Should be 3
    pub data: Cid,
    pub rev: String,
    // `prev` added for backwards compatibility with v2, no requirement of keeping around history
    pub prev: Option<Cid>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Commit {
    pub did: String,
    pub version: u8, // Should be 3
    pub data: Cid,
    pub rev: String,
    pub prev: Option<Cid>,
    pub sig: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct LegacyV2Commit {
    pub did: String,
    pub version: u8, // Should be 2
    pub data: Cid,
    pub rev: Option<String>,
    pub prev: Option<Cid>,
    pub sig: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum VersionedCommit {
    Commit(Commit),
    LegacyV2Commit(LegacyV2Commit),
}

impl VersionedCommit {
    pub fn data(&self) -> Cid {
        match self {
            VersionedCommit::Commit(c) => c.data,
            VersionedCommit::LegacyV2Commit(c) => c.data,
        }
    }

    pub fn did(&self) -> &String {
        match self {
            VersionedCommit::Commit(c) => &c.did,
            VersionedCommit::LegacyV2Commit(c) => &c.did,
        }
    }

    pub fn version(&self) -> u8 {
        match self {
            VersionedCommit::Commit(c) => c.version,
            VersionedCommit::LegacyV2Commit(c) => c.version,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum Lex {
    Ipld(Ipld),
    Blob(BlobRef),
    List(Vec<Lex>),
    Map(BTreeMap<String, Lex>),
}

// Repo Operations
// ---------------

pub type RepoRecord = BTreeMap<String, Lex>;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct BlobConstraint {
    pub max_size: Option<usize>,
    pub accept: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PreparedBlobRef {
    pub cid: Cid,
    pub mime_type: String,
    pub contraints: BlobConstraint,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PreparedCreateOrUpdate {
    pub action: WriteOpAction,
    pub uri: String,
    pub cid: Cid,
    pub swap_cid: Option<Cid>,
    pub record: RepoRecord,
    pub blobs: Vec<PreparedBlobRef>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PreparedDelete {
    pub action: WriteOpAction,
    pub uri: String,
    pub swap_cid: Option<Cid>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum PreparedWrite {
    Create(PreparedCreateOrUpdate),
    Update(PreparedCreateOrUpdate),
    Delete(PreparedDelete),
}

impl PreparedWrite {
    pub fn uri(&self) -> &String {
        match self {
            PreparedWrite::Create(w) => &w.uri,
            PreparedWrite::Update(w) => &w.uri,
            PreparedWrite::Delete(w) => &w.uri,
        }
    }

    pub fn swap_cid(&self) -> &Option<Cid> {
        match self {
            PreparedWrite::Create(w) => &w.swap_cid,
            PreparedWrite::Update(w) => &w.swap_cid,
            PreparedWrite::Delete(w) => &w.swap_cid,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum WriteOpAction {
    Create,
    Update,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RecordCreateOrUpdateOp {
    pub action: WriteOpAction,
    pub collection: String,
    pub rkey: String,
    pub record: RepoRecord,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RecordDeleteOp {
    pub action: WriteOpAction,
    pub collection: String,
    pub rkey: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum RecordWriteOp {
    Create(RecordCreateOrUpdateOp),
    Update(RecordCreateOrUpdateOp),
    Delete(RecordDeleteOp),
}

// @TODO: Use AtUri
pub fn create_write_to_op(write: PreparedCreateOrUpdate) -> RecordWriteOp {
    let uri_without_prefix = write.uri.replace("at://", "");
    let parts = uri_without_prefix.split("/").collect::<Vec<&str>>();
    RecordWriteOp::Create {
        0: RecordCreateOrUpdateOp {
            action: WriteOpAction::Create,
            collection: parts[1].to_string(),
            rkey: parts[2].to_string(),
            record: write.record,
        },
    }
}

// @TODO: Use AtUri
pub fn update_write_to_op(write: PreparedCreateOrUpdate) -> RecordWriteOp {
    let uri_without_prefix = write.uri.replace("at://", "");
    let parts = uri_without_prefix.split("/").collect::<Vec<&str>>();
    RecordWriteOp::Create {
        0: RecordCreateOrUpdateOp {
            action: WriteOpAction::Update,
            collection: parts[1].to_string(),
            rkey: parts[2].to_string(),
            record: write.record,
        },
    }
}

// @TODO: Use AtUri
pub fn delete_write_to_op(write: PreparedDelete) -> RecordWriteOp {
    let uri_without_prefix = write.uri.replace("at://", "");
    let parts = uri_without_prefix.split("/").collect::<Vec<&str>>();
    RecordWriteOp::Delete {
        0: RecordDeleteOp {
            action: WriteOpAction::Delete,
            collection: parts[1].to_string(),
            rkey: parts[2].to_string(),
        },
    }
}

pub fn write_to_op(write: PreparedWrite) -> RecordWriteOp {
    match write {
        PreparedWrite::Create(c) => create_write_to_op(c),
        PreparedWrite::Update(u) => update_write_to_op(u),
        PreparedWrite::Delete(d) => delete_write_to_op(d),
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum RecordWriteEnum {
    List(Vec<RecordWriteOp>),
    Single(RecordWriteOp),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RecordCreateOrDeleteDescript {
    pub action: WriteOpAction,
    pub collection: String,
    pub rkey: String,
    pub cid: Cid,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RecordUpdateDescript {
    pub action: WriteOpAction,
    pub collection: String,
    pub rkey: String,
    pub prev: Cid,
    pub cid: Cid,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum RecordWriteDescript {
    Create(RecordCreateOrDeleteDescript),
    Update(RecordUpdateDescript),
    Delete(RecordCreateOrDeleteDescript),
}

pub type WriteLog = Vec<Vec<RecordWriteDescript>>;

// Updates/Commits
// ---------------

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct CommitData {
    pub cid: Cid,
    pub rev: String,
    pub since: Option<String>,
    pub prev: Option<Cid>,
    pub new_blocks: BlockMap,
    pub removed_cids: CidSet,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RepoUpdate {
    pub cid: Cid,
    pub rev: String,
    pub since: Option<String>,
    pub prev: Option<Cid>,
    pub new_blocks: BlockMap,
    pub removed_cids: CidSet,
    pub ops: Vec<RecordWriteOp>,
}

pub type CollectionContents = BTreeMap<String, RepoRecord>;
pub type RepoContents = BTreeMap<String, CollectionContents>;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RepoRecordWithCid {
    pub cid: Cid,
    pub value: RepoRecord,
}
pub type CollectionContentsWithCids = BTreeMap<String, RepoRecordWithCid>;
pub type RepoContentsWithCids = BTreeMap<String, CollectionContentsWithCids>;

pub type DatastoreContents = BTreeMap<String, Cid>;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RecordPath {
    pub collection: String,
    pub rkey: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RecordClaim {
    pub collection: String,
    pub rkey: String,
    pub record: Option<RepoRecord>,
}

// Sync
// ---------------

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct VerifiedDiff {
    pub write: Vec<RecordWriteDescript>,
    pub commit: CommitData,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct VerifiedRepo {
    pub creates: Vec<RecordCreateOrDeleteDescript>,
    pub commit: CommitData,
}

pub type CarBlock = CidAndBytes;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct StatusAttr {
    pub applied: bool,
    pub r#ref: Option<String>,
}

pub struct CidAndBytes {
    pub cid: Cid,
    pub bytes: Vec<u8>,
}

pub type BlockWriter = Vec<CidAndBytes>;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum Ids {
    ComAtprotoAdminDefs,
    ComAtprotoAdminDeleteAccount,
    ComAtprotoAdminDisableAccountInvites,
    ComAtprotoAdminDisableInviteCodes,
    ComAtprotoAdminEnableAccountInvites,
    ComAtprotoAdminGetAccountInfo,
    ComAtprotoAdminGetAccountInfos,
    ComAtprotoAdminGetInviteCodes,
    ComAtprotoAdminGetSubjectStatus,
    ComAtprotoAdminSendEmail,
    ComAtprotoAdminUpdateAccountEmail,
    ComAtprotoAdminUpdateAccountHandle,
    ComAtprotoAdminUpdateAccountPassword,
    ComAtprotoAdminUpdateSubjectStatus,
    ComAtprotoIdentityGetRecommendedDidCredentials,
    ComAtprotoIdentityRequestPlcOperationSignature,
    ComAtprotoIdentityResolveHandle,
    ComAtprotoIdentitySignPlcOperation,
    ComAtprotoIdentitySubmitPlcOperation,
    ComAtprotoIdentityUpdateHandle,
    ComAtprotoLabelDefs,
    ComAtprotoLabelQueryLabels,
    ComAtprotoLabelSubscribeLabels,
    ComAtprotoModerationCreateReport,
    ComAtprotoModerationDefs,
    ComAtprotoRepoApplyWrites,
    ComAtprotoRepoCreateRecord,
    ComAtprotoRepoDeleteRecord,
    ComAtprotoRepoDescribeRepo,
    ComAtprotoRepoGetRecord,
    ComAtprotoRepoImportRepo,
    ComAtprotoRepoListMissingBlobs,
    ComAtprotoRepoListRecords,
    ComAtprotoRepoPutRecord,
    ComAtprotoRepoStrongRef,
    ComAtprotoRepoUploadBlob,
    ComAtprotoServerActivateAccount,
    ComAtprotoServerCheckAccountStatus,
    ComAtprotoServerConfirmEmail,
    ComAtprotoServerCreateAccount,
    ComAtprotoServerCreateAppPassword,
    ComAtprotoServerCreateInviteCode,
    ComAtprotoServerCreateInviteCodes,
    ComAtprotoServerCreateSession,
    ComAtprotoServerDeactivateAccount,
    ComAtprotoServerDefs,
    ComAtprotoServerDeleteAccount,
    ComAtprotoServerDeleteSession,
    ComAtprotoServerDescribeServer,
    ComAtprotoServerGetAccountInviteCodes,
    ComAtprotoServerGetServiceAuth,
    ComAtprotoServerGetSession,
    ComAtprotoServerListAppPasswords,
    ComAtprotoServerRefreshSession,
    ComAtprotoServerRequestAccountDelete,
    ComAtprotoServerRequestEmailConfirmation,
    ComAtprotoServerRequestEmailUpdate,
    ComAtprotoServerRequestPasswordReset,
    ComAtprotoServerReserveSigningKey,
    ComAtprotoServerResetPassword,
    ComAtprotoServerRevokeAppPassword,
    ComAtprotoServerUpdateEmail,
    ComAtprotoSyncGetBlob,
    ComAtprotoSyncGetBlocks,
    ComAtprotoSyncGetCheckout,
    ComAtprotoSyncGetHead,
    ComAtprotoSyncGetLatestCommit,
    ComAtprotoSyncGetRecord,
    ComAtprotoSyncGetRepo,
    ComAtprotoSyncListBlobs,
    ComAtprotoSyncListRepos,
    ComAtprotoSyncNotifyOfUpdate,
    ComAtprotoSyncRequestCrawl,
    ComAtprotoSyncSubscribeRepos,
    ComAtprotoTempCheckSignupQueue,
    ComAtprotoTempFetchLabels,
    ComAtprotoTempRequestPhoneVerification,
    AppBskyActorDefs,
    AppBskyActorGetPreferences,
    AppBskyActorGetProfile,
    AppBskyActorGetProfiles,
    AppBskyActorGetSuggestions,
    AppBskyActorProfile,
    AppBskyActorPutPreferences,
    AppBskyActorSearchActors,
    AppBskyActorSearchActorsTypeahead,
    AppBskyEmbedExternal,
    AppBskyEmbedImages,
    AppBskyEmbedRecord,
    AppBskyEmbedRecordWithMedia,
    AppBskyFeedDefs,
    AppBskyFeedDescribeFeedGenerator,
    AppBskyFeedGenerator,
    AppBskyFeedGetActorFeeds,
    AppBskyFeedGetActorLikes,
    AppBskyFeedGetAuthorFeed,
    AppBskyFeedGetFeed,
    AppBskyFeedGetFeedGenerator,
    AppBskyFeedGetFeedGenerators,
    AppBskyFeedGetFeedSkeleton,
    AppBskyFeedGetLikes,
    AppBskyFeedGetListFeed,
    AppBskyFeedGetPostThread,
    AppBskyFeedGetPosts,
    AppBskyFeedGetRepostedBy,
    AppBskyFeedGetSuggestedFeeds,
    AppBskyFeedGetTimeline,
    AppBskyFeedLike,
    AppBskyFeedPost,
    AppBskyFeedRepost,
    AppBskyFeedSearchPosts,
    AppBskyFeedThreadgate,
    AppBskyGraphBlock,
    AppBskyGraphDefs,
    AppBskyGraphFollow,
    AppBskyGraphGetBlocks,
    AppBskyGraphGetFollowers,
    AppBskyGraphGetFollows,
    AppBskyGraphGetList,
    AppBskyGraphGetListBlocks,
    AppBskyGraphGetListMutes,
    AppBskyGraphGetLists,
    AppBskyGraphGetMutes,
    AppBskyGraphGetRelationships,
    AppBskyGraphGetSuggestedFollowsByActor,
    AppBskyGraphList,
    AppBskyGraphListblock,
    AppBskyGraphListitem,
    AppBskyGraphMuteActor,
    AppBskyGraphMuteActorList,
    AppBskyGraphUnmuteActor,
    AppBskyGraphUnmuteActorList,
    AppBskyLabelerDefs,
    AppBskyLabelerGetServices,
    AppBskyLabelerService,
    AppBskyNotificationGetUnreadCount,
    AppBskyNotificationListNotifications,
    AppBskyNotificationRegisterPush,
    AppBskyNotificationUpdateSeen,
    AppBskyRichtextFacet,
    AppBskyUnspeccedDefs,
    AppBskyUnspeccedGetPopularFeedGenerators,
    AppBskyUnspeccedGetTaggedSuggestions,
    AppBskyUnspeccedSearchActorsSkeleton,
    AppBskyUnspeccedSearchPostsSkeleton,
    ToolsOzoneCommunicationCreateTemplate,
    ToolsOzoneCommunicationDefs,
    ToolsOzoneCommunicationDeleteTemplate,
    ToolsOzoneCommunicationListTemplates,
    ToolsOzoneCommunicationUpdateTemplate,
    ToolsOzoneModerationDefs,
    ToolsOzoneModerationEmitEvent,
    ToolsOzoneModerationGetEvent,
    ToolsOzoneModerationGetRecord,
    ToolsOzoneModerationGetRepo,
    ToolsOzoneModerationQueryEvents,
    ToolsOzoneModerationQueryStatuses,
    ToolsOzoneModerationSearchRepos,
}

impl Ids {
    pub fn as_str(&self) -> &'static str {
        match self {
            Ids::ComAtprotoAdminDefs => "com.atproto.admin.defs",
            Ids::ComAtprotoAdminDeleteAccount => "com.atproto.admin.deleteAccount",
            Ids::ComAtprotoAdminDisableAccountInvites => "com.atproto.admin.disableAccountInvites",
            Ids::ComAtprotoAdminDisableInviteCodes => "com.atproto.admin.disableInviteCodes",
            Ids::ComAtprotoAdminEnableAccountInvites => "com.atproto.admin.enableAccountInvites",
            Ids::ComAtprotoAdminGetAccountInfo => "com.atproto.admin.getAccountInfo",
            Ids::ComAtprotoAdminGetAccountInfos => "com.atproto.admin.getAccountInfos",
            Ids::ComAtprotoAdminGetInviteCodes => "com.atproto.admin.getInviteCodes",
            Ids::ComAtprotoAdminGetSubjectStatus => "com.atproto.admin.getSubjectStatus",
            Ids::ComAtprotoAdminSendEmail => "com.atproto.admin.sendEmail",
            Ids::ComAtprotoAdminUpdateAccountEmail => "com.atproto.admin.updateAccountEmail",
            Ids::ComAtprotoAdminUpdateAccountHandle => "com.atproto.admin.updateAccountHandle",
            Ids::ComAtprotoAdminUpdateAccountPassword => "com.atproto.admin.updateAccountPassword",
            Ids::ComAtprotoAdminUpdateSubjectStatus => "com.atproto.admin.updateSubjectStatus",
            Ids::ComAtprotoIdentityGetRecommendedDidCredentials => {
                "com.atproto.identity.getRecommendedDidCredentials"
            }
            Ids::ComAtprotoIdentityRequestPlcOperationSignature => {
                "com.atproto.identity.requestPlcOperationSignature"
            }
            Ids::ComAtprotoIdentityResolveHandle => "com.atproto.identity.resolveHandle",
            Ids::ComAtprotoIdentitySignPlcOperation => "com.atproto.identity.signPlcOperation",
            Ids::ComAtprotoIdentitySubmitPlcOperation => "com.atproto.identity.submitPlcOperation",
            Ids::ComAtprotoIdentityUpdateHandle => "com.atproto.identity.updateHandle",
            Ids::ComAtprotoLabelDefs => "com.atproto.label.defs",
            Ids::ComAtprotoLabelQueryLabels => "com.atproto.label.queryLabels",
            Ids::ComAtprotoLabelSubscribeLabels => "com.atproto.label.subscribeLabels",
            Ids::ComAtprotoModerationCreateReport => "com.atproto.moderation.createReport",
            Ids::ComAtprotoModerationDefs => "com.atproto.moderation.defs",
            Ids::ComAtprotoRepoApplyWrites => "com.atproto.repo.applyWrites",
            Ids::ComAtprotoRepoCreateRecord => "com.atproto.repo.createRecord",
            Ids::ComAtprotoRepoDeleteRecord => "com.atproto.repo.deleteRecord",
            Ids::ComAtprotoRepoDescribeRepo => "com.atproto.repo.describeRepo",
            Ids::ComAtprotoRepoGetRecord => "com.atproto.repo.getRecord",
            Ids::ComAtprotoRepoImportRepo => "com.atproto.repo.importRepo",
            Ids::ComAtprotoRepoListMissingBlobs => "com.atproto.repo.listMissingBlobs",
            Ids::ComAtprotoRepoListRecords => "com.atproto.repo.listRecords",
            Ids::ComAtprotoRepoPutRecord => "com.atproto.repo.putRecord",
            Ids::ComAtprotoRepoStrongRef => "com.atproto.repo.strongRef",
            Ids::ComAtprotoRepoUploadBlob => "com.atproto.repo.uploadBlob",
            Ids::ComAtprotoServerActivateAccount => "com.atproto.server.activateAccount",
            Ids::ComAtprotoServerCheckAccountStatus => "com.atproto.server.checkAccountStatus",
            Ids::ComAtprotoServerConfirmEmail => "com.atproto.server.confirmEmail",
            Ids::ComAtprotoServerCreateAccount => "com.atproto.server.createAccount",
            Ids::ComAtprotoServerCreateAppPassword => "com.atproto.server.createAppPassword",
            Ids::ComAtprotoServerCreateInviteCode => "com.atproto.server.createInviteCode",
            Ids::ComAtprotoServerCreateInviteCodes => "com.atproto.server.createInviteCodes",
            Ids::ComAtprotoServerCreateSession => "com.atproto.server.createSession",
            Ids::ComAtprotoServerDeactivateAccount => "com.atproto.server.deactivateAccount",
            Ids::ComAtprotoServerDefs => "com.atproto.server.defs",
            Ids::ComAtprotoServerDeleteAccount => "com.atproto.server.deleteAccount",
            Ids::ComAtprotoServerDeleteSession => "com.atproto.server.deleteSession",
            Ids::ComAtprotoServerDescribeServer => "com.atproto.server.describeServer",
            Ids::ComAtprotoServerGetAccountInviteCodes => {
                "com.atproto.server.getAccountInviteCodes"
            }
            Ids::ComAtprotoServerGetServiceAuth => "com.atproto.server.getServiceAuth",
            Ids::ComAtprotoServerGetSession => "com.atproto.server.getSession",
            Ids::ComAtprotoServerListAppPasswords => "com.atproto.server.listAppPasswords",
            Ids::ComAtprotoServerRefreshSession => "com.atproto.server.refreshSession",
            Ids::ComAtprotoServerRequestAccountDelete => "com.atproto.server.requestAccountDelete",
            Ids::ComAtprotoServerRequestEmailConfirmation => {
                "com.atproto.server.requestEmailConfirmation"
            }
            Ids::ComAtprotoServerRequestEmailUpdate => "com.atproto.server.requestEmailUpdate",
            Ids::ComAtprotoServerRequestPasswordReset => "com.atproto.server.requestPasswordReset",
            Ids::ComAtprotoServerReserveSigningKey => "com.atproto.server.reserveSigningKey",
            Ids::ComAtprotoServerResetPassword => "com.atproto.server.resetPassword",
            Ids::ComAtprotoServerRevokeAppPassword => "com.atproto.server.revokeAppPassword",
            Ids::ComAtprotoServerUpdateEmail => "com.atproto.server.updateEmail",
            Ids::ComAtprotoSyncGetBlob => "com.atproto.sync.getBlob",
            Ids::ComAtprotoSyncGetBlocks => "com.atproto.sync.getBlocks",
            Ids::ComAtprotoSyncGetCheckout => "com.atproto.sync.getCheckout",
            Ids::ComAtprotoSyncGetHead => "com.atproto.sync.getHead",
            Ids::ComAtprotoSyncGetLatestCommit => "com.atproto.sync.getLatestCommit",
            Ids::ComAtprotoSyncGetRecord => "com.atproto.sync.getRecord",
            Ids::ComAtprotoSyncGetRepo => "com.atproto.sync.getRepo",
            Ids::ComAtprotoSyncListBlobs => "com.atproto.sync.listBlobs",
            Ids::ComAtprotoSyncListRepos => "com.atproto.sync.listRepos",
            Ids::ComAtprotoSyncNotifyOfUpdate => "com.atproto.sync.notifyOfUpdate",
            Ids::ComAtprotoSyncRequestCrawl => "com.atproto.sync.requestCrawl",
            Ids::ComAtprotoSyncSubscribeRepos => "com.atproto.sync.subscribeRepos",
            Ids::ComAtprotoTempCheckSignupQueue => "com.atproto.temp.checkSignupQueue",
            Ids::ComAtprotoTempFetchLabels => "com.atproto.temp.fetchLabels",
            Ids::ComAtprotoTempRequestPhoneVerification => {
                "com.atproto.temp.requestPhoneVerification"
            }
            Ids::AppBskyActorDefs => "app.bsky.actor.defs",
            Ids::AppBskyActorGetPreferences => "app.bsky.actor.getPreferences",
            Ids::AppBskyActorGetProfile => "app.bsky.actor.getProfile",
            Ids::AppBskyActorGetProfiles => "app.bsky.actor.getProfiles",
            Ids::AppBskyActorGetSuggestions => "app.bsky.actor.getSuggestions",
            Ids::AppBskyActorProfile => "app.bsky.actor.profile",
            Ids::AppBskyActorPutPreferences => "app.bsky.actor.putPreferences",
            Ids::AppBskyActorSearchActors => "app.bsky.actor.searchActors",
            Ids::AppBskyActorSearchActorsTypeahead => "app.bsky.actor.searchActorsTypeahead",
            Ids::AppBskyEmbedExternal => "app.bsky.embed.external",
            Ids::AppBskyEmbedImages => "app.bsky.embed.images",
            Ids::AppBskyEmbedRecord => "app.bsky.embed.record",
            Ids::AppBskyEmbedRecordWithMedia => "app.bsky.embed.recordWithMedia",
            Ids::AppBskyFeedDefs => "app.bsky.feed.defs",
            Ids::AppBskyFeedDescribeFeedGenerator => "app.bsky.feed.describeFeedGenerator",
            Ids::AppBskyFeedGenerator => "app.bsky.feed.generator",
            Ids::AppBskyFeedGetActorFeeds => "app.bsky.feed.getActorFeeds",
            Ids::AppBskyFeedGetActorLikes => "app.bsky.feed.getActorLikes",
            Ids::AppBskyFeedGetAuthorFeed => "app.bsky.feed.getAuthorFeed",
            Ids::AppBskyFeedGetFeed => "app.bsky.feed.getFeed",
            Ids::AppBskyFeedGetFeedGenerator => "app.bsky.feed.getFeedGenerator",
            Ids::AppBskyFeedGetFeedGenerators => "app.bsky.feed.getFeedGenerators",
            Ids::AppBskyFeedGetFeedSkeleton => "app.bsky.feed.getFeedSkeleton",
            Ids::AppBskyFeedGetLikes => "app.bsky.feed.getLikes",
            Ids::AppBskyFeedGetListFeed => "app.bsky.feed.getListFeed",
            Ids::AppBskyFeedGetPostThread => "app.bsky.feed.getPostThread",
            Ids::AppBskyFeedGetPosts => "app.bsky.feed.getPosts",
            Ids::AppBskyFeedGetRepostedBy => "app.bsky.feed.getRepostedBy",
            Ids::AppBskyFeedGetSuggestedFeeds => "app.bsky.feed.getSuggestedFeeds",
            Ids::AppBskyFeedGetTimeline => "app.bsky.feed.getTimeline",
            Ids::AppBskyFeedLike => "app.bsky.feed.like",
            Ids::AppBskyFeedPost => "app.bsky.feed.post",
            Ids::AppBskyFeedRepost => "app.bsky.feed.repost",
            Ids::AppBskyFeedSearchPosts => "app.bsky.feed.searchPosts",
            Ids::AppBskyFeedThreadgate => "app.bsky.feed.threadgate",
            Ids::AppBskyGraphBlock => "app.bsky.graph.block",
            Ids::AppBskyGraphDefs => "app.bsky.graph.defs",
            Ids::AppBskyGraphFollow => "app.bsky.graph.follow",
            Ids::AppBskyGraphGetBlocks => "app.bsky.graph.getBlocks",
            Ids::AppBskyGraphGetFollowers => "app.bsky.graph.getFollowers",
            Ids::AppBskyGraphGetFollows => "app.bsky.graph.getFollows",
            Ids::AppBskyGraphGetList => "app.bsky.graph.getList",
            Ids::AppBskyGraphGetListBlocks => "app.bsky.graph.getListBlocks",
            Ids::AppBskyGraphGetListMutes => "app.bsky.graph.getListMutes",
            Ids::AppBskyGraphGetLists => "app.bsky.graph.getLists",
            Ids::AppBskyGraphGetMutes => "app.bsky.graph.getMutes",
            Ids::AppBskyGraphGetRelationships => "app.bsky.graph.getRelationships",
            Ids::AppBskyGraphGetSuggestedFollowsByActor => {
                "app.bsky.graph.getSuggestedFollowsByActor"
            }
            Ids::AppBskyGraphList => "app.bsky.graph.list",
            Ids::AppBskyGraphListblock => "app.bsky.graph.listblock",
            Ids::AppBskyGraphListitem => "app.bsky.graph.listitem",
            Ids::AppBskyGraphMuteActor => "app.bsky.graph.muteActor",
            Ids::AppBskyGraphMuteActorList => "app.bsky.graph.muteActorList",
            Ids::AppBskyGraphUnmuteActor => "app.bsky.graph.unmuteActor",
            Ids::AppBskyGraphUnmuteActorList => "app.bsky.graph.unmuteActorList",
            Ids::AppBskyLabelerDefs => "app.bsky.labeler.defs",
            Ids::AppBskyLabelerGetServices => "app.bsky.labeler.getServices",
            Ids::AppBskyLabelerService => "app.bsky.labeler.service",
            Ids::AppBskyNotificationGetUnreadCount => "app.bsky.notification.getUnreadCount",
            Ids::AppBskyNotificationListNotifications => "app.bsky.notification.listNotifications",
            Ids::AppBskyNotificationRegisterPush => "app.bsky.notification.registerPush",
            Ids::AppBskyNotificationUpdateSeen => "app.bsky.notification.updateSeen",
            Ids::AppBskyRichtextFacet => "app.bsky.richtext.facet",
            Ids::AppBskyUnspeccedDefs => "app.bsky.unspecced.defs",
            Ids::AppBskyUnspeccedGetPopularFeedGenerators => {
                "app.bsky.unspecced.getPopularFeedGenerators"
            }
            Ids::AppBskyUnspeccedGetTaggedSuggestions => "app.bsky.unspecced.getTaggedSuggestions",
            Ids::AppBskyUnspeccedSearchActorsSkeleton => "app.bsky.unspecced.searchActorsSkeleton",
            Ids::AppBskyUnspeccedSearchPostsSkeleton => "app.bsky.unspecced.searchPostsSkeleton",
            Ids::ToolsOzoneCommunicationCreateTemplate => {
                "tools.ozone.communication.createTemplate"
            }
            Ids::ToolsOzoneCommunicationDefs => "tools.ozone.communication.defs",
            Ids::ToolsOzoneCommunicationDeleteTemplate => {
                "tools.ozone.communication.deleteTemplate"
            }
            Ids::ToolsOzoneCommunicationListTemplates => "tools.ozone.communication.listTemplates",
            Ids::ToolsOzoneCommunicationUpdateTemplate => {
                "tools.ozone.communication.updateTemplate"
            }
            Ids::ToolsOzoneModerationDefs => "tools.ozone.moderation.defs",
            Ids::ToolsOzoneModerationEmitEvent => "tools.ozone.moderation.emitEvent",
            Ids::ToolsOzoneModerationGetEvent => "tools.ozone.moderation.getEvent",
            Ids::ToolsOzoneModerationGetRecord => "tools.ozone.moderation.getRecord",
            Ids::ToolsOzoneModerationGetRepo => "tools.ozone.moderation.getRepo",
            Ids::ToolsOzoneModerationQueryEvents => "tools.ozone.moderation.queryEvents",
            Ids::ToolsOzoneModerationQueryStatuses => "tools.ozone.moderation.queryStatuses",
            Ids::ToolsOzoneModerationSearchRepos => "tools.ozone.moderation.searchRepos",
        }
    }
}
