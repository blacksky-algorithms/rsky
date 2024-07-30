/**
 * GENERATED CODE - DO NOT MODIFY
 */
use serde_derive::Deserialize;
use serde_derive::Serialize;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Root {
    #[serde(rename = "ComAtprotoAdminDefs")]
    pub com_atproto_admin_defs: ComAtprotoAdminDefs,
    #[serde(rename = "ComAtprotoAdminDeleteAccount")]
    pub com_atproto_admin_delete_account: ComAtprotoAdminDeleteAccount,
    #[serde(rename = "ComAtprotoAdminDisableAccountInvites")]
    pub com_atproto_admin_disable_account_invites: ComAtprotoAdminDisableAccountInvites,
    #[serde(rename = "ComAtprotoAdminDisableInviteCodes")]
    pub com_atproto_admin_disable_invite_codes: ComAtprotoAdminDisableInviteCodes,
    #[serde(rename = "ComAtprotoAdminEnableAccountInvites")]
    pub com_atproto_admin_enable_account_invites: ComAtprotoAdminEnableAccountInvites,
    #[serde(rename = "ComAtprotoAdminGetAccountInfo")]
    pub com_atproto_admin_get_account_info: ComAtprotoAdminGetAccountInfo,
    #[serde(rename = "ComAtprotoAdminGetAccountInfos")]
    pub com_atproto_admin_get_account_infos: ComAtprotoAdminGetAccountInfos,
    #[serde(rename = "ComAtprotoAdminGetInviteCodes")]
    pub com_atproto_admin_get_invite_codes: ComAtprotoAdminGetInviteCodes,
    #[serde(rename = "ComAtprotoAdminGetSubjectStatus")]
    pub com_atproto_admin_get_subject_status: ComAtprotoAdminGetSubjectStatus,
    #[serde(rename = "ComAtprotoAdminSendEmail")]
    pub com_atproto_admin_send_email: ComAtprotoAdminSendEmail,
    #[serde(rename = "ComAtprotoAdminUpdateAccountEmail")]
    pub com_atproto_admin_update_account_email: ComAtprotoAdminUpdateAccountEmail,
    #[serde(rename = "ComAtprotoAdminUpdateAccountHandle")]
    pub com_atproto_admin_update_account_handle: ComAtprotoAdminUpdateAccountHandle,
    #[serde(rename = "ComAtprotoAdminUpdateAccountPassword")]
    pub com_atproto_admin_update_account_password: ComAtprotoAdminUpdateAccountPassword,
    #[serde(rename = "ComAtprotoAdminUpdateSubjectStatus")]
    pub com_atproto_admin_update_subject_status: ComAtprotoAdminUpdateSubjectStatus,
    #[serde(rename = "ComAtprotoIdentityGetRecommendedDidCredentials")]
    pub com_atproto_identity_get_recommended_did_credentials:
        ComAtprotoIdentityGetRecommendedDidCredentials,
    #[serde(rename = "ComAtprotoIdentityRequestPlcOperationSignature")]
    pub com_atproto_identity_request_plc_operation_signature:
        ComAtprotoIdentityRequestPlcOperationSignature,
    #[serde(rename = "ComAtprotoIdentityResolveHandle")]
    pub com_atproto_identity_resolve_handle: ComAtprotoIdentityResolveHandle,
    #[serde(rename = "ComAtprotoIdentitySignPlcOperation")]
    pub com_atproto_identity_sign_plc_operation: ComAtprotoIdentitySignPlcOperation,
    #[serde(rename = "ComAtprotoIdentitySubmitPlcOperation")]
    pub com_atproto_identity_submit_plc_operation: ComAtprotoIdentitySubmitPlcOperation,
    #[serde(rename = "ComAtprotoIdentityUpdateHandle")]
    pub com_atproto_identity_update_handle: ComAtprotoIdentityUpdateHandle,
    #[serde(rename = "ComAtprotoLabelDefs")]
    pub com_atproto_label_defs: ComAtprotoLabelDefs,
    #[serde(rename = "ComAtprotoLabelQueryLabels")]
    pub com_atproto_label_query_labels: ComAtprotoLabelQueryLabels,
    #[serde(rename = "ComAtprotoLabelSubscribeLabels")]
    pub com_atproto_label_subscribe_labels: ComAtprotoLabelSubscribeLabels,
    #[serde(rename = "ComAtprotoModerationCreateReport")]
    pub com_atproto_moderation_create_report: ComAtprotoModerationCreateReport,
    #[serde(rename = "ComAtprotoModerationDefs")]
    pub com_atproto_moderation_defs: ComAtprotoModerationDefs,
    #[serde(rename = "ComAtprotoRepoApplyWrites")]
    pub com_atproto_repo_apply_writes: ComAtprotoRepoApplyWrites,
    #[serde(rename = "ComAtprotoRepoCreateRecord")]
    pub com_atproto_repo_create_record: ComAtprotoRepoCreateRecord,
    #[serde(rename = "ComAtprotoRepoDeleteRecord")]
    pub com_atproto_repo_delete_record: ComAtprotoRepoDeleteRecord,
    #[serde(rename = "ComAtprotoRepoDescribeRepo")]
    pub com_atproto_repo_describe_repo: ComAtprotoRepoDescribeRepo,
    #[serde(rename = "ComAtprotoRepoGetRecord")]
    pub com_atproto_repo_get_record: ComAtprotoRepoGetRecord,
    #[serde(rename = "ComAtprotoRepoImportRepo")]
    pub com_atproto_repo_import_repo: ComAtprotoRepoImportRepo,
    #[serde(rename = "ComAtprotoRepoListMissingBlobs")]
    pub com_atproto_repo_list_missing_blobs: ComAtprotoRepoListMissingBlobs,
    #[serde(rename = "ComAtprotoRepoListRecords")]
    pub com_atproto_repo_list_records: ComAtprotoRepoListRecords,
    #[serde(rename = "ComAtprotoRepoPutRecord")]
    pub com_atproto_repo_put_record: ComAtprotoRepoPutRecord,
    #[serde(rename = "ComAtprotoRepoStrongRef")]
    pub com_atproto_repo_strong_ref: ComAtprotoRepoStrongRef,
    #[serde(rename = "ComAtprotoRepoUploadBlob")]
    pub com_atproto_repo_upload_blob: ComAtprotoRepoUploadBlob,
    #[serde(rename = "ComAtprotoServerActivateAccount")]
    pub com_atproto_server_activate_account: ComAtprotoServerActivateAccount,
    #[serde(rename = "ComAtprotoServerCheckAccountStatus")]
    pub com_atproto_server_check_account_status: ComAtprotoServerCheckAccountStatus,
    #[serde(rename = "ComAtprotoServerConfirmEmail")]
    pub com_atproto_server_confirm_email: ComAtprotoServerConfirmEmail,
    #[serde(rename = "ComAtprotoServerCreateAccount")]
    pub com_atproto_server_create_account: ComAtprotoServerCreateAccount,
    #[serde(rename = "ComAtprotoServerCreateAppPassword")]
    pub com_atproto_server_create_app_password: ComAtprotoServerCreateAppPassword,
    #[serde(rename = "ComAtprotoServerCreateInviteCode")]
    pub com_atproto_server_create_invite_code: ComAtprotoServerCreateInviteCode,
    #[serde(rename = "ComAtprotoServerCreateInviteCodes")]
    pub com_atproto_server_create_invite_codes: ComAtprotoServerCreateInviteCodes,
    #[serde(rename = "ComAtprotoServerCreateSession")]
    pub com_atproto_server_create_session: ComAtprotoServerCreateSession,
    #[serde(rename = "ComAtprotoServerDeactivateAccount")]
    pub com_atproto_server_deactivate_account: ComAtprotoServerDeactivateAccount,
    #[serde(rename = "ComAtprotoServerDefs")]
    pub com_atproto_server_defs: ComAtprotoServerDefs,
    #[serde(rename = "ComAtprotoServerDeleteAccount")]
    pub com_atproto_server_delete_account: ComAtprotoServerDeleteAccount,
    #[serde(rename = "ComAtprotoServerDeleteSession")]
    pub com_atproto_server_delete_session: ComAtprotoServerDeleteSession,
    #[serde(rename = "ComAtprotoServerDescribeServer")]
    pub com_atproto_server_describe_server: ComAtprotoServerDescribeServer,
    #[serde(rename = "ComAtprotoServerGetAccountInviteCodes")]
    pub com_atproto_server_get_account_invite_codes: ComAtprotoServerGetAccountInviteCodes,
    #[serde(rename = "ComAtprotoServerGetServiceAuth")]
    pub com_atproto_server_get_service_auth: ComAtprotoServerGetServiceAuth,
    #[serde(rename = "ComAtprotoServerGetSession")]
    pub com_atproto_server_get_session: ComAtprotoServerGetSession,
    #[serde(rename = "ComAtprotoServerListAppPasswords")]
    pub com_atproto_server_list_app_passwords: ComAtprotoServerListAppPasswords,
    #[serde(rename = "ComAtprotoServerRefreshSession")]
    pub com_atproto_server_refresh_session: ComAtprotoServerRefreshSession,
    #[serde(rename = "ComAtprotoServerRequestAccountDelete")]
    pub com_atproto_server_request_account_delete: ComAtprotoServerRequestAccountDelete,
    #[serde(rename = "ComAtprotoServerRequestEmailConfirmation")]
    pub com_atproto_server_request_email_confirmation: ComAtprotoServerRequestEmailConfirmation,
    #[serde(rename = "ComAtprotoServerRequestEmailUpdate")]
    pub com_atproto_server_request_email_update: ComAtprotoServerRequestEmailUpdate,
    #[serde(rename = "ComAtprotoServerRequestPasswordReset")]
    pub com_atproto_server_request_password_reset: ComAtprotoServerRequestPasswordReset,
    #[serde(rename = "ComAtprotoServerReserveSigningKey")]
    pub com_atproto_server_reserve_signing_key: ComAtprotoServerReserveSigningKey,
    #[serde(rename = "ComAtprotoServerResetPassword")]
    pub com_atproto_server_reset_password: ComAtprotoServerResetPassword,
    #[serde(rename = "ComAtprotoServerRevokeAppPassword")]
    pub com_atproto_server_revoke_app_password: ComAtprotoServerRevokeAppPassword,
    #[serde(rename = "ComAtprotoServerUpdateEmail")]
    pub com_atproto_server_update_email: ComAtprotoServerUpdateEmail,
    #[serde(rename = "ComAtprotoSyncGetBlob")]
    pub com_atproto_sync_get_blob: ComAtprotoSyncGetBlob,
    #[serde(rename = "ComAtprotoSyncGetBlocks")]
    pub com_atproto_sync_get_blocks: ComAtprotoSyncGetBlocks,
    #[serde(rename = "ComAtprotoSyncGetCheckout")]
    pub com_atproto_sync_get_checkout: ComAtprotoSyncGetCheckout,
    #[serde(rename = "ComAtprotoSyncGetHead")]
    pub com_atproto_sync_get_head: ComAtprotoSyncGetHead,
    #[serde(rename = "ComAtprotoSyncGetLatestCommit")]
    pub com_atproto_sync_get_latest_commit: ComAtprotoSyncGetLatestCommit,
    #[serde(rename = "ComAtprotoSyncGetRecord")]
    pub com_atproto_sync_get_record: ComAtprotoSyncGetRecord,
    #[serde(rename = "ComAtprotoSyncGetRepo")]
    pub com_atproto_sync_get_repo: ComAtprotoSyncGetRepo,
    #[serde(rename = "ComAtprotoSyncListBlobs")]
    pub com_atproto_sync_list_blobs: ComAtprotoSyncListBlobs,
    #[serde(rename = "ComAtprotoSyncListRepos")]
    pub com_atproto_sync_list_repos: ComAtprotoSyncListRepos,
    #[serde(rename = "ComAtprotoSyncNotifyOfUpdate")]
    pub com_atproto_sync_notify_of_update: ComAtprotoSyncNotifyOfUpdate,
    #[serde(rename = "ComAtprotoSyncRequestCrawl")]
    pub com_atproto_sync_request_crawl: ComAtprotoSyncRequestCrawl,
    #[serde(rename = "ComAtprotoSyncSubscribeRepos")]
    pub com_atproto_sync_subscribe_repos: ComAtprotoSyncSubscribeRepos,
    #[serde(rename = "ComAtprotoTempCheckSignupQueue")]
    pub com_atproto_temp_check_signup_queue: ComAtprotoTempCheckSignupQueue,
    #[serde(rename = "ComAtprotoTempFetchLabels")]
    pub com_atproto_temp_fetch_labels: ComAtprotoTempFetchLabels,
    #[serde(rename = "ComAtprotoTempRequestPhoneVerification")]
    pub com_atproto_temp_request_phone_verification: ComAtprotoTempRequestPhoneVerification,
    #[serde(rename = "AppBskyActorDefs")]
    pub app_bsky_actor_defs: AppBskyActorDefs,
    #[serde(rename = "AppBskyActorGetPreferences")]
    pub app_bsky_actor_get_preferences: AppBskyActorGetPreferences,
    #[serde(rename = "AppBskyActorGetProfile")]
    pub app_bsky_actor_get_profile: AppBskyActorGetProfile,
    #[serde(rename = "AppBskyActorGetProfiles")]
    pub app_bsky_actor_get_profiles: AppBskyActorGetProfiles,
    #[serde(rename = "AppBskyActorGetSuggestions")]
    pub app_bsky_actor_get_suggestions: AppBskyActorGetSuggestions,
    #[serde(rename = "AppBskyActorProfile")]
    pub app_bsky_actor_profile: AppBskyActorProfile,
    #[serde(rename = "AppBskyActorPutPreferences")]
    pub app_bsky_actor_put_preferences: AppBskyActorPutPreferences,
    #[serde(rename = "AppBskyActorSearchActors")]
    pub app_bsky_actor_search_actors: AppBskyActorSearchActors,
    #[serde(rename = "AppBskyActorSearchActorsTypeahead")]
    pub app_bsky_actor_search_actors_typeahead: AppBskyActorSearchActorsTypeahead,
    #[serde(rename = "AppBskyEmbedExternal")]
    pub app_bsky_embed_external: AppBskyEmbedExternal,
    #[serde(rename = "AppBskyEmbedImages")]
    pub app_bsky_embed_images: AppBskyEmbedImages,
    #[serde(rename = "AppBskyEmbedRecord")]
    pub app_bsky_embed_record: AppBskyEmbedRecord,
    #[serde(rename = "AppBskyEmbedRecordWithMedia")]
    pub app_bsky_embed_record_with_media: AppBskyEmbedRecordWithMedia,
    #[serde(rename = "AppBskyFeedDefs")]
    pub app_bsky_feed_defs: AppBskyFeedDefs,
    #[serde(rename = "AppBskyFeedDescribeFeedGenerator")]
    pub app_bsky_feed_describe_feed_generator: AppBskyFeedDescribeFeedGenerator,
    #[serde(rename = "AppBskyFeedGenerator")]
    pub app_bsky_feed_generator: AppBskyFeedGenerator,
    #[serde(rename = "AppBskyFeedGetActorFeeds")]
    pub app_bsky_feed_get_actor_feeds: AppBskyFeedGetActorFeeds,
    #[serde(rename = "AppBskyFeedGetActorLikes")]
    pub app_bsky_feed_get_actor_likes: AppBskyFeedGetActorLikes,
    #[serde(rename = "AppBskyFeedGetAuthorFeed")]
    pub app_bsky_feed_get_author_feed: AppBskyFeedGetAuthorFeed,
    #[serde(rename = "AppBskyFeedGetFeed")]
    pub app_bsky_feed_get_feed: AppBskyFeedGetFeed,
    #[serde(rename = "AppBskyFeedGetFeedGenerator")]
    pub app_bsky_feed_get_feed_generator: AppBskyFeedGetFeedGenerator,
    #[serde(rename = "AppBskyFeedGetFeedGenerators")]
    pub app_bsky_feed_get_feed_generators: AppBskyFeedGetFeedGenerators,
    #[serde(rename = "AppBskyFeedGetFeedSkeleton")]
    pub app_bsky_feed_get_feed_skeleton: AppBskyFeedGetFeedSkeleton,
    #[serde(rename = "AppBskyFeedGetLikes")]
    pub app_bsky_feed_get_likes: AppBskyFeedGetLikes,
    #[serde(rename = "AppBskyFeedGetListFeed")]
    pub app_bsky_feed_get_list_feed: AppBskyFeedGetListFeed,
    #[serde(rename = "AppBskyFeedGetPostThread")]
    pub app_bsky_feed_get_post_thread: AppBskyFeedGetPostThread,
    #[serde(rename = "AppBskyFeedGetPosts")]
    pub app_bsky_feed_get_posts: AppBskyFeedGetPosts,
    #[serde(rename = "AppBskyFeedGetRepostedBy")]
    pub app_bsky_feed_get_reposted_by: AppBskyFeedGetRepostedBy,
    #[serde(rename = "AppBskyFeedGetSuggestedFeeds")]
    pub app_bsky_feed_get_suggested_feeds: AppBskyFeedGetSuggestedFeeds,
    #[serde(rename = "AppBskyFeedGetTimeline")]
    pub app_bsky_feed_get_timeline: AppBskyFeedGetTimeline,
    #[serde(rename = "AppBskyFeedLike")]
    pub app_bsky_feed_like: AppBskyFeedLike,
    #[serde(rename = "AppBskyFeedPost")]
    pub app_bsky_feed_post: AppBskyFeedPost,
    #[serde(rename = "AppBskyFeedRepost")]
    pub app_bsky_feed_repost: AppBskyFeedRepost,
    #[serde(rename = "AppBskyFeedSearchPosts")]
    pub app_bsky_feed_search_posts: AppBskyFeedSearchPosts,
    #[serde(rename = "AppBskyFeedSendInteractions")]
    pub app_bsky_feed_send_interactions: AppBskyFeedSendInteractions,
    #[serde(rename = "AppBskyFeedThreadgate")]
    pub app_bsky_feed_threadgate: AppBskyFeedThreadgate,
    #[serde(rename = "AppBskyGraphBlock")]
    pub app_bsky_graph_block: AppBskyGraphBlock,
    #[serde(rename = "AppBskyGraphDefs")]
    pub app_bsky_graph_defs: AppBskyGraphDefs,
    #[serde(rename = "AppBskyGraphFollow")]
    pub app_bsky_graph_follow: AppBskyGraphFollow,
    #[serde(rename = "AppBskyGraphGetBlocks")]
    pub app_bsky_graph_get_blocks: AppBskyGraphGetBlocks,
    #[serde(rename = "AppBskyGraphGetFollowers")]
    pub app_bsky_graph_get_followers: AppBskyGraphGetFollowers,
    #[serde(rename = "AppBskyGraphGetFollows")]
    pub app_bsky_graph_get_follows: AppBskyGraphGetFollows,
    #[serde(rename = "AppBskyGraphGetList")]
    pub app_bsky_graph_get_list: AppBskyGraphGetList,
    #[serde(rename = "AppBskyGraphGetListBlocks")]
    pub app_bsky_graph_get_list_blocks: AppBskyGraphGetListBlocks,
    #[serde(rename = "AppBskyGraphGetListMutes")]
    pub app_bsky_graph_get_list_mutes: AppBskyGraphGetListMutes,
    #[serde(rename = "AppBskyGraphGetLists")]
    pub app_bsky_graph_get_lists: AppBskyGraphGetLists,
    #[serde(rename = "AppBskyGraphGetMutes")]
    pub app_bsky_graph_get_mutes: AppBskyGraphGetMutes,
    #[serde(rename = "AppBskyGraphGetRelationships")]
    pub app_bsky_graph_get_relationships: AppBskyGraphGetRelationships,
    #[serde(rename = "AppBskyGraphGetSuggestedFollowsByActor")]
    pub app_bsky_graph_get_suggested_follows_by_actor: AppBskyGraphGetSuggestedFollowsByActor,
    #[serde(rename = "AppBskyGraphList")]
    pub app_bsky_graph_list: AppBskyGraphList,
    #[serde(rename = "AppBskyGraphListblock")]
    pub app_bsky_graph_listblock: AppBskyGraphListblock,
    #[serde(rename = "AppBskyGraphListitem")]
    pub app_bsky_graph_listitem: AppBskyGraphListitem,
    #[serde(rename = "AppBskyGraphMuteActor")]
    pub app_bsky_graph_mute_actor: AppBskyGraphMuteActor,
    #[serde(rename = "AppBskyGraphMuteActorList")]
    pub app_bsky_graph_mute_actor_list: AppBskyGraphMuteActorList,
    #[serde(rename = "AppBskyGraphUnmuteActor")]
    pub app_bsky_graph_unmute_actor: AppBskyGraphUnmuteActor,
    #[serde(rename = "AppBskyGraphUnmuteActorList")]
    pub app_bsky_graph_unmute_actor_list: AppBskyGraphUnmuteActorList,
    #[serde(rename = "AppBskyLabelerDefs")]
    pub app_bsky_labeler_defs: AppBskyLabelerDefs,
    #[serde(rename = "AppBskyLabelerGetServices")]
    pub app_bsky_labeler_get_services: AppBskyLabelerGetServices,
    #[serde(rename = "AppBskyLabelerService")]
    pub app_bsky_labeler_service: AppBskyLabelerService,
    #[serde(rename = "AppBskyNotificationGetUnreadCount")]
    pub app_bsky_notification_get_unread_count: AppBskyNotificationGetUnreadCount,
    #[serde(rename = "AppBskyNotificationListNotifications")]
    pub app_bsky_notification_list_notifications: AppBskyNotificationListNotifications,
    #[serde(rename = "AppBskyNotificationRegisterPush")]
    pub app_bsky_notification_register_push: AppBskyNotificationRegisterPush,
    #[serde(rename = "AppBskyNotificationUpdateSeen")]
    pub app_bsky_notification_update_seen: AppBskyNotificationUpdateSeen,
    #[serde(rename = "AppBskyRichtextFacet")]
    pub app_bsky_richtext_facet: AppBskyRichtextFacet,
    #[serde(rename = "AppBskyUnspeccedDefs")]
    pub app_bsky_unspecced_defs: AppBskyUnspeccedDefs,
    #[serde(rename = "AppBskyUnspeccedGetPopularFeedGenerators")]
    pub app_bsky_unspecced_get_popular_feed_generators: AppBskyUnspeccedGetPopularFeedGenerators,
    #[serde(rename = "AppBskyUnspeccedGetTaggedSuggestions")]
    pub app_bsky_unspecced_get_tagged_suggestions: AppBskyUnspeccedGetTaggedSuggestions,
    #[serde(rename = "AppBskyUnspeccedSearchActorsSkeleton")]
    pub app_bsky_unspecced_search_actors_skeleton: AppBskyUnspeccedSearchActorsSkeleton,
    #[serde(rename = "AppBskyUnspeccedSearchPostsSkeleton")]
    pub app_bsky_unspecced_search_posts_skeleton: AppBskyUnspeccedSearchPostsSkeleton,
    #[serde(rename = "ToolsOzoneCommunicationCreateTemplate")]
    pub tools_ozone_communication_create_template: ToolsOzoneCommunicationCreateTemplate,
    #[serde(rename = "ToolsOzoneCommunicationDefs")]
    pub tools_ozone_communication_defs: ToolsOzoneCommunicationDefs,
    #[serde(rename = "ToolsOzoneCommunicationDeleteTemplate")]
    pub tools_ozone_communication_delete_template: ToolsOzoneCommunicationDeleteTemplate,
    #[serde(rename = "ToolsOzoneCommunicationListTemplates")]
    pub tools_ozone_communication_list_templates: ToolsOzoneCommunicationListTemplates,
    #[serde(rename = "ToolsOzoneCommunicationUpdateTemplate")]
    pub tools_ozone_communication_update_template: ToolsOzoneCommunicationUpdateTemplate,
    #[serde(rename = "ToolsOzoneModerationDefs")]
    pub tools_ozone_moderation_defs: ToolsOzoneModerationDefs,
    #[serde(rename = "ToolsOzoneModerationEmitEvent")]
    pub tools_ozone_moderation_emit_event: ToolsOzoneModerationEmitEvent,
    #[serde(rename = "ToolsOzoneModerationGetEvent")]
    pub tools_ozone_moderation_get_event: ToolsOzoneModerationGetEvent,
    #[serde(rename = "ToolsOzoneModerationGetRecord")]
    pub tools_ozone_moderation_get_record: ToolsOzoneModerationGetRecord,
    #[serde(rename = "ToolsOzoneModerationGetRepo")]
    pub tools_ozone_moderation_get_repo: ToolsOzoneModerationGetRepo,
    #[serde(rename = "ToolsOzoneModerationQueryEvents")]
    pub tools_ozone_moderation_query_events: ToolsOzoneModerationQueryEvents,
    #[serde(rename = "ToolsOzoneModerationQueryStatuses")]
    pub tools_ozone_moderation_query_statuses: ToolsOzoneModerationQueryStatuses,
    #[serde(rename = "ToolsOzoneModerationSearchRepos")]
    pub tools_ozone_moderation_search_repos: ToolsOzoneModerationSearchRepos,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoAdminDefs {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs {
    pub status_attr: StatusAttr,
    pub account_view: AccountView,
    pub repo_ref: RepoRef,
    pub repo_blob_ref: RepoBlobRef,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusAttr {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties {
    pub applied: Applied,
    #[serde(rename = "ref")]
    pub ref_field: Ref,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ref {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties2 {
    pub did: Did,
    pub handle: Handle,
    pub email: Email,
    pub related_records: RelatedRecords,
    pub indexed_at: IndexedAt,
    pub invited_by: InvitedBy,
    pub invites: Invites,
    pub invites_disabled: InvitesDisabled,
    pub email_confirmed_at: EmailConfirmedAt,
    pub invite_note: InviteNote,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handle {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Email {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedRecords {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedAt {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvitedBy {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Invites {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items2 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvitesDisabled {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailConfirmedAt {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteNote {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoRef {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties3 {
    pub did: Did2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoBlobRef {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties4 {
    pub did: Did3,
    pub cid: Cid,
    pub record_uri: RecordUri,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordUri {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoAdminDeleteAccount {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs2 {
    pub main: Main,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input {
    pub encoding: String,
    pub schema: Schema,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties5,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties5 {
    pub did: Did4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoAdminDisableAccountInvites {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs3 {
    pub main: Main2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input2 {
    pub encoding: String,
    pub schema: Schema2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties6,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties6 {
    pub account: Account,
    pub note: Note,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoAdminDisableInviteCodes {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs4 {
    pub main: Main3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input3 {
    pub encoding: String,
    pub schema: Schema3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties7,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties7 {
    pub codes: Codes,
    pub accounts: Accounts,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Codes {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items3 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Accounts {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items4 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoAdminEnableAccountInvites {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs5,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs5 {
    pub main: Main4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input4 {
    pub encoding: String,
    pub schema: Schema4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties8,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties8 {
    pub account: Account2,
    pub note: Note2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoAdminGetAccountInfo {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs6,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs6 {
    pub main: Main5,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters,
    pub output: Output,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties9,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties9 {
    pub did: Did5,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    pub encoding: String,
    pub schema: Schema5,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema5 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoAdminGetAccountInfos {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs7,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs7 {
    pub main: Main6,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters2,
    pub output: Output2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties10,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties10 {
    pub dids: Dids,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dids {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items5,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output2 {
    pub encoding: String,
    pub schema: Schema6,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties11,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties11 {
    pub infos: Infos,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Infos {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items6,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items6 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoAdminGetInviteCodes {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs8,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs8 {
    pub main: Main7,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main7 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters3,
    pub output: Output3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties12,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties12 {
    pub sort: Sort,
    pub limit: Limit,
    pub cursor: Cursor,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sort {
    #[serde(rename = "type")]
    pub type_field: String,
    pub known_values: Vec<String>,
    pub default: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output3 {
    pub encoding: String,
    pub schema: Schema7,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema7 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties13,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties13 {
    pub cursor: Cursor2,
    pub codes: Codes2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Codes2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items7,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items7 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoAdminGetSubjectStatus {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs9,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs9 {
    pub main: Main8,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main8 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters4,
    pub output: Output4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties14,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties14 {
    pub did: Did6,
    pub uri: Uri,
    pub blob: Blob,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blob {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output4 {
    pub encoding: String,
    pub schema: Schema8,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema8 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties15,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties15 {
    pub subject: Subject,
    pub takedown: Takedown,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Takedown {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoAdminSendEmail {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs10,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs10 {
    pub main: Main9,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main9 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input5,
    pub output: Output5,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input5 {
    pub encoding: String,
    pub schema: Schema9,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema9 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties16,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties16 {
    pub recipient_did: RecipientDid,
    pub content: Content,
    pub subject: Subject2,
    pub sender_did: SenderDid,
    pub comment: Comment,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipientDid {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Content {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SenderDid {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output5 {
    pub encoding: String,
    pub schema: Schema10,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema10 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties17,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties17 {
    pub sent: Sent,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sent {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoAdminUpdateAccountEmail {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs11,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs11 {
    pub main: Main10,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main10 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input6,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input6 {
    pub encoding: String,
    pub schema: Schema11,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema11 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties18,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties18 {
    pub account: Account3,
    pub email: Email2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Email2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoAdminUpdateAccountHandle {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs12,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs12 {
    pub main: Main11,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main11 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input7,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input7 {
    pub encoding: String,
    pub schema: Schema12,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema12 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties19,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties19 {
    pub did: Did7,
    pub handle: Handle2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did7 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handle2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoAdminUpdateAccountPassword {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs13,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs13 {
    pub main: Main12,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main12 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input8,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input8 {
    pub encoding: String,
    pub schema: Schema13,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema13 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties20,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties20 {
    pub did: Did8,
    pub password: Password,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did8 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Password {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoAdminUpdateSubjectStatus {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs14,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs14 {
    pub main: Main13,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main13 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input9,
    pub output: Output6,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input9 {
    pub encoding: String,
    pub schema: Schema14,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema14 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties21,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties21 {
    pub subject: Subject3,
    pub takedown: Takedown2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Takedown2 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output6 {
    pub encoding: String,
    pub schema: Schema15,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema15 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties22,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties22 {
    pub subject: Subject4,
    pub takedown: Takedown3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Takedown3 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoIdentityGetRecommendedDidCredentials {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs15,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs15 {
    pub main: Main14,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main14 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub output: Output7,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output7 {
    pub encoding: String,
    pub schema: Schema16,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema16 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties23,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties23 {
    pub rotation_keys: RotationKeys,
    pub also_known_as: AlsoKnownAs,
    pub verification_methods: VerificationMethods,
    pub services: Services,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RotationKeys {
    pub description: String,
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items8,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items8 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlsoKnownAs {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items9,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items9 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationMethods {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Services {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoIdentityRequestPlcOperationSignature {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs16,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs16 {
    pub main: Main15,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main15 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoIdentityResolveHandle {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs17,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs17 {
    pub main: Main16,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main16 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters5,
    pub output: Output8,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties24,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties24 {
    pub handle: Handle3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handle3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output8 {
    pub encoding: String,
    pub schema: Schema17,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema17 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties25,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties25 {
    pub did: Did9,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did9 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoIdentitySignPlcOperation {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs18,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs18 {
    pub main: Main17,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main17 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input10,
    pub output: Output9,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input10 {
    pub encoding: String,
    pub schema: Schema18,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema18 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties26,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties26 {
    pub token: Token,
    pub rotation_keys: RotationKeys2,
    pub also_known_as: AlsoKnownAs2,
    pub verification_methods: VerificationMethods2,
    pub services: Services2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub description: String,
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RotationKeys2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items10,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items10 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlsoKnownAs2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items11,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items11 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationMethods2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Services2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output9 {
    pub encoding: String,
    pub schema: Schema19,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema19 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties27,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties27 {
    pub operation: Operation,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoIdentitySubmitPlcOperation {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs19,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs19 {
    pub main: Main18,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main18 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input11,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input11 {
    pub encoding: String,
    pub schema: Schema20,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema20 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties28,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties28 {
    pub operation: Operation2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoIdentityUpdateHandle {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs20,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs20 {
    pub main: Main19,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main19 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input12,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input12 {
    pub encoding: String,
    pub schema: Schema21,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema21 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties29,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties29 {
    pub handle: Handle4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handle4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoLabelDefs {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs21,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs21 {
    pub label: Label,
    pub self_labels: SelfLabels,
    pub self_label: SelfLabel,
    pub label_value_definition: LabelValueDefinition,
    pub label_value_definition_strings: LabelValueDefinitionStrings,
    pub label_value: LabelValue,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties30,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties30 {
    pub ver: Ver,
    pub src: Src,
    pub uri: Uri2,
    pub cid: Cid2,
    pub val: Val,
    pub neg: Neg,
    pub cts: Cts,
    pub exp: Exp,
    pub sig: Sig,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ver {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Src {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Val {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_length: i64,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Neg {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cts {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Exp {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sig {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfLabels {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties31,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties31 {
    pub values: Values,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Values {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items12,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items12 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfLabel {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties32,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties32 {
    pub val: Val2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Val2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_length: i64,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelValueDefinition {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties33,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties33 {
    pub identifier: Identifier,
    pub severity: Severity,
    pub blurs: Blurs,
    pub default_setting: DefaultSetting,
    pub adult_only: AdultOnly,
    pub locales: Locales,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identifier {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub max_length: i64,
    pub max_graphemes: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Severity {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub known_values: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blurs {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub known_values: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultSetting {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub known_values: Vec<String>,
    pub default: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdultOnly {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Locales {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items13,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items13 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelValueDefinitionStrings {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties34,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties34 {
    pub lang: Lang,
    pub name: Name,
    pub description: Description,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lang {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Name {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub max_graphemes: i64,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Description {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub max_graphemes: i64,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelValue {
    #[serde(rename = "type")]
    pub type_field: String,
    pub known_values: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoLabelQueryLabels {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs22,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs22 {
    pub main: Main20,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main20 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters6,
    pub output: Output10,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties35,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties35 {
    pub uri_patterns: UriPatterns,
    pub sources: Sources,
    pub limit: Limit2,
    pub cursor: Cursor3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UriPatterns {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items14,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items14 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sources {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items15,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items15 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor3 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output10 {
    pub encoding: String,
    pub schema: Schema22,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema22 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties36,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties36 {
    pub cursor: Cursor4,
    pub labels: Labels,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor4 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items16,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items16 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoLabelSubscribeLabels {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs23,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs23 {
    pub main: Main21,
    pub labels: Labels2,
    pub info: Info,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main21 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters7,
    pub message: Message,
    pub errors: Vec<Error>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters7 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties37,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties37 {
    pub cursor: Cursor5,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub schema: Schema23,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema23 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties38,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties38 {
    pub seq: Seq,
    pub labels: Labels3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Seq {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items17,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items17 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties39,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties39 {
    pub name: Name2,
    pub message: Message2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Name2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub known_values: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoModerationCreateReport {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs24,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs24 {
    pub main: Main22,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main22 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input13,
    pub output: Output11,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input13 {
    pub encoding: String,
    pub schema: Schema24,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema24 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties40,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties40 {
    pub reason_type: ReasonType,
    pub reason: Reason,
    pub subject: Subject5,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasonType {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reason {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_graphemes: i64,
    pub max_length: i64,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output11 {
    pub encoding: String,
    pub schema: Schema25,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema25 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties41,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties41 {
    pub id: Id,
    pub reason_type: ReasonType2,
    pub reason: Reason2,
    pub subject: Subject6,
    pub reported_by: ReportedBy,
    pub created_at: CreatedAt,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Id {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasonType2 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reason2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_graphemes: i64,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportedBy {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAt {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoModerationDefs {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs25,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs25 {
    pub reason_type: ReasonType3,
    pub reason_spam: ReasonSpam,
    pub reason_violation: ReasonViolation,
    pub reason_misleading: ReasonMisleading,
    pub reason_sexual: ReasonSexual,
    pub reason_rude: ReasonRude,
    pub reason_other: ReasonOther,
    pub reason_appeal: ReasonAppeal,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasonType3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub known_values: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasonSpam {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasonViolation {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasonMisleading {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasonSexual {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasonRude {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasonOther {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasonAppeal {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoRepoApplyWrites {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs26,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs26 {
    pub main: Main23,
    pub create: Create,
    pub update: Update,
    pub delete: Delete,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main23 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input14,
    pub errors: Vec<Error2>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input14 {
    pub encoding: String,
    pub schema: Schema26,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema26 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties42,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties42 {
    pub repo: Repo,
    pub validate: Validate,
    pub writes: Writes,
    pub swap_commit: SwapCommit,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repo {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Validate {
    #[serde(rename = "type")]
    pub type_field: String,
    pub default: bool,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Writes {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items18,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items18 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
    pub closed: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapCommit {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error2 {
    pub name: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Create {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties43,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties43 {
    pub collection: Collection,
    pub rkey: Rkey,
    pub value: Value,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rkey {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Value {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties44,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties44 {
    pub collection: Collection2,
    pub rkey: Rkey2,
    pub value: Value2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rkey2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Value2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Delete {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties45,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties45 {
    pub collection: Collection3,
    pub rkey: Rkey3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rkey3 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoRepoCreateRecord {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs27,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs27 {
    pub main: Main24,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main24 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input15,
    pub output: Output12,
    pub errors: Vec<Error3>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input15 {
    pub encoding: String,
    pub schema: Schema27,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema27 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties46,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties46 {
    pub repo: Repo2,
    pub collection: Collection4,
    pub rkey: Rkey4,
    pub validate: Validate2,
    pub record: Record,
    pub swap_commit: SwapCommit2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repo2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rkey4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Validate2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub default: bool,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapCommit2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output12 {
    pub encoding: String,
    pub schema: Schema28,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema28 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties47,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties47 {
    pub uri: Uri3,
    pub cid: Cid3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error3 {
    pub name: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoRepoDeleteRecord {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs28,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs28 {
    pub main: Main25,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main25 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input16,
    pub errors: Vec<Error4>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input16 {
    pub encoding: String,
    pub schema: Schema29,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema29 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties48,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties48 {
    pub repo: Repo3,
    pub collection: Collection5,
    pub rkey: Rkey5,
    pub swap_record: SwapRecord,
    pub swap_commit: SwapCommit3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repo3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rkey5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapRecord {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapCommit3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error4 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoRepoDescribeRepo {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs29,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs29 {
    pub main: Main26,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main26 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters8,
    pub output: Output13,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters8 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties49,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties49 {
    pub repo: Repo4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repo4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output13 {
    pub encoding: String,
    pub schema: Schema30,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema30 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties50,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties50 {
    pub handle: Handle5,
    pub did: Did10,
    pub did_doc: DidDoc,
    pub collections: Collections,
    pub handle_is_correct: HandleIsCorrect,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handle5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did10 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidDoc {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collections {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub items: Items19,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items19 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandleIsCorrect {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoRepoGetRecord {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs30,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs30 {
    pub main: Main27,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main27 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters9,
    pub output: Output14,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters9 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties51,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties51 {
    pub repo: Repo5,
    pub collection: Collection6,
    pub rkey: Rkey6,
    pub cid: Cid4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repo5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rkey6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output14 {
    pub encoding: String,
    pub schema: Schema31,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema31 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties52,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties52 {
    pub uri: Uri4,
    pub cid: Cid5,
    pub value: Value3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Value3 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoRepoImportRepo {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs31,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs31 {
    pub main: Main28,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main28 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input17,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input17 {
    pub encoding: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoRepoListMissingBlobs {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs32,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs32 {
    pub main: Main29,
    pub record_blob: RecordBlob,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main29 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters10,
    pub output: Output15,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters10 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties53,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties53 {
    pub limit: Limit3,
    pub cursor: Cursor6,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor6 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output15 {
    pub encoding: String,
    pub schema: Schema32,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema32 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties54,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties54 {
    pub cursor: Cursor7,
    pub blobs: Blobs,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor7 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blobs {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items20,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items20 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordBlob {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties55,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties55 {
    pub cid: Cid6,
    pub record_uri: RecordUri2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordUri2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoRepoListRecords {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs33,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs33 {
    pub main: Main30,
    pub record: Record2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main30 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters11,
    pub output: Output16,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters11 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties56,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties56 {
    pub repo: Repo6,
    pub collection: Collection7,
    pub limit: Limit4,
    pub cursor: Cursor8,
    pub rkey_start: RkeyStart,
    pub rkey_end: RkeyEnd,
    pub reverse: Reverse,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repo6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection7 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor8 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RkeyStart {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RkeyEnd {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reverse {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output16 {
    pub encoding: String,
    pub schema: Schema33,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema33 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties57,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties57 {
    pub cursor: Cursor9,
    pub records: Records,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor9 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Records {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items21,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items21 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties58,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties58 {
    pub uri: Uri5,
    pub cid: Cid7,
    pub value: Value4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid7 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Value4 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoRepoPutRecord {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs34,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs34 {
    pub main: Main31,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main31 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input18,
    pub output: Output17,
    pub errors: Vec<Error5>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input18 {
    pub encoding: String,
    pub schema: Schema34,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema34 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub nullable: Vec<String>,
    pub properties: Properties59,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties59 {
    pub repo: Repo7,
    pub collection: Collection8,
    pub rkey: Rkey7,
    pub validate: Validate3,
    pub record: Record3,
    pub swap_record: SwapRecord2,
    pub swap_commit: SwapCommit4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repo7 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection8 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rkey7 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Validate3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub default: bool,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapRecord2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapCommit4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output17 {
    pub encoding: String,
    pub schema: Schema35,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema35 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties60,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties60 {
    pub uri: Uri6,
    pub cid: Cid8,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid8 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error5 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoRepoStrongRef {
    pub lexicon: i64,
    pub id: String,
    pub description: String,
    pub defs: Defs35,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs35 {
    pub main: Main32,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main32 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties61,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties61 {
    pub uri: Uri7,
    pub cid: Cid9,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri7 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid9 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoRepoUploadBlob {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs36,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs36 {
    pub main: Main33,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main33 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input19,
    pub output: Output18,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input19 {
    pub encoding: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output18 {
    pub encoding: String,
    pub schema: Schema36,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema36 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties62,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties62 {
    pub blob: Blob2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blob2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerActivateAccount {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs37,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs37 {
    pub main: Main34,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main34 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerCheckAccountStatus {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs38,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs38 {
    pub main: Main35,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main35 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub output: Output19,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output19 {
    pub encoding: String,
    pub schema: Schema37,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema37 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties63,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties63 {
    pub activated: Activated,
    pub valid_did: ValidDid,
    pub repo_commit: RepoCommit,
    pub repo_rev: RepoRev,
    pub repo_blocks: RepoBlocks,
    pub indexed_records: IndexedRecords,
    pub private_state_values: PrivateStateValues,
    pub expected_blobs: ExpectedBlobs,
    pub imported_blobs: ImportedBlobs,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activated {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidDid {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoCommit {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoRev {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoBlocks {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedRecords {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivateStateValues {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpectedBlobs {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedBlobs {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerConfirmEmail {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs39,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs39 {
    pub main: Main36,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main36 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input20,
    pub errors: Vec<Error6>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input20 {
    pub encoding: String,
    pub schema: Schema38,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema38 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties64 {
    pub email: Email3,
    pub token: Token2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Email3 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error6 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerCreateAccount {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs40,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs40 {
    pub main: Main37,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main37 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input21,
    pub output: Output20,
    pub errors: Vec<Error7>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input21 {
    pub encoding: String,
    pub schema: Schema39,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema39 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties65,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties65 {
    pub email: Email4,
    pub handle: Handle6,
    pub did: Did11,
    pub invite_code: InviteCode,
    pub verification_code: VerificationCode,
    pub verification_phone: VerificationPhone,
    pub password: Password2,
    pub recovery_key: RecoveryKey,
    pub plc_op: PlcOp,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Email4 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handle6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did11 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteCode {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationCode {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationPhone {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Password2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryKey {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlcOp {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output20 {
    pub encoding: String,
    pub schema: Schema40,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema40 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties66,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties66 {
    pub access_jwt: AccessJwt,
    pub refresh_jwt: RefreshJwt,
    pub handle: Handle7,
    pub did: Did12,
    pub did_doc: DidDoc2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessJwt {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshJwt {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handle7 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did12 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidDoc2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error7 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerCreateAppPassword {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs41,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs41 {
    pub main: Main38,
    pub app_password: AppPassword,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main38 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input22,
    pub output: Output21,
    pub errors: Vec<Error8>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input22 {
    pub encoding: String,
    pub schema: Schema41,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema41 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties67,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties67 {
    pub name: Name3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Name3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output21 {
    pub encoding: String,
    pub schema: Schema42,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema42 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error8 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPassword {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties68,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties68 {
    pub name: Name4,
    pub password: Password3,
    pub created_at: CreatedAt2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Name4 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Password3 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAt2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerCreateInviteCode {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs42,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs42 {
    pub main: Main39,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main39 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input23,
    pub output: Output22,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input23 {
    pub encoding: String,
    pub schema: Schema43,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema43 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties69,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties69 {
    pub use_count: UseCount,
    pub for_account: ForAccount,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UseCount {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForAccount {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output22 {
    pub encoding: String,
    pub schema: Schema44,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema44 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties70,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties70 {
    pub code: Code,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Code {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerCreateInviteCodes {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs43,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs43 {
    pub main: Main40,
    pub account_codes: AccountCodes,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main40 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input24,
    pub output: Output23,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input24 {
    pub encoding: String,
    pub schema: Schema45,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema45 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties71,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties71 {
    pub code_count: CodeCount,
    pub use_count: UseCount2,
    pub for_accounts: ForAccounts,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeCount {
    #[serde(rename = "type")]
    pub type_field: String,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UseCount2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForAccounts {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items22,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items22 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output23 {
    pub encoding: String,
    pub schema: Schema46,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema46 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties72,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties72 {
    pub codes: Codes3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Codes3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items23,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items23 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountCodes {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties73,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties73 {
    pub account: Account4,
    pub codes: Codes4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account4 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Codes4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items24,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items24 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerCreateSession {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs44,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs44 {
    pub main: Main41,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main41 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input25,
    pub output: Output24,
    pub errors: Vec<Error9>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input25 {
    pub encoding: String,
    pub schema: Schema47,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema47 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties74,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties74 {
    pub identifier: Identifier2,
    pub password: Password4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identifier2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Password4 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output24 {
    pub encoding: String,
    pub schema: Schema48,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema48 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties75,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties75 {
    pub access_jwt: AccessJwt2,
    pub refresh_jwt: RefreshJwt2,
    pub handle: Handle8,
    pub did: Did13,
    pub did_doc: DidDoc3,
    pub email: Email5,
    pub email_confirmed: EmailConfirmed,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessJwt2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshJwt2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handle8 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did13 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidDoc3 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Email5 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailConfirmed {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error9 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerDeactivateAccount {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs45,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs45 {
    pub main: Main42,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main42 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input26,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input26 {
    pub encoding: String,
    pub schema: Schema49,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema49 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties76,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties76 {
    pub delete_after: DeleteAfter,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAfter {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerDefs {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs46,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs46 {
    pub invite_code: InviteCode2,
    pub invite_code_use: InviteCodeUse,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteCode2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties77,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties77 {
    pub code: Code2,
    pub available: Available,
    pub disabled: Disabled,
    pub for_account: ForAccount2,
    pub created_by: CreatedBy,
    pub created_at: CreatedAt3,
    pub uses: Uses,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Code2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Available {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Disabled {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForAccount2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedBy {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAt3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uses {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items25,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items25 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteCodeUse {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties78,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties78 {
    pub used_by: UsedBy,
    pub used_at: UsedAt,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsedBy {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsedAt {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerDeleteAccount {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs47,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs47 {
    pub main: Main43,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main43 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input27,
    pub errors: Vec<Error10>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input27 {
    pub encoding: String,
    pub schema: Schema50,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema50 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties79,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties79 {
    pub did: Did14,
    pub password: Password5,
    pub token: Token3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did14 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Password5 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token3 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error10 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerDeleteSession {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs48,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs48 {
    pub main: Main44,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main44 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerDescribeServer {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs49,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs49 {
    pub main: Main45,
    pub links: Links2,
    pub contact: Contact2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main45 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub output: Output25,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output25 {
    pub encoding: String,
    pub schema: Schema51,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema51 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties80,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties80 {
    pub invite_code_required: InviteCodeRequired,
    pub phone_verification_required: PhoneVerificationRequired,
    pub available_user_domains: AvailableUserDomains,
    pub links: Links,
    pub contact: Contact,
    pub did: Did15,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteCodeRequired {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneVerificationRequired {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableUserDomains {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub items: Items26,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items26 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Links {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did15 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Links2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties81,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties81 {
    pub privacy_policy: PrivacyPolicy,
    pub terms_of_service: TermsOfService,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyPolicy {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TermsOfService {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contact2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties82,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties82 {
    pub email: Email6,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Email6 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerGetAccountInviteCodes {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs50,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs50 {
    pub main: Main46,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main46 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters12,
    pub output: Output26,
    pub errors: Vec<Error11>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters12 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties83,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties83 {
    pub include_used: IncludeUsed,
    pub create_available: CreateAvailable,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncludeUsed {
    #[serde(rename = "type")]
    pub type_field: String,
    pub default: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAvailable {
    #[serde(rename = "type")]
    pub type_field: String,
    pub default: bool,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output26 {
    pub encoding: String,
    pub schema: Schema52,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema52 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties84,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties84 {
    pub codes: Codes5,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Codes5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items27,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items27 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error11 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerGetServiceAuth {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs51,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs51 {
    pub main: Main47,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main47 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters13,
    pub output: Output27,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters13 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties85,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties85 {
    pub aud: Aud,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Aud {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output27 {
    pub encoding: String,
    pub schema: Schema53,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema53 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties86,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties86 {
    pub token: Token4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token4 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerGetSession {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs52,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs52 {
    pub main: Main48,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main48 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub output: Output28,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output28 {
    pub encoding: String,
    pub schema: Schema54,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema54 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties87,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties87 {
    pub handle: Handle9,
    pub did: Did16,
    pub email: Email7,
    pub email_confirmed: EmailConfirmed2,
    pub did_doc: DidDoc4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handle9 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did16 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Email7 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailConfirmed2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidDoc4 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerListAppPasswords {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs53,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs53 {
    pub main: Main49,
    pub app_password: AppPassword2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main49 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub output: Output29,
    pub errors: Vec<Error12>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output29 {
    pub encoding: String,
    pub schema: Schema55,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema55 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties88,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties88 {
    pub passwords: Passwords,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Passwords {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items28,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items28 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error12 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPassword2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties89,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties89 {
    pub name: Name5,
    pub created_at: CreatedAt4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Name5 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAt4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerRefreshSession {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs54,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs54 {
    pub main: Main50,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main50 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub output: Output30,
    pub errors: Vec<Error13>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output30 {
    pub encoding: String,
    pub schema: Schema56,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema56 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties90,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties90 {
    pub access_jwt: AccessJwt3,
    pub refresh_jwt: RefreshJwt3,
    pub handle: Handle10,
    pub did: Did17,
    pub did_doc: DidDoc5,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessJwt3 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshJwt3 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handle10 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did17 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidDoc5 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error13 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerRequestAccountDelete {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs55,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs55 {
    pub main: Main51,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main51 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerRequestEmailConfirmation {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs56,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs56 {
    pub main: Main52,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main52 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerRequestEmailUpdate {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs57,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs57 {
    pub main: Main53,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main53 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub output: Output31,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output31 {
    pub encoding: String,
    pub schema: Schema57,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema57 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties91,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties91 {
    pub token_required: TokenRequired,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenRequired {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerRequestPasswordReset {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs58,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs58 {
    pub main: Main54,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main54 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input28,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input28 {
    pub encoding: String,
    pub schema: Schema58,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema58 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties92,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties92 {
    pub email: Email8,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Email8 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerReserveSigningKey {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs59,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs59 {
    pub main: Main55,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main55 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input29,
    pub output: Output32,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input29 {
    pub encoding: String,
    pub schema: Schema59,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema59 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties93,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties93 {
    pub did: Did18,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did18 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output32 {
    pub encoding: String,
    pub schema: Schema60,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema60 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties94,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties94 {
    pub signing_key: SigningKey,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SigningKey {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerResetPassword {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs60,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs60 {
    pub main: Main56,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main56 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input30,
    pub errors: Vec<Error14>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input30 {
    pub encoding: String,
    pub schema: Schema61,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema61 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties95,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties95 {
    pub token: Token5,
    pub password: Password6,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token5 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Password6 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error14 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerRevokeAppPassword {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs61,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs61 {
    pub main: Main57,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main57 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input31,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input31 {
    pub encoding: String,
    pub schema: Schema62,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema62 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties96,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties96 {
    pub name: Name6,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Name6 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoServerUpdateEmail {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs62,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs62 {
    pub main: Main58,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main58 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input32,
    pub errors: Vec<Error15>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input32 {
    pub encoding: String,
    pub schema: Schema63,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema63 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties97,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties97 {
    pub email: Email9,
    pub token: Token6,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Email9 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error15 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoSyncGetBlob {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs63,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs63 {
    pub main: Main59,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main59 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters14,
    pub output: Output33,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters14 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties98,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties98 {
    pub did: Did19,
    pub cid: Cid10,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did19 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid10 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output33 {
    pub encoding: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoSyncGetBlocks {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs64 {
    pub main: Main60,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main60 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters15,
    pub output: Output34,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters15 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties99,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties99 {
    pub did: Did20,
    pub cids: Cids,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did20 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cids {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items29,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items29 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output34 {
    pub encoding: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoSyncGetCheckout {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs65,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs65 {
    pub main: Main61,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main61 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters16,
    pub output: Output35,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters16 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties100,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties100 {
    pub did: Did21,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did21 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output35 {
    pub encoding: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoSyncGetHead {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs66,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs66 {
    pub main: Main62,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main62 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters17,
    pub output: Output36,
    pub errors: Vec<Error16>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters17 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties101,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties101 {
    pub did: Did22,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did22 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output36 {
    pub encoding: String,
    pub schema: Schema64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema64 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties102,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties102 {
    pub root: Root2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Root2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error16 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoSyncGetLatestCommit {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs67,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs67 {
    pub main: Main63,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main63 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters18,
    pub output: Output37,
    pub errors: Vec<Error17>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters18 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties103,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties103 {
    pub did: Did23,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did23 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output37 {
    pub encoding: String,
    pub schema: Schema65,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema65 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties104,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties104 {
    pub cid: Cid11,
    pub rev: Rev,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid11 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rev {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error17 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoSyncGetRecord {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs68,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs68 {
    pub main: Main64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main64 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters19,
    pub output: Output38,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters19 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties105,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties105 {
    pub did: Did24,
    pub collection: Collection9,
    pub rkey: Rkey8,
    pub commit: Commit,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did24 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection9 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rkey8 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output38 {
    pub encoding: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoSyncGetRepo {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs69,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs69 {
    pub main: Main65,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main65 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters20,
    pub output: Output39,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters20 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties106,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties106 {
    pub did: Did25,
    pub since: Since,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did25 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Since {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output39 {
    pub encoding: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoSyncListBlobs {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs70,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs70 {
    pub main: Main66,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main66 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters21,
    pub output: Output40,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters21 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties107,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties107 {
    pub did: Did26,
    pub since: Since2,
    pub limit: Limit5,
    pub cursor: Cursor10,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did26 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Since2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor10 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output40 {
    pub encoding: String,
    pub schema: Schema66,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema66 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties108,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties108 {
    pub cursor: Cursor11,
    pub cids: Cids2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor11 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cids2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items30,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items30 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoSyncListRepos {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs71,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs71 {
    pub main: Main67,
    pub repo: Repo8,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main67 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters22,
    pub output: Output41,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters22 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties109,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties109 {
    pub limit: Limit6,
    pub cursor: Cursor12,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor12 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output41 {
    pub encoding: String,
    pub schema: Schema67,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema67 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties110,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties110 {
    pub cursor: Cursor13,
    pub repos: Repos,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor13 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repos {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items31,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items31 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repo8 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties111,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties111 {
    pub did: Did27,
    pub head: Head,
    pub rev: Rev2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did27 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Head {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rev2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoSyncNotifyOfUpdate {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs72,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs72 {
    pub main: Main68,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main68 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input33,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input33 {
    pub encoding: String,
    pub schema: Schema68,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema68 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties112,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties112 {
    pub hostname: Hostname,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hostname {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoSyncRequestCrawl {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs73,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs73 {
    pub main: Main69,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main69 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input34,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input34 {
    pub encoding: String,
    pub schema: Schema69,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema69 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties113,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties113 {
    pub hostname: Hostname2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hostname2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoSyncSubscribeRepos {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs74,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs74 {
    pub main: Main70,
    pub commit: Commit2,
    pub identity: Identity,
    pub handle: Handle11,
    pub migrate: Migrate,
    pub tombstone: Tombstone,
    pub info: Info2,
    pub repo_op: RepoOp,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main70 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters23,
    pub message: Message3,
    pub errors: Vec<Error18>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters23 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties114,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties114 {
    pub cursor: Cursor14,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor14 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message3 {
    pub schema: Schema70,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema70 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error18 {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub nullable: Vec<String>,
    pub properties: Properties115,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties115 {
    pub seq: Seq2,
    pub rebase: Rebase,
    pub too_big: TooBig,
    pub repo: Repo9,
    pub commit: Commit3,
    pub prev: Prev,
    pub rev: Rev3,
    pub since: Since3,
    pub blocks: Blocks,
    pub ops: Ops,
    pub blobs: Blobs2,
    pub time: Time,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Seq2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rebase {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TooBig {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repo9 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prev {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rev3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Since3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blocks {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ops {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items32,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items32 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blobs2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items33,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items33 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Time {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties116,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties116 {
    pub seq: Seq3,
    pub did: Did28,
    pub time: Time2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Seq3 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did28 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Time2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handle11 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties117,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties117 {
    pub seq: Seq4,
    pub did: Did29,
    pub handle: Handle12,
    pub time: Time3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Seq4 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did29 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handle12 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Time3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Migrate {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub nullable: Vec<String>,
    pub properties: Properties118,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties118 {
    pub seq: Seq5,
    pub did: Did30,
    pub migrate_to: MigrateTo,
    pub time: Time4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Seq5 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did30 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrateTo {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Time4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tombstone {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties119,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties119 {
    pub seq: Seq6,
    pub did: Did31,
    pub time: Time5,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Seq6 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did31 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Time5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Info2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties120,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties120 {
    pub name: Name7,
    pub message: Message4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Name7 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub known_values: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message4 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoOp {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub nullable: Vec<String>,
    pub properties: Properties121,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties121 {
    pub action: Action,
    pub path: Path,
    pub cid: Cid12,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    #[serde(rename = "type")]
    pub type_field: String,
    pub known_values: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Path {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid12 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoTempCheckSignupQueue {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs75,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs75 {
    pub main: Main71,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main71 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub output: Output42,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output42 {
    pub encoding: String,
    pub schema: Schema71,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema71 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties122,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties122 {
    pub activated: Activated2,
    pub place_in_queue: PlaceInQueue,
    pub estimated_time_ms: EstimatedTimeMs,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activated2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceInQueue {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EstimatedTimeMs {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoTempFetchLabels {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs76,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs76 {
    pub main: Main72,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main72 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters24,
    pub output: Output43,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters24 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties123,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties123 {
    pub since: Since4,
    pub limit: Limit7,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Since4 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit7 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output43 {
    pub encoding: String,
    pub schema: Schema72,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema72 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties124,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties124 {
    pub labels: Labels4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items34,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items34 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComAtprotoTempRequestPhoneVerification {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs77,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs77 {
    pub main: Main73,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main73 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input35,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input35 {
    pub encoding: String,
    pub schema: Schema73,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema73 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties125,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties125 {
    pub phone_number: PhoneNumber,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneNumber {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyActorDefs {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs78,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs78 {
    pub profile_view_basic: ProfileViewBasic,
    pub profile_view: ProfileView,
    pub profile_view_detailed: ProfileViewDetailed,
    pub profile_associated: ProfileAssociated,
    pub viewer_state: ViewerState,
    pub preferences: Preferences,
    pub adult_content_pref: AdultContentPref,
    pub content_label_pref: ContentLabelPref,
    pub saved_feeds_pref: SavedFeedsPref,
    pub personal_details_pref: PersonalDetailsPref,
    pub feed_view_pref: FeedViewPref,
    pub thread_view_pref: ThreadViewPref,
    pub interests_pref: InterestsPref,
    pub muted_word_target: MutedWordTarget,
    pub muted_word: MutedWord,
    pub muted_words_pref: MutedWordsPref,
    pub hidden_posts_pref: HiddenPostsPref,
    pub labelers_pref: LabelersPref,
    pub labeler_pref_item: LabelerPrefItem,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileViewBasic {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties126,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties126 {
    pub did: Did32,
    pub handle: Handle13,
    pub display_name: DisplayName,
    pub avatar: Avatar,
    pub associated: Associated,
    pub viewer: Viewer,
    pub labels: Labels5,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did32 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handle13 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayName {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_graphemes: i64,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Avatar {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Associated {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewer {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items35,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items35 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties127,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties127 {
    pub did: Did33,
    pub handle: Handle14,
    pub display_name: DisplayName2,
    pub description: Description2,
    pub avatar: Avatar2,
    pub associated: Associated2,
    pub indexed_at: IndexedAt2,
    pub viewer: Viewer2,
    pub labels: Labels6,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did33 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handle14 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayName2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_graphemes: i64,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Description2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_graphemes: i64,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Avatar2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Associated2 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedAt2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewer2 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items36,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items36 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileViewDetailed {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties128,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties128 {
    pub did: Did34,
    pub handle: Handle15,
    pub display_name: DisplayName3,
    pub description: Description3,
    pub avatar: Avatar3,
    pub banner: Banner,
    pub followers_count: FollowersCount,
    pub follows_count: FollowsCount,
    pub posts_count: PostsCount,
    pub associated: Associated3,
    pub indexed_at: IndexedAt3,
    pub viewer: Viewer3,
    pub labels: Labels7,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did34 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handle15 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayName3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_graphemes: i64,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Description3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_graphemes: i64,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Avatar3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Banner {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowersCount {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowsCount {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostsCount {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Associated3 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedAt3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewer3 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels7 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items37,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items37 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileAssociated {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties129,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties129 {
    pub lists: Lists,
    pub feedgens: Feedgens,
    pub labeler: Labeler,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lists {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feedgens {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labeler {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewerState {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub properties: Properties130,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties130 {
    pub muted: Muted,
    pub muted_by_list: MutedByList,
    pub blocked_by: BlockedBy,
    pub blocking: Blocking,
    pub blocking_by_list: BlockingByList,
    pub following: Following,
    pub followed_by: FollowedBy,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Muted {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutedByList {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockedBy {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blocking {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockingByList {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Following {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowedBy {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items38,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items38 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdultContentPref {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties131,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties131 {
    pub enabled: Enabled,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Enabled {
    #[serde(rename = "type")]
    pub type_field: String,
    pub default: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentLabelPref {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties132,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties132 {
    pub labeler_did: LabelerDid,
    pub label: Label2,
    pub visibility: Visibility,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelerDid {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Label2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Visibility {
    #[serde(rename = "type")]
    pub type_field: String,
    pub known_values: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedFeedsPref {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties133,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties133 {
    pub pinned: Pinned,
    pub saved: Saved,
    pub timeline_index: TimelineIndex,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pinned {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items39,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items39 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items40,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items40 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineIndex {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalDetailsPref {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties134,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties134 {
    pub birth_date: BirthDate,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BirthDate {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedViewPref {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties135,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties135 {
    pub feed: Feed,
    pub hide_replies: HideReplies,
    pub hide_replies_by_unfollowed: HideRepliesByUnfollowed,
    pub hide_replies_by_like_count: HideRepliesByLikeCount,
    pub hide_reposts: HideReposts,
    pub hide_quote_posts: HideQuotePosts,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feed {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HideReplies {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HideRepliesByUnfollowed {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub default: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HideRepliesByLikeCount {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HideReposts {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HideQuotePosts {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadViewPref {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties136,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties136 {
    pub sort: Sort2,
    pub prioritize_followed_users: PrioritizeFollowedUsers,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sort2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub known_values: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrioritizeFollowedUsers {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterestsPref {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties137,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties137 {
    pub tags: Tags,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tags {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_length: i64,
    pub items: Items41,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items41 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_length: i64,
    pub max_graphemes: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutedWordTarget {
    #[serde(rename = "type")]
    pub type_field: String,
    pub known_values: Vec<String>,
    pub max_length: i64,
    pub max_graphemes: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutedWord {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties138,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties138 {
    pub value: Value5,
    pub targets: Targets,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Value5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub max_length: i64,
    pub max_graphemes: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Targets {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub items: Items42,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items42 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutedWordsPref {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties139,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties139 {
    pub items: Items43,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items43 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items44,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items44 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HiddenPostsPref {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties140,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties140 {
    pub items: Items45,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items45 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items46,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items46 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelersPref {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties141,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties141 {
    pub labelers: Labelers,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labelers {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items47,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items47 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelerPrefItem {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties142,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties142 {
    pub did: Did35,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did35 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyActorGetPreferences {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs79,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs79 {
    pub main: Main74,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main74 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters25,
    pub output: Output44,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters25 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties143,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties143 {}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output44 {
    pub encoding: String,
    pub schema: Schema74,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema74 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties144,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties144 {
    pub preferences: Preferences2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences2 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyActorGetProfile {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs80,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs80 {
    pub main: Main75,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main75 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters26,
    pub output: Output45,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters26 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties145,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties145 {
    pub actor: Actor,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output45 {
    pub encoding: String,
    pub schema: Schema75,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema75 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyActorGetProfiles {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs81,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs81 {
    pub main: Main76,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main76 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters27,
    pub output: Output46,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters27 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties146,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties146 {
    pub actors: Actors,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actors {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items48,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items48 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output46 {
    pub encoding: String,
    pub schema: Schema76,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema76 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties147,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties147 {
    pub profiles: Profiles,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profiles {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items49,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items49 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyActorGetSuggestions {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs82,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs82 {
    pub main: Main77,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main77 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters28,
    pub output: Output47,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters28 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties148,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties148 {
    pub limit: Limit8,
    pub cursor: Cursor15,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit8 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor15 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output47 {
    pub encoding: String,
    pub schema: Schema77,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema77 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties149,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties149 {
    pub cursor: Cursor16,
    pub actors: Actors2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor16 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actors2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items50,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items50 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyActorProfile {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs83,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs83 {
    pub main: Main78,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main78 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub key: String,
    pub record: Record4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties150,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties150 {
    pub display_name: DisplayName4,
    pub description: Description4,
    pub avatar: Avatar4,
    pub banner: Banner2,
    pub labels: Labels8,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayName4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_graphemes: i64,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Description4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub max_graphemes: i64,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Avatar4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub accept: Vec<String>,
    pub max_size: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Banner2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub accept: Vec<String>,
    pub max_size: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels8 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyActorPutPreferences {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs84,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs84 {
    pub main: Main79,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main79 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input36,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input36 {
    pub encoding: String,
    pub schema: Schema78,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema78 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties151,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties151 {
    pub preferences: Preferences3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences3 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyActorSearchActors {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs85,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs85 {
    pub main: Main80,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main80 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters29,
    pub output: Output48,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters29 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties152,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties152 {
    pub term: Term,
    pub q: Q,
    pub limit: Limit9,
    pub cursor: Cursor17,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Term {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Q {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit9 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor17 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output48 {
    pub encoding: String,
    pub schema: Schema79,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema79 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties153,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties153 {
    pub cursor: Cursor18,
    pub actors: Actors3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor18 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actors3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items51,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items51 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyActorSearchActorsTypeahead {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs86,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs86 {
    pub main: Main81,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main81 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters30,
    pub output: Output49,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters30 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties154,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties154 {
    pub term: Term2,
    pub q: Q2,
    pub viewer: Viewer4,
    pub limit: Limit10,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Term2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Q2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewer4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit10 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output49 {
    pub encoding: String,
    pub schema: Schema80,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema80 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties155,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties155 {
    pub actors: Actors4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actors4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items52,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items52 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyEmbedExternal {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs87,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs87 {
    pub main: Main82,
    pub external: External2,
    pub view: View,
    pub view_external: ViewExternal,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main82 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties156,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties156 {
    pub external: External,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct External {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct External2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties157,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties157 {
    pub uri: Uri8,
    pub title: Title,
    pub description: Description5,
    pub thumb: Thumb,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri8 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Title {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Description5 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thumb {
    #[serde(rename = "type")]
    pub type_field: String,
    pub accept: Vec<String>,
    pub max_size: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties158,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties158 {
    pub external: External3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct External3 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewExternal {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties159,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties159 {
    pub uri: Uri9,
    pub title: Title2,
    pub description: Description6,
    pub thumb: Thumb2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri9 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Title2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Description6 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thumb2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyEmbedImages {
    pub lexicon: i64,
    pub id: String,
    pub description: String,
    pub defs: Defs88,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs88 {
    pub main: Main83,
    pub image: Image,
    pub aspect_ratio: AspectRatio2,
    pub view: View2,
    pub view_image: ViewImage,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main83 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties160,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties160 {
    pub images: Images,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Images {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items53,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items53 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties161,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties161 {
    pub image: Image2,
    pub alt: Alt,
    pub aspect_ratio: AspectRatio,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub accept: Vec<String>,
    pub max_size: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alt {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AspectRatio {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AspectRatio2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties162,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties162 {
    pub width: Width,
    pub height: Height,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Width {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Height {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties163,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties163 {
    pub images: Images2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Images2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items54,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items54 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewImage {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties164,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties164 {
    pub thumb: Thumb3,
    pub fullsize: Fullsize,
    pub alt: Alt2,
    pub aspect_ratio: AspectRatio3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thumb3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fullsize {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alt2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AspectRatio3 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyEmbedRecord {
    pub lexicon: i64,
    pub id: String,
    pub description: String,
    pub defs: Defs89,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs89 {
    pub main: Main84,
    pub view: View3,
    pub view_record: ViewRecord,
    pub view_not_found: ViewNotFound,
    pub view_blocked: ViewBlocked,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main84 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties165,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties165 {
    pub record: Record5,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record5 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties166,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties166 {
    pub record: Record6,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewRecord {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties167,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties167 {
    pub uri: Uri10,
    pub cid: Cid13,
    pub author: Author,
    pub value: Value6,
    pub labels: Labels9,
    pub reply_count: ReplyCount,
    pub repost_count: RepostCount,
    pub like_count: LikeCount,
    pub embeds: Embeds,
    pub indexed_at: IndexedAt4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri10 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid13 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Author {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Value6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels9 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items55,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items55 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyCount {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepostCount {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LikeCount {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Embeds {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items56,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items56 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedAt4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewNotFound {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties168,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties168 {
    pub uri: Uri11,
    pub not_found: NotFound,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri11 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotFound {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "const")]
    pub const_field: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewBlocked {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties169,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties169 {
    pub uri: Uri12,
    pub blocked: Blocked,
    pub author: Author2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri12 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blocked {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "const")]
    pub const_field: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Author2 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyEmbedRecordWithMedia {
    pub lexicon: i64,
    pub id: String,
    pub description: String,
    pub defs: Defs90,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs90 {
    pub main: Main85,
    pub view: View4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main85 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties170,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties170 {
    pub record: Record7,
    pub media: Media,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record7 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties171,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties171 {
    pub record: Record8,
    pub media: Media2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record8 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Media2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedDefs {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs91,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs91 {
    pub post_view: PostView,
    pub viewer_state: ViewerState2,
    pub feed_view_post: FeedViewPost,
    pub reply_ref: ReplyRef,
    pub reason_repost: ReasonRepost,
    pub thread_view_post: ThreadViewPost,
    pub not_found_post: NotFoundPost,
    pub blocked_post: BlockedPost,
    pub blocked_author: BlockedAuthor,
    pub generator_view: GeneratorView,
    pub generator_viewer_state: GeneratorViewerState,
    pub skeleton_feed_post: SkeletonFeedPost,
    pub skeleton_reason_repost: SkeletonReasonRepost,
    pub threadgate_view: ThreadgateView,
    pub interaction: Interaction,
    pub request_less: RequestLess,
    pub request_more: RequestMore,
    pub clickthrough_item: ClickthroughItem,
    pub clickthrough_author: ClickthroughAuthor,
    pub clickthrough_reposter: ClickthroughReposter,
    pub clickthrough_embed: ClickthroughEmbed,
    pub interaction_seen: InteractionSeen,
    pub interaction_like: InteractionLike,
    pub interaction_repost: InteractionRepost,
    pub interaction_reply: InteractionReply,
    pub interaction_quote: InteractionQuote,
    pub interaction_share: InteractionShare,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostView {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties172,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties172 {
    pub uri: Uri13,
    pub cid: Cid14,
    pub author: Author3,
    pub record: Record9,
    pub embed: Embed,
    pub reply_count: ReplyCount2,
    pub repost_count: RepostCount2,
    pub like_count: LikeCount2,
    pub indexed_at: IndexedAt5,
    pub viewer: Viewer5,
    pub labels: Labels10,
    pub threadgate: Threadgate,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri13 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid14 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Author3 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record9 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Embed {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyCount2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepostCount2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LikeCount2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedAt5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewer5 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels10 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items57,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items57 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Threadgate {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewerState2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub properties: Properties173,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties173 {
    pub repost: Repost,
    pub like: Like,
    pub reply_disabled: ReplyDisabled,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repost {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Like {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyDisabled {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedViewPost {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties174,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties174 {
    pub post: Post,
    pub reply: Reply,
    pub reason: Reason3,
    pub feed_context: FeedContext,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Post {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reason3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedContext {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyRef {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties175,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties175 {
    pub root: Root3,
    pub parent: Parent,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Root3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parent {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasonRepost {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties176,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties176 {
    pub by: By,
    pub indexed_at: IndexedAt6,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct By {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedAt6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadViewPost {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties177,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties177 {
    pub post: Post2,
    pub parent: Parent2,
    pub replies: Replies,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Post2 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parent2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Replies {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items58,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items58 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotFoundPost {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties178,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties178 {
    pub uri: Uri14,
    pub not_found: NotFound2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri14 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotFound2 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "const")]
    pub const_field: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockedPost {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties179,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties179 {
    pub uri: Uri15,
    pub blocked: Blocked2,
    pub author: Author4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri15 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blocked2 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "const")]
    pub const_field: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Author4 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockedAuthor {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties180,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties180 {
    pub did: Did36,
    pub viewer: Viewer6,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did36 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewer6 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratorView {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties181,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties181 {
    pub uri: Uri16,
    pub cid: Cid15,
    pub did: Did37,
    pub creator: Creator,
    pub display_name: DisplayName5,
    pub description: Description7,
    pub description_facets: DescriptionFacets,
    pub avatar: Avatar5,
    pub like_count: LikeCount3,
    pub accepts_interactions: AcceptsInteractions,
    pub labels: Labels11,
    pub viewer: Viewer7,
    pub indexed_at: IndexedAt7,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri16 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid15 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did37 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Creator {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayName5 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Description7 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_graphemes: i64,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescriptionFacets {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items59,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items59 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Avatar5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LikeCount3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptsInteractions {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels11 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items60,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items60 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewer7 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedAt7 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratorViewerState {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties182,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties182 {
    pub like: Like2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Like2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkeletonFeedPost {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties183,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties183 {
    pub post: Post3,
    pub reason: Reason4,
    pub feed_context: FeedContext2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Post3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reason4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedContext2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkeletonReasonRepost {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties184,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties184 {
    pub repost: Repost2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repost2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadgateView {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties185,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties185 {
    pub uri: Uri17,
    pub cid: Cid16,
    pub record: Record10,
    pub lists: Lists2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri17 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid16 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record10 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lists2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items61,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items61 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Interaction {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties186,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties186 {
    pub item: Item,
    pub event: Event,
    pub feed_context: FeedContext3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    #[serde(rename = "type")]
    pub type_field: String,
    pub known_values: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedContext3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestLess {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestMore {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClickthroughItem {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClickthroughAuthor {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClickthroughReposter {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClickthroughEmbed {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionSeen {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionLike {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionRepost {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionReply {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionQuote {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionShare {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedDescribeFeedGenerator {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs92,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs92 {
    pub main: Main86,
    pub feed: Feed2,
    pub links: Links4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main86 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub output: Output50,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output50 {
    pub encoding: String,
    pub schema: Schema81,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema81 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties187,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties187 {
    pub did: Did38,
    pub feeds: Feeds,
    pub links: Links3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did38 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feeds {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items62,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items62 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Links3 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feed2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties188,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties188 {
    pub uri: Uri18,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri18 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Links4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties189,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties189 {
    pub privacy_policy: PrivacyPolicy2,
    pub terms_of_service: TermsOfService2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyPolicy2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TermsOfService2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedGenerator {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs93,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs93 {
    pub main: Main87,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main87 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub key: String,
    pub record: Record11,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record11 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties190,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties190 {
    pub did: Did39,
    pub display_name: DisplayName6,
    pub description: Description8,
    pub description_facets: DescriptionFacets2,
    pub avatar: Avatar6,
    pub accepts_interactions: AcceptsInteractions2,
    pub labels: Labels12,
    pub created_at: CreatedAt5,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did39 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayName6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_graphemes: i64,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Description8 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_graphemes: i64,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescriptionFacets2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items63,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items63 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Avatar6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub accept: Vec<String>,
    pub max_size: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptsInteractions2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels12 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAt5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedGetActorFeeds {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs94,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs94 {
    pub main: Main88,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main88 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters31,
    pub output: Output51,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters31 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties191,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties191 {
    pub actor: Actor2,
    pub limit: Limit11,
    pub cursor: Cursor19,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit11 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor19 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output51 {
    pub encoding: String,
    pub schema: Schema82,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema82 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties192,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties192 {
    pub cursor: Cursor20,
    pub feeds: Feeds2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor20 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feeds2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items64 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedGetActorLikes {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs95,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs95 {
    pub main: Main89,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main89 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters32,
    pub output: Output52,
    pub errors: Vec<Error19>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters32 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties193,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties193 {
    pub actor: Actor3,
    pub limit: Limit12,
    pub cursor: Cursor21,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit12 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor21 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output52 {
    pub encoding: String,
    pub schema: Schema83,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema83 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties194,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties194 {
    pub cursor: Cursor22,
    pub feed: Feed3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor22 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feed3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items65,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items65 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error19 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedGetAuthorFeed {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs96,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs96 {
    pub main: Main90,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main90 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters33,
    pub output: Output53,
    pub errors: Vec<Error20>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters33 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties195,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties195 {
    pub actor: Actor4,
    pub limit: Limit13,
    pub cursor: Cursor23,
    pub filter: Filter,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit13 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor23 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub known_values: Vec<String>,
    pub default: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output53 {
    pub encoding: String,
    pub schema: Schema84,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema84 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties196,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties196 {
    pub cursor: Cursor24,
    pub feed: Feed4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor24 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feed4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items66,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items66 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error20 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedGetFeed {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs97,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs97 {
    pub main: Main91,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main91 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters34,
    pub output: Output54,
    pub errors: Vec<Error21>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters34 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties197,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties197 {
    pub feed: Feed5,
    pub limit: Limit14,
    pub cursor: Cursor25,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feed5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit14 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor25 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output54 {
    pub encoding: String,
    pub schema: Schema85,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema85 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties198,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties198 {
    pub cursor: Cursor26,
    pub feed: Feed6,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor26 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feed6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items67,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items67 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error21 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedGetFeedGenerator {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs98,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs98 {
    pub main: Main92,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main92 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters35,
    pub output: Output55,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters35 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties199,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties199 {
    pub feed: Feed7,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feed7 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output55 {
    pub encoding: String,
    pub schema: Schema86,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema86 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties200,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties200 {
    pub view: View5,
    pub is_online: IsOnline,
    pub is_valid: IsValid,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View5 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IsOnline {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IsValid {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedGetFeedGenerators {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs99,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs99 {
    pub main: Main93,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main93 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters36,
    pub output: Output56,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters36 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties201,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties201 {
    pub feeds: Feeds3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feeds3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items68,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items68 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output56 {
    pub encoding: String,
    pub schema: Schema87,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema87 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties202,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties202 {
    pub feeds: Feeds4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feeds4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items69,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items69 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedGetFeedSkeleton {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs100,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs100 {
    pub main: Main94,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main94 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters37,
    pub output: Output57,
    pub errors: Vec<Error22>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters37 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties203,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties203 {
    pub feed: Feed8,
    pub limit: Limit15,
    pub cursor: Cursor27,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feed8 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit15 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor27 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output57 {
    pub encoding: String,
    pub schema: Schema88,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema88 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties204,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties204 {
    pub cursor: Cursor28,
    pub feed: Feed9,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor28 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feed9 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items70,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items70 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error22 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedGetLikes {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs101,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs101 {
    pub main: Main95,
    pub like: Like3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main95 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters38,
    pub output: Output58,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters38 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties205,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties205 {
    pub uri: Uri19,
    pub cid: Cid17,
    pub limit: Limit16,
    pub cursor: Cursor29,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri19 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid17 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit16 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor29 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output58 {
    pub encoding: String,
    pub schema: Schema89,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema89 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties206,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties206 {
    pub uri: Uri20,
    pub cid: Cid18,
    pub cursor: Cursor30,
    pub likes: Likes,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri20 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid18 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor30 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Likes {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items71,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items71 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Like3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties207,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties207 {
    pub indexed_at: IndexedAt8,
    pub created_at: CreatedAt6,
    pub actor: Actor5,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedAt8 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAt6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor5 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedGetListFeed {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs102,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs102 {
    pub main: Main96,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main96 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters39,
    pub output: Output59,
    pub errors: Vec<Error23>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters39 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties208,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties208 {
    pub list: List,
    pub limit: Limit17,
    pub cursor: Cursor31,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct List {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit17 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor31 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output59 {
    pub encoding: String,
    pub schema: Schema90,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema90 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties209,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties209 {
    pub cursor: Cursor32,
    pub feed: Feed10,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor32 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feed10 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items72,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items72 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error23 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedGetPostThread {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs103,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs103 {
    pub main: Main97,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main97 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters40,
    pub output: Output60,
    pub errors: Vec<Error24>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters40 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties210,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties210 {
    pub uri: Uri21,
    pub depth: Depth,
    pub parent_height: ParentHeight,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri21 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Depth {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub default: i64,
    pub minimum: i64,
    pub maximum: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParentHeight {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub default: i64,
    pub minimum: i64,
    pub maximum: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output60 {
    pub encoding: String,
    pub schema: Schema91,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema91 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties211,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties211 {
    pub thread: Thread,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error24 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedGetPosts {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs104,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs104 {
    pub main: Main98,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main98 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters41,
    pub output: Output61,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters41 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties212,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties212 {
    pub uris: Uris,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uris {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub items: Items73,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items73 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output61 {
    pub encoding: String,
    pub schema: Schema92,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema92 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties213,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties213 {
    pub posts: Posts,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Posts {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items74,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items74 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedGetRepostedBy {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs105,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs105 {
    pub main: Main99,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main99 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters42,
    pub output: Output62,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters42 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties214,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties214 {
    pub uri: Uri22,
    pub cid: Cid19,
    pub limit: Limit18,
    pub cursor: Cursor33,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri22 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid19 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit18 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor33 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output62 {
    pub encoding: String,
    pub schema: Schema93,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema93 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties215,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties215 {
    pub uri: Uri23,
    pub cid: Cid20,
    pub cursor: Cursor34,
    pub reposted_by: RepostedBy,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri23 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid20 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor34 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepostedBy {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items75,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items75 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedGetSuggestedFeeds {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs106,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs106 {
    pub main: Main100,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main100 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters43,
    pub output: Output63,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters43 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties216,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties216 {
    pub limit: Limit19,
    pub cursor: Cursor35,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit19 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor35 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output63 {
    pub encoding: String,
    pub schema: Schema94,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema94 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties217,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties217 {
    pub cursor: Cursor36,
    pub feeds: Feeds5,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor36 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feeds5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items76,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items76 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedGetTimeline {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs107,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs107 {
    pub main: Main101,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main101 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters44,
    pub output: Output64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters44 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties218,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties218 {
    pub algorithm: Algorithm,
    pub limit: Limit20,
    pub cursor: Cursor37,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Algorithm {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit20 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor37 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output64 {
    pub encoding: String,
    pub schema: Schema95,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema95 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties219,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties219 {
    pub cursor: Cursor38,
    pub feed: Feed11,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor38 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feed11 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items77,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items77 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedLike {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs108,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs108 {
    pub main: Main102,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main102 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub key: String,
    pub record: Record12,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record12 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties220,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties220 {
    pub subject: Subject7,
    pub created_at: CreatedAt7,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject7 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAt7 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedPost {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs109,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs109 {
    pub main: Main103,
    pub reply_ref: ReplyRef2,
    pub entity: Entity,
    pub text_slice: TextSlice,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main103 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub key: String,
    pub record: Record13,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record13 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties221,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties221 {
    pub text: Text,
    pub entities: Entities,
    pub facets: Facets,
    pub reply: Reply2,
    pub embed: Embed2,
    pub langs: Langs,
    pub labels: Labels13,
    pub tags: Tags2,
    pub created_at: CreatedAt8,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Text {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_length: i64,
    pub max_graphemes: i64,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entities {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub items: Items78,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items78 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Facets {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub items: Items79,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items79 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reply2 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Embed2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Langs {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub max_length: i64,
    pub items: Items80,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items80 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels13 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tags2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub max_length: i64,
    pub items: Items81,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items81 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_length: i64,
    pub max_graphemes: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAt8 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyRef2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties222,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties222 {
    pub root: Root4,
    pub parent: Parent3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Root4 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parent3 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entity {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties223,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties223 {
    pub index: Index,
    #[serde(rename = "type")]
    pub type_field: Type,
    pub value: Value7,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Index {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Type {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Value7 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSlice {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties224,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties224 {
    pub start: Start,
    pub end: End,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Start {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct End {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedRepost {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs110,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs110 {
    pub main: Main104,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main104 {
    pub description: String,
    #[serde(rename = "type")]
    pub type_field: String,
    pub key: String,
    pub record: Record14,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record14 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties225,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties225 {
    pub subject: Subject8,
    pub created_at: CreatedAt9,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject8 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAt9 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedSearchPosts {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs111,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs111 {
    pub main: Main105,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main105 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters45,
    pub output: Output65,
    pub errors: Vec<Error25>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters45 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties226,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties226 {
    pub q: Q3,
    pub sort: Sort3,
    pub since: Since5,
    pub until: Until,
    pub mentions: Mentions,
    pub author: Author5,
    pub lang: Lang2,
    pub domain: Domain,
    pub url: Url,
    pub tag: Tag,
    pub limit: Limit21,
    pub cursor: Cursor39,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Q3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sort3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub known_values: Vec<String>,
    pub default: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Since5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Until {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mentions {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Author5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lang2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Domain {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Url {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items82,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items82 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_length: i64,
    pub max_graphemes: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit21 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor39 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output65 {
    pub encoding: String,
    pub schema: Schema96,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema96 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties227,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties227 {
    pub cursor: Cursor40,
    pub hits_total: HitsTotal,
    pub posts: Posts2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor40 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HitsTotal {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Posts2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items83,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items83 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error25 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedSendInteractions {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs112,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs112 {
    pub main: Main106,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main106 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input37,
    pub output: Output66,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input37 {
    pub encoding: String,
    pub schema: Schema97,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema97 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties228,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties228 {
    pub interactions: Interactions,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Interactions {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items84,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items84 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output66 {
    pub encoding: String,
    pub schema: Schema98,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema98 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties229,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties229 {}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyFeedThreadgate {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs113,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs113 {
    pub main: Main107,
    pub mention_rule: MentionRule,
    pub following_rule: FollowingRule,
    pub list_rule: ListRule,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main107 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub key: String,
    pub description: String,
    pub record: Record15,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record15 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties230,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties230 {
    pub post: Post4,
    pub allow: Allow,
    pub created_at: CreatedAt10,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Post4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Allow {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_length: i64,
    pub items: Items85,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items85 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAt10 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MentionRule {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub properties: Properties231,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties231 {}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowingRule {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub properties: Properties232,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties232 {}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRule {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties233,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties233 {
    pub list: List2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct List2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyGraphBlock {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs114,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs114 {
    pub main: Main108,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main108 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub key: String,
    pub record: Record16,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record16 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties234,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties234 {
    pub subject: Subject9,
    pub created_at: CreatedAt11,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject9 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAt11 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyGraphDefs {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs115,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs115 {
    pub list_view_basic: ListViewBasic,
    pub list_view: ListView,
    pub list_item_view: ListItemView,
    pub list_purpose: ListPurpose,
    pub modlist: Modlist,
    pub curatelist: Curatelist,
    pub list_viewer_state: ListViewerState,
    pub not_found_actor: NotFoundActor,
    pub relationship: Relationship,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListViewBasic {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties235,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties235 {
    pub uri: Uri24,
    pub cid: Cid21,
    pub name: Name8,
    pub purpose: Purpose,
    pub avatar: Avatar7,
    pub labels: Labels14,
    pub viewer: Viewer8,
    pub indexed_at: IndexedAt9,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri24 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid21 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Name8 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_length: i64,
    pub min_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Purpose {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Avatar7 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels14 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items86,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items86 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewer8 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedAt9 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListView {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties236,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties236 {
    pub uri: Uri25,
    pub cid: Cid22,
    pub creator: Creator2,
    pub name: Name9,
    pub purpose: Purpose2,
    pub description: Description9,
    pub description_facets: DescriptionFacets3,
    pub avatar: Avatar8,
    pub labels: Labels15,
    pub viewer: Viewer9,
    pub indexed_at: IndexedAt10,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri25 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid22 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Creator2 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Name9 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_length: i64,
    pub min_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Purpose2 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Description9 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_graphemes: i64,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescriptionFacets3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items87,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items87 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Avatar8 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels15 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items88,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items88 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewer9 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedAt10 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListItemView {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties237,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties237 {
    pub uri: Uri26,
    pub subject: Subject10,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri26 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject10 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPurpose {
    #[serde(rename = "type")]
    pub type_field: String,
    pub known_values: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Modlist {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Curatelist {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListViewerState {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties238,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties238 {
    pub muted: Muted2,
    pub blocked: Blocked3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Muted2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blocked3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotFoundActor {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties239,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties239 {
    pub actor: Actor6,
    pub not_found: NotFound3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotFound3 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "const")]
    pub const_field: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relationship {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties240,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties240 {
    pub did: Did40,
    pub following: Following2,
    pub followed_by: FollowedBy2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did40 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Following2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowedBy2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyGraphFollow {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs116,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs116 {
    pub main: Main109,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main109 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub key: String,
    pub record: Record17,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record17 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties241,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties241 {
    pub subject: Subject11,
    pub created_at: CreatedAt12,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject11 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAt12 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyGraphGetBlocks {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs117,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs117 {
    pub main: Main110,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main110 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters46,
    pub output: Output67,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters46 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties242,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties242 {
    pub limit: Limit22,
    pub cursor: Cursor41,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit22 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor41 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output67 {
    pub encoding: String,
    pub schema: Schema99,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema99 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties243,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties243 {
    pub cursor: Cursor42,
    pub blocks: Blocks2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor42 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blocks2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items89,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items89 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyGraphGetFollowers {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs118,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs118 {
    pub main: Main111,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main111 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters47,
    pub output: Output68,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters47 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties244,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties244 {
    pub actor: Actor7,
    pub limit: Limit23,
    pub cursor: Cursor43,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor7 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit23 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor43 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output68 {
    pub encoding: String,
    pub schema: Schema100,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema100 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties245,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties245 {
    pub subject: Subject12,
    pub cursor: Cursor44,
    pub followers: Followers,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject12 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor44 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Followers {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items90,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items90 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyGraphGetFollows {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs119,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs119 {
    pub main: Main112,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main112 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters48,
    pub output: Output69,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters48 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties246,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties246 {
    pub actor: Actor8,
    pub limit: Limit24,
    pub cursor: Cursor45,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor8 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit24 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor45 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output69 {
    pub encoding: String,
    pub schema: Schema101,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema101 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties247,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties247 {
    pub subject: Subject13,
    pub cursor: Cursor46,
    pub follows: Follows,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject13 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor46 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Follows {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items91,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items91 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyGraphGetList {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs120,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs120 {
    pub main: Main113,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main113 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters49,
    pub output: Output70,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters49 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties248,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties248 {
    pub list: List3,
    pub limit: Limit25,
    pub cursor: Cursor47,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct List3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit25 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor47 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output70 {
    pub encoding: String,
    pub schema: Schema102,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema102 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties249,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties249 {
    pub cursor: Cursor48,
    pub list: List4,
    pub items: Items92,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor48 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct List4 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items92 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items93,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items93 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyGraphGetListBlocks {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs121,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs121 {
    pub main: Main114,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main114 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters50,
    pub output: Output71,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters50 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties250,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties250 {
    pub limit: Limit26,
    pub cursor: Cursor49,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit26 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor49 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output71 {
    pub encoding: String,
    pub schema: Schema103,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema103 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties251,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties251 {
    pub cursor: Cursor50,
    pub lists: Lists3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor50 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lists3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items94,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items94 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyGraphGetListMutes {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs122,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs122 {
    pub main: Main115,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main115 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters51,
    pub output: Output72,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters51 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties252,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties252 {
    pub limit: Limit27,
    pub cursor: Cursor51,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit27 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor51 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output72 {
    pub encoding: String,
    pub schema: Schema104,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema104 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties253,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties253 {
    pub cursor: Cursor52,
    pub lists: Lists4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor52 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lists4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items95,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items95 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyGraphGetLists {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs123,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs123 {
    pub main: Main116,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main116 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters52,
    pub output: Output73,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters52 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties254,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties254 {
    pub actor: Actor9,
    pub limit: Limit28,
    pub cursor: Cursor53,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor9 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit28 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor53 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output73 {
    pub encoding: String,
    pub schema: Schema105,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema105 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties255,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties255 {
    pub cursor: Cursor54,
    pub lists: Lists5,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor54 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lists5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items96,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items96 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyGraphGetMutes {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs124,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs124 {
    pub main: Main117,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main117 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters53,
    pub output: Output74,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters53 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties256,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties256 {
    pub limit: Limit29,
    pub cursor: Cursor55,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit29 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor55 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output74 {
    pub encoding: String,
    pub schema: Schema106,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema106 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties257,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties257 {
    pub cursor: Cursor56,
    pub mutes: Mutes,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor56 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mutes {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items97,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items97 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyGraphGetRelationships {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs125,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs125 {
    pub main: Main118,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main118 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters54,
    pub output: Output75,
    pub errors: Vec<Error26>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters54 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties258,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties258 {
    pub actor: Actor10,
    pub others: Others,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor10 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Others {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub max_length: i64,
    pub items: Items98,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items98 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output75 {
    pub encoding: String,
    pub schema: Schema107,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema107 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties259,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties259 {
    pub actor: Actor11,
    pub relationships: Relationships,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor11 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relationships {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items99,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items99 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error26 {
    pub name: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyGraphGetSuggestedFollowsByActor {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs126,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs126 {
    pub main: Main119,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main119 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters55,
    pub output: Output76,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters55 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties260,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties260 {
    pub actor: Actor12,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor12 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output76 {
    pub encoding: String,
    pub schema: Schema108,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema108 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties261,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties261 {
    pub suggestions: Suggestions,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestions {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items100,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items100 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyGraphList {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs127,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs127 {
    pub main: Main120,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main120 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub key: String,
    pub record: Record18,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record18 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties262,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties262 {
    pub purpose: Purpose3,
    pub name: Name10,
    pub description: Description10,
    pub description_facets: DescriptionFacets4,
    pub avatar: Avatar9,
    pub labels: Labels16,
    pub created_at: CreatedAt13,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Purpose3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Name10 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_length: i64,
    pub min_length: i64,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Description10 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_graphemes: i64,
    pub max_length: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescriptionFacets4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items101,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items101 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Avatar9 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub accept: Vec<String>,
    pub max_size: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels16 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAt13 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyGraphListblock {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs128,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs128 {
    pub main: Main121,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main121 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub key: String,
    pub record: Record19,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record19 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties263,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties263 {
    pub subject: Subject14,
    pub created_at: CreatedAt14,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject14 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAt14 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyGraphListitem {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs129,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs129 {
    pub main: Main122,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main122 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub key: String,
    pub record: Record20,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record20 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties264,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties264 {
    pub subject: Subject15,
    pub list: List5,
    pub created_at: CreatedAt15,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject15 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct List5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAt15 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyGraphMuteActor {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs130,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs130 {
    pub main: Main123,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main123 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input38,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input38 {
    pub encoding: String,
    pub schema: Schema109,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema109 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties265,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties265 {
    pub actor: Actor13,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor13 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyGraphMuteActorList {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs131,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs131 {
    pub main: Main124,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main124 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input39,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input39 {
    pub encoding: String,
    pub schema: Schema110,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema110 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties266,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties266 {
    pub list: List6,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct List6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyGraphUnmuteActor {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs132,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs132 {
    pub main: Main125,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main125 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input40,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input40 {
    pub encoding: String,
    pub schema: Schema111,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema111 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties267,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties267 {
    pub actor: Actor14,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor14 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyGraphUnmuteActorList {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs133,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs133 {
    pub main: Main126,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main126 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input41,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input41 {
    pub encoding: String,
    pub schema: Schema112,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema112 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties268,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties268 {
    pub list: List7,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct List7 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyLabelerDefs {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs134,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs134 {
    pub labeler_view: LabelerView,
    pub labeler_view_detailed: LabelerViewDetailed,
    pub labeler_viewer_state: LabelerViewerState,
    pub labeler_policies: LabelerPolicies,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelerView {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties269,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties269 {
    pub uri: Uri27,
    pub cid: Cid23,
    pub creator: Creator3,
    pub like_count: LikeCount4,
    pub viewer: Viewer10,
    pub indexed_at: IndexedAt11,
    pub labels: Labels17,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri27 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid23 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Creator3 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LikeCount4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewer10 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedAt11 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels17 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items102,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items102 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelerViewDetailed {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties270,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties270 {
    pub uri: Uri28,
    pub cid: Cid24,
    pub creator: Creator4,
    pub policies: Policies,
    pub like_count: LikeCount5,
    pub viewer: Viewer11,
    pub indexed_at: IndexedAt12,
    pub labels: Labels18,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri28 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid24 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Creator4 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Policies {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LikeCount5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewer11 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedAt12 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels18 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items103,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items103 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelerViewerState {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties271,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties271 {
    pub like: Like4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Like4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelerPolicies {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties272,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties272 {
    pub label_values: LabelValues,
    pub label_value_definitions: LabelValueDefinitions,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelValues {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub items: Items104,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items104 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelValueDefinitions {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub items: Items105,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items105 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyLabelerGetServices {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs135,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs135 {
    pub main: Main127,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main127 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters56,
    pub output: Output77,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters56 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties273,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties273 {
    pub dids: Dids2,
    pub detailed: Detailed,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dids2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items106,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items106 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Detailed {
    #[serde(rename = "type")]
    pub type_field: String,
    pub default: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output77 {
    pub encoding: String,
    pub schema: Schema113,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema113 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties274,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties274 {
    pub views: Views,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Views {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items107,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items107 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyLabelerService {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs136,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs136 {
    pub main: Main128,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main128 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub key: String,
    pub record: Record21,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record21 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties275,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties275 {
    pub policies: Policies2,
    pub labels: Labels19,
    pub created_at: CreatedAt16,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Policies2 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels19 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAt16 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyNotificationGetUnreadCount {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs137,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs137 {
    pub main: Main129,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main129 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters57,
    pub output: Output78,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters57 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties276,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties276 {
    pub seen_at: SeenAt,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeenAt {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output78 {
    pub encoding: String,
    pub schema: Schema114,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema114 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties277,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties277 {
    pub count: Count,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Count {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyNotificationListNotifications {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs138,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs138 {
    pub main: Main130,
    pub notification: Notification,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main130 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters58,
    pub output: Output79,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters58 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties278,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties278 {
    pub limit: Limit30,
    pub cursor: Cursor57,
    pub seen_at: SeenAt2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit30 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor57 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeenAt2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output79 {
    pub encoding: String,
    pub schema: Schema115,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema115 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties279,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties279 {
    pub cursor: Cursor58,
    pub notifications: Notifications,
    pub seen_at: SeenAt3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor58 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notifications {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items108,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items108 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeenAt3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties280,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties280 {
    pub uri: Uri29,
    pub cid: Cid25,
    pub author: Author6,
    pub reason: Reason5,
    pub reason_subject: ReasonSubject,
    pub record: Record22,
    pub is_read: IsRead,
    pub indexed_at: IndexedAt13,
    pub labels: Labels20,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri29 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid25 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Author6 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reason5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub known_values: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasonSubject {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record22 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IsRead {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedAt13 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels20 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items109,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items109 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyNotificationRegisterPush {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs139,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs139 {
    pub main: Main131,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main131 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input42,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input42 {
    pub encoding: String,
    pub schema: Schema116,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema116 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties281,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties281 {
    pub service_did: ServiceDid,
    pub token: Token7,
    pub platform: Platform,
    pub app_id: AppId,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceDid {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token7 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Platform {
    #[serde(rename = "type")]
    pub type_field: String,
    pub known_values: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppId {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyNotificationUpdateSeen {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs140,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs140 {
    pub main: Main132,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main132 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input43,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input43 {
    pub encoding: String,
    pub schema: Schema117,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema117 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties282,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties282 {
    pub seen_at: SeenAt4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeenAt4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyRichtextFacet {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs141,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs141 {
    pub main: Main133,
    pub mention: Mention,
    pub link: Link,
    pub tag: Tag2,
    pub byte_slice: ByteSlice,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main133 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties283,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties283 {
    pub index: Index2,
    pub features: Features,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Index2 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Features {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items110,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items110 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mention {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties284,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties284 {
    pub did: Did41,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did41 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties285,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties285 {
    pub uri: Uri30,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri30 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties286,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties286 {
    pub tag: Tag3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_length: i64,
    pub max_graphemes: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ByteSlice {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties287,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties287 {
    pub byte_start: ByteStart,
    pub byte_end: ByteEnd,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ByteStart {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ByteEnd {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyUnspeccedDefs {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs142,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs142 {
    pub skeleton_search_post: SkeletonSearchPost,
    pub skeleton_search_actor: SkeletonSearchActor,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkeletonSearchPost {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties288,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties288 {
    pub uri: Uri31,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri31 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkeletonSearchActor {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties289,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties289 {
    pub did: Did42,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did42 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyUnspeccedGetPopularFeedGenerators {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs143,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs143 {
    pub main: Main134,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main134 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters59,
    pub output: Output80,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters59 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties290,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties290 {
    pub limit: Limit31,
    pub cursor: Cursor59,
    pub query: Query,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit31 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor59 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Query {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output80 {
    pub encoding: String,
    pub schema: Schema118,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema118 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties291,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties291 {
    pub cursor: Cursor60,
    pub feeds: Feeds6,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor60 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feeds6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items111,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items111 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyUnspeccedGetTaggedSuggestions {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs144,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs144 {
    pub main: Main135,
    pub suggestion: Suggestion,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main135 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters60,
    pub output: Output81,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters60 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties292,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties292 {}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output81 {
    pub encoding: String,
    pub schema: Schema119,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema119 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties293,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties293 {
    pub suggestions: Suggestions2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestions2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items112,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items112 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties294,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties294 {
    pub tag: Tag4,
    pub subject_type: SubjectType,
    pub subject: Subject16,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag4 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectType {
    #[serde(rename = "type")]
    pub type_field: String,
    pub known_values: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject16 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyUnspeccedSearchActorsSkeleton {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs145,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs145 {
    pub main: Main136,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main136 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters61,
    pub output: Output82,
    pub errors: Vec<Error27>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters61 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties295,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties295 {
    pub q: Q4,
    pub viewer: Viewer12,
    pub typeahead: Typeahead,
    pub limit: Limit32,
    pub cursor: Cursor61,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Q4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewer12 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Typeahead {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit32 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor61 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output82 {
    pub encoding: String,
    pub schema: Schema120,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema120 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties296,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties296 {
    pub cursor: Cursor62,
    pub hits_total: HitsTotal2,
    pub actors: Actors5,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor62 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HitsTotal2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actors5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items113,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items113 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error27 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBskyUnspeccedSearchPostsSkeleton {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs146,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs146 {
    pub main: Main137,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main137 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters62,
    pub output: Output83,
    pub errors: Vec<Error28>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters62 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties297,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties297 {
    pub q: Q5,
    pub sort: Sort4,
    pub since: Since6,
    pub until: Until2,
    pub mentions: Mentions2,
    pub author: Author7,
    pub lang: Lang3,
    pub domain: Domain2,
    pub url: Url2,
    pub tag: Tag5,
    pub viewer: Viewer13,
    pub limit: Limit33,
    pub cursor: Cursor63,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Q5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sort4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub known_values: Vec<String>,
    pub default: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Since6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Until2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mentions2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Author7 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lang3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Domain2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Url2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items114,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items114 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub max_length: i64,
    pub max_graphemes: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewer13 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit33 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor63 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output83 {
    pub encoding: String,
    pub schema: Schema121,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema121 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties298,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties298 {
    pub cursor: Cursor64,
    pub hits_total: HitsTotal3,
    pub posts: Posts3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor64 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HitsTotal3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Posts3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items115,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items115 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error28 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsOzoneCommunicationCreateTemplate {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs147,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs147 {
    pub main: Main138,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main138 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input44,
    pub output: Output84,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input44 {
    pub encoding: String,
    pub schema: Schema122,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema122 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties299,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties299 {
    pub name: Name11,
    pub content_markdown: ContentMarkdown,
    pub subject: Subject17,
    pub created_by: CreatedBy2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Name11 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentMarkdown {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject17 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedBy2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output84 {
    pub encoding: String,
    pub schema: Schema123,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema123 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsOzoneCommunicationDefs {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs148,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs148 {
    pub template_view: TemplateView,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateView {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties300,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties300 {
    pub id: Id2,
    pub name: Name12,
    pub subject: Subject18,
    pub content_markdown: ContentMarkdown2,
    pub disabled: Disabled2,
    pub last_updated_by: LastUpdatedBy,
    pub created_at: CreatedAt17,
    pub updated_at: UpdatedAt,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Id2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Name12 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject18 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentMarkdown2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Disabled2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastUpdatedBy {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAt17 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatedAt {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsOzoneCommunicationDeleteTemplate {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs149,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs149 {
    pub main: Main139,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main139 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input45,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input45 {
    pub encoding: String,
    pub schema: Schema124,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema124 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties301,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties301 {
    pub id: Id3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Id3 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsOzoneCommunicationListTemplates {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs150,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs150 {
    pub main: Main140,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main140 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub output: Output85,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output85 {
    pub encoding: String,
    pub schema: Schema125,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema125 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties302,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties302 {
    pub communication_templates: CommunicationTemplates,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommunicationTemplates {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items116,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items116 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsOzoneCommunicationUpdateTemplate {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs151,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs151 {
    pub main: Main141,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main141 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input46,
    pub output: Output86,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input46 {
    pub encoding: String,
    pub schema: Schema126,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema126 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties303,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties303 {
    pub id: Id4,
    pub name: Name13,
    pub content_markdown: ContentMarkdown3,
    pub subject: Subject19,
    pub updated_by: UpdatedBy,
    pub disabled: Disabled3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Id4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Name13 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentMarkdown3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject19 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatedBy {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Disabled3 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output86 {
    pub encoding: String,
    pub schema: Schema127,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema127 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsOzoneModerationDefs {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs152,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs152 {
    pub mod_event_view: ModEventView,
    pub mod_event_view_detail: ModEventViewDetail,
    pub subject_status_view: SubjectStatusView,
    pub subject_review_state: SubjectReviewState,
    pub review_open: ReviewOpen,
    pub review_escalated: ReviewEscalated,
    pub review_closed: ReviewClosed,
    pub review_none: ReviewNone,
    pub mod_event_takedown: ModEventTakedown,
    pub mod_event_reverse_takedown: ModEventReverseTakedown,
    pub mod_event_resolve_appeal: ModEventResolveAppeal,
    pub mod_event_comment: ModEventComment,
    pub mod_event_report: ModEventReport,
    pub mod_event_label: ModEventLabel,
    pub mod_event_acknowledge: ModEventAcknowledge,
    pub mod_event_escalate: ModEventEscalate,
    pub mod_event_mute: ModEventMute,
    pub mod_event_unmute: ModEventUnmute,
    pub mod_event_email: ModEventEmail,
    pub mod_event_divert: ModEventDivert,
    pub mod_event_tag: ModEventTag,
    pub repo_view: RepoView,
    pub repo_view_detail: RepoViewDetail,
    pub repo_view_not_found: RepoViewNotFound,
    pub record_view: RecordView,
    pub record_view_detail: RecordViewDetail,
    pub record_view_not_found: RecordViewNotFound,
    pub moderation: Moderation5,
    pub moderation_detail: ModerationDetail,
    pub blob_view: BlobView,
    pub image_details: ImageDetails,
    pub video_details: VideoDetails,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModEventView {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties304,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties304 {
    pub id: Id5,
    pub event: Event2,
    pub subject: Subject20,
    pub subject_blob_cids: SubjectBlobCids,
    pub created_by: CreatedBy3,
    pub created_at: CreatedAt18,
    pub creator_handle: CreatorHandle,
    pub subject_handle: SubjectHandle,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Id5 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject20 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectBlobCids {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items117,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items117 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedBy3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAt18 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatorHandle {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectHandle {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModEventViewDetail {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties305,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties305 {
    pub id: Id6,
    pub event: Event3,
    pub subject: Subject21,
    pub subject_blobs: SubjectBlobs,
    pub created_by: CreatedBy4,
    pub created_at: CreatedAt19,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Id6 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject21 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectBlobs {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items118,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items118 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedBy4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAt19 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectStatusView {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties306,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties306 {
    pub id: Id7,
    pub subject: Subject22,
    pub subject_blob_cids: SubjectBlobCids2,
    pub subject_repo_handle: SubjectRepoHandle,
    pub updated_at: UpdatedAt2,
    pub created_at: CreatedAt20,
    pub review_state: ReviewState,
    pub comment: Comment2,
    pub mute_until: MuteUntil,
    pub last_reviewed_by: LastReviewedBy,
    pub last_reviewed_at: LastReviewedAt,
    pub last_reported_at: LastReportedAt,
    pub last_appealed_at: LastAppealedAt,
    pub takendown: Takendown,
    pub appealed: Appealed,
    pub suspend_until: SuspendUntil,
    pub tags: Tags3,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Id7 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject22 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectBlobCids2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items119,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items119 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectRepoHandle {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatedAt2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAt20 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewState {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MuteUntil {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastReviewedBy {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastReviewedAt {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastReportedAt {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastAppealedAt {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Takendown {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Appealed {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuspendUntil {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tags3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items120,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items120 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectReviewState {
    #[serde(rename = "type")]
    pub type_field: String,
    pub known_values: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewOpen {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewEscalated {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewClosed {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewNone {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModEventTakedown {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub properties: Properties307,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties307 {
    pub comment: Comment3,
    pub duration_in_hours: DurationInHours,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment3 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DurationInHours {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModEventReverseTakedown {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub properties: Properties308,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties308 {
    pub comment: Comment4,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModEventResolveAppeal {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub properties: Properties309,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties309 {
    pub comment: Comment5,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModEventComment {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties310,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties310 {
    pub comment: Comment6,
    pub sticky: Sticky,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment6 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sticky {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModEventReport {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties311,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties311 {
    pub comment: Comment7,
    pub report_type: ReportType,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment7 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportType {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModEventLabel {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties312,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties312 {
    pub comment: Comment8,
    pub create_label_vals: CreateLabelVals,
    pub negate_label_vals: NegateLabelVals,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment8 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateLabelVals {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items121,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items121 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NegateLabelVals {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items122,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items122 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModEventAcknowledge {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties313,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties313 {
    pub comment: Comment9,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment9 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModEventEscalate {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties314,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties314 {
    pub comment: Comment10,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment10 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModEventMute {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties315,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties315 {
    pub comment: Comment11,
    pub duration_in_hours: DurationInHours2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment11 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DurationInHours2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModEventUnmute {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub properties: Properties316,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties316 {
    pub comment: Comment12,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment12 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModEventEmail {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties317,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties317 {
    pub subject_line: SubjectLine,
    pub content: Content2,
    pub comment: Comment13,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectLine {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Content2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment13 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModEventDivert {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub properties: Properties318,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties318 {
    pub comment: Comment14,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment14 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModEventTag {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub required: Vec<String>,
    pub properties: Properties319,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties319 {
    pub add: Add,
    pub remove: Remove,
    pub comment: Comment15,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Add {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items123,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items123 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Remove {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items124,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items124 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment15 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoView {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties320,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties320 {
    pub did: Did43,
    pub handle: Handle16,
    pub email: Email10,
    pub related_records: RelatedRecords2,
    pub indexed_at: IndexedAt14,
    pub moderation: Moderation,
    pub invited_by: InvitedBy2,
    pub invites_disabled: InvitesDisabled2,
    pub invite_note: InviteNote2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did43 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handle16 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Email10 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedRecords2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items125,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items125 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedAt14 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Moderation {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvitedBy2 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvitesDisabled2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteNote2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoViewDetail {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties321,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties321 {
    pub did: Did44,
    pub handle: Handle17,
    pub email: Email11,
    pub related_records: RelatedRecords3,
    pub indexed_at: IndexedAt15,
    pub moderation: Moderation2,
    pub labels: Labels21,
    pub invited_by: InvitedBy3,
    pub invites: Invites2,
    pub invites_disabled: InvitesDisabled3,
    pub invite_note: InviteNote3,
    pub email_confirmed_at: EmailConfirmedAt2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did44 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handle17 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Email11 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedRecords3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items126,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items126 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedAt15 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Moderation2 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels21 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items127,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items127 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvitedBy3 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Invites2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items128,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items128 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvitesDisabled3 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteNote3 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailConfirmedAt2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoViewNotFound {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties322,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties322 {
    pub did: Did45,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did45 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordView {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties323,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties323 {
    pub uri: Uri32,
    pub cid: Cid26,
    pub value: Value8,
    pub blob_cids: BlobCids,
    pub indexed_at: IndexedAt16,
    pub moderation: Moderation3,
    pub repo: Repo10,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri32 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid26 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Value8 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobCids {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items129,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items129 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedAt16 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Moderation3 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repo10 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordViewDetail {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties324,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties324 {
    pub uri: Uri33,
    pub cid: Cid27,
    pub value: Value9,
    pub blobs: Blobs3,
    pub labels: Labels22,
    pub indexed_at: IndexedAt17,
    pub moderation: Moderation4,
    pub repo: Repo11,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri33 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid27 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Value9 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blobs3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items130,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items130 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels22 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items131,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items131 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedAt17 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Moderation4 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repo11 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordViewNotFound {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties325,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties325 {
    pub uri: Uri34,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri34 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Moderation5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties326,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties326 {
    pub subject_status: SubjectStatus,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectStatus {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModerationDetail {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties327,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties327 {
    pub subject_status: SubjectStatus2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectStatus2 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobView {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties328,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties328 {
    pub cid: Cid28,
    pub mime_type: MimeType,
    pub size: Size,
    pub created_at: CreatedAt21,
    pub details: Details,
    pub moderation: Moderation6,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid28 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MimeType {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Size {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAt21 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Details {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Moderation6 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageDetails {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties329,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties329 {
    pub width: Width2,
    pub height: Height2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Width2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Height2 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoDetails {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties330,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties330 {
    pub width: Width3,
    pub height: Height3,
    pub length: Length,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Width3 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Height3 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Length {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsOzoneModerationEmitEvent {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs153,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs153 {
    pub main: Main142,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main142 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub input: Input47,
    pub output: Output87,
    pub errors: Vec<Error29>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input47 {
    pub encoding: String,
    pub schema: Schema128,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema128 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties331,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties331 {
    pub event: Event4,
    pub subject: Subject23,
    pub subject_blob_cids: SubjectBlobCids3,
    pub created_by: CreatedBy5,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject23 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub refs: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectBlobCids3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items132,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items132 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedBy5 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output87 {
    pub encoding: String,
    pub schema: Schema129,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema129 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error29 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsOzoneModerationGetEvent {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs154,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs154 {
    pub main: Main143,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main143 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters63,
    pub output: Output88,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters63 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties332,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties332 {
    pub id: Id8,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Id8 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output88 {
    pub encoding: String,
    pub schema: Schema130,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema130 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsOzoneModerationGetRecord {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs155,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs155 {
    pub main: Main144,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main144 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters64,
    pub output: Output89,
    pub errors: Vec<Error30>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters64 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties333,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties333 {
    pub uri: Uri35,
    pub cid: Cid29,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uri35 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cid29 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output89 {
    pub encoding: String,
    pub schema: Schema131,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema131 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error30 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsOzoneModerationGetRepo {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs156,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs156 {
    pub main: Main145,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main145 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters65,
    pub output: Output90,
    pub errors: Vec<Error31>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters65 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties334,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties334 {
    pub did: Did46,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Did46 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output90 {
    pub encoding: String,
    pub schema: Schema132,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema132 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error31 {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsOzoneModerationQueryEvents {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs157,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs157 {
    pub main: Main146,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main146 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters66,
    pub output: Output91,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters66 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties335,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties335 {
    pub types: Types,
    pub created_by: CreatedBy6,
    pub sort_direction: SortDirection,
    pub created_after: CreatedAfter,
    pub created_before: CreatedBefore,
    pub subject: Subject24,
    pub include_all_user_records: IncludeAllUserRecords,
    pub limit: Limit34,
    pub has_comment: HasComment,
    pub comment: Comment16,
    pub added_labels: AddedLabels,
    pub removed_labels: RemovedLabels,
    pub added_tags: AddedTags,
    pub removed_tags: RemovedTags,
    pub report_types: ReportTypes,
    pub cursor: Cursor65,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Types {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items133,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items133 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedBy6 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortDirection {
    #[serde(rename = "type")]
    pub type_field: String,
    pub default: String,
    #[serde(rename = "enum")]
    pub enum_field: Vec<String>,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAfter {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedBefore {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject24 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncludeAllUserRecords {
    #[serde(rename = "type")]
    pub type_field: String,
    pub default: bool,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit34 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HasComment {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment16 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedLabels {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items134,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items134 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedLabels {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items135,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items135 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedTags {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items136,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items136 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedTags {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items137,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items137 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportTypes {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items138,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items138 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor65 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output91 {
    pub encoding: String,
    pub schema: Schema133,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema133 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties336,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties336 {
    pub cursor: Cursor66,
    pub events: Events,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor66 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Events {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items139,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items139 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsOzoneModerationQueryStatuses {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs158,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs158 {
    pub main: Main147,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main147 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters67,
    pub output: Output92,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters67 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties337,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties337 {
    pub subject: Subject25,
    pub comment: Comment17,
    pub reported_after: ReportedAfter,
    pub reported_before: ReportedBefore,
    pub reviewed_after: ReviewedAfter,
    pub reviewed_before: ReviewedBefore,
    pub include_muted: IncludeMuted,
    pub review_state: ReviewState2,
    pub ignore_subjects: IgnoreSubjects,
    pub last_reviewed_by: LastReviewedBy2,
    pub sort_field: SortField,
    pub sort_direction: SortDirection2,
    pub takendown: Takendown2,
    pub appealed: Appealed2,
    pub limit: Limit35,
    pub tags: Tags4,
    pub exclude_tags: ExcludeTags,
    pub cursor: Cursor67,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject25 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment17 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportedAfter {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportedBefore {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedAfter {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedBefore {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncludeMuted {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewState2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IgnoreSubjects {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items140,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items140 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastReviewedBy2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub format: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortField {
    #[serde(rename = "type")]
    pub type_field: String,
    pub default: String,
    #[serde(rename = "enum")]
    pub enum_field: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortDirection2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub default: String,
    #[serde(rename = "enum")]
    pub enum_field: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Takendown2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Appealed2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit35 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tags4 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items141,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items141 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcludeTags {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items142,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items142 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor67 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output92 {
    pub encoding: String,
    pub schema: Schema134,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema134 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties338,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties338 {
    pub cursor: Cursor68,
    pub subject_statuses: SubjectStatuses,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor68 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectStatuses {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items143,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items143 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsOzoneModerationSearchRepos {
    pub lexicon: i64,
    pub id: String,
    pub defs: Defs159,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defs159 {
    pub main: Main148,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main148 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
    pub parameters: Parameters68,
    pub output: Output93,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameters68 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub properties: Properties339,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties339 {
    pub term: Term3,
    pub q: Q6,
    pub limit: Limit36,
    pub cursor: Cursor69,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Term3 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub description: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Q6 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit36 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor69 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output93 {
    pub encoding: String,
    pub schema: Schema135,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema135 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub required: Vec<String>,
    pub properties: Properties340,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties340 {
    pub cursor: Cursor70,
    pub repos: Repos2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor70 {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repos2 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Items144,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Items144 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "ref")]
    pub ref_field: String,
}
