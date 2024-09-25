use crate::account_manager::helpers::auth::ServiceJwtParams;
use crate::account_manager::AccountManager;
use crate::common::beginning_of_time;
use crate::db::establish_connection;
use crate::models::models;
use crate::read_after_write::types::{LocalRecords, RecordDescript};
use crate::read_after_write::util;
use crate::repo::types::Ids;
use crate::repo::ActorStore;
use crate::xrpc_server::auth::create_service_auth_headers;
use crate::{APP_USER_AGENT, INVALID_HANDLE};
use anyhow::{bail, Result};
use atrium_api::app::bsky::feed::get_feed_generator::{
    Output as AppBskyFeedGetFeedGeneratorOutput, Parameters as AppBskyFeedGetFeedGeneratorParams,
    ParametersData as AppBskyFeedGetFeedGeneratorData,
};
use atrium_api::app::bsky::feed::get_posts::{
    Output as AppBskyFeedGetPostsOutput, Parameters as AppBskyFeedGetPostsParams,
    ParametersData as AppBskyFeedGetPostsData,
};
use atrium_api::app::bsky::graph::get_list::{
    Output as AppBskyGraphGetListOutput, Parameters as AppBskyGraphGetListParams,
    ParametersData as AppBskyGraphGetListData,
};
use atrium_api::client::AtpServiceClient;
use atrium_ipld::ipld::Ipld as AtriumIpld;
use atrium_xrpc_client::reqwest::{ReqwestClient, ReqwestClientBuilder};
use diesel::*;
use futures::stream::{self, StreamExt};
use libipld::Cid;
use reqwest::header::HeaderMap;
use rsky_lexicon::app::bsky::actor::{Profile, ProfileView, ProfileViewBasic, ProfileViewDetailed};
use rsky_lexicon::app::bsky::embed::external::{
    ExternalObject, View as ExternalView, ViewExternal,
};
use rsky_lexicon::app::bsky::embed::images::{View as ImagesView, ViewImage};
use rsky_lexicon::app::bsky::embed::record::{
    Record, View as RecordView, ViewNotFound as RecordViewNotFound, ViewRecord,
};
use rsky_lexicon::app::bsky::embed::record_with_media::{
    RecordWithMedia, View as RecordWithMediaView,
};
use rsky_lexicon::app::bsky::embed::{record, EmbedViews, Embeds, MediaUnion, MediaViewUnion};
use rsky_lexicon::app::bsky::feed::{FeedViewPost, GeneratorView, Post, PostView};
use rsky_lexicon::app::bsky::graph::ListView;
use rsky_syntax::aturi::AtUri;
use secp256k1::SecretKey;
use std::env;
use std::str::FromStr;

pub type Agent = AtpServiceClient<ReqwestClient>;

pub type LocalViewerCreator = Box<dyn Fn(ActorStore) -> LocalViewer + Send + Sync>;

pub struct LocalViewerCreatorParams {
    pub account_manager: AccountManager,
    pub pds_hostname: String,
    pub appview_agent: Option<String>,
    pub appview_did: Option<String>,
    pub appview_cdn_url_pattern: Option<String>,
}

pub struct LocalViewer {
    pub did: String,
    pub actor_store: ActorStore,
    pub pds_hostname: String,
    pub appview_agent: Option<Agent>,
    appview_agent_str: Option<String>,
    pub appview_did: Option<String>,
    pub appview_cdn_url_pattern: Option<String>,
}

impl LocalViewer {
    pub fn new(
        actor_store: ActorStore,
        pds_hostname: String,
        appview_agent: Option<Agent>,
        appview_agent_str: Option<String>,
        appview_did: Option<String>,
        appview_cdn_url_pattern: Option<String>,
    ) -> Self {
        LocalViewer {
            did: actor_store.did.clone(),
            actor_store,
            pds_hostname,
            appview_agent,
            appview_agent_str,
            appview_did,
            appview_cdn_url_pattern,
        }
    }

    pub fn creator(params: LocalViewerCreatorParams) -> LocalViewerCreator {
        return Box::new(move |actor_store: ActorStore| -> LocalViewer {
            LocalViewer::new(
                actor_store,
                params.pds_hostname.clone(),
                match params.appview_agent {
                    None => None,
                    Some(ref bsky_app_view_url) => {
                        let client = ReqwestClientBuilder::new(bsky_app_view_url.clone())
                            .client(
                                reqwest::ClientBuilder::new()
                                    .user_agent(APP_USER_AGENT)
                                    .timeout(std::time::Duration::from_millis(1000))
                                    .build()
                                    .unwrap(),
                            )
                            .build();
                        Some(AtpServiceClient::new(client))
                    }
                },
                match params.appview_agent {
                    None => None,
                    Some(ref bsky_app_view_url) => Some(bsky_app_view_url.clone()),
                },
                params.appview_did.clone(),
                params.appview_cdn_url_pattern.clone(),
            )
        });
    }

    pub fn get_image_url(&self, pattern: String, cid: String) -> String {
        match &self.appview_cdn_url_pattern {
            None => format!(
                "https://{}/xrpc/{}?did={}&cid={}",
                self.pds_hostname,
                Ids::ComAtprotoSyncGetBlob.as_str(),
                self.did,
                cid
            ),
            Some(appview_cdn_url_pattern) => {
                util::nodejs_format(&*appview_cdn_url_pattern, &[&pattern, &self.did, &cid])
            }
        }
    }

    pub async fn service_auth_headers(&self, did: &String, lxm: &String) -> Result<HeaderMap> {
        match &self.appview_did {
            None => bail!("Could not find bsky appview did"),
            Some(appview_did) => {
                let private_key = env::var("PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX").unwrap();
                let keypair =
                    SecretKey::from_slice(&hex::decode(private_key.as_bytes()).unwrap()).unwrap();
                create_service_auth_headers(ServiceJwtParams {
                    iss: did.clone(),
                    aud: appview_did.clone(),
                    exp: None,
                    lxm: Some(lxm.clone()),
                    jti: None,
                    keypair,
                })
                .await
            }
        }
    }

    pub async fn get_records_since_rev(&self, rev: String) -> Result<LocalRecords> {
        get_records_since_rev(&self.actor_store, rev).await
    }

    pub async fn get_profile_basic(&self) -> Result<Option<ProfileViewBasic>> {
        use crate::schema::pds::record::dsl as RecordSchema;
        use crate::schema::pds::repo_block::dsl as RepoBlockSchema;
        let conn = &mut establish_connection()?;

        let profile_res: Option<(models::Record, Option<models::RepoBlock>)> = RecordSchema::record
            .left_join(RepoBlockSchema::repo_block.on(RepoBlockSchema::cid.eq(RecordSchema::cid)))
            .select((
                models::Record::as_select(),
                Option::<models::RepoBlock>::as_select(),
            ))
            .filter(RecordSchema::did.eq(&self.actor_store.did))
            .filter(RecordSchema::collection.eq(Ids::AppBskyActorProfile.as_str()))
            .filter(RecordSchema::rkey.eq("self"))
            .first(conn)
            .optional()?;
        let account_res = AccountManager::get_account(&self.did, None).await?;
        match account_res {
            None => Ok(None),
            Some(account_res) => {
                let record: Option<Profile> = match profile_res {
                    Some(profile_res) => match profile_res.1 {
                        Some(profile_res) => {
                            serde_ipld_dagcbor::from_slice(profile_res.content.as_slice())?
                        }
                        None => None,
                    },
                    None => None,
                };
                Ok(Some(ProfileViewBasic {
                    did: self.did.clone(),
                    handle: account_res
                        .handle
                        .unwrap_or_else(|| INVALID_HANDLE.to_string()),
                    display_name: match record.clone() {
                        Some(record) => record.display_name,
                        None => None,
                    },
                    avatar: match record {
                        Some(record) => match record.avatar {
                            Some(avatar) => match avatar.r#ref {
                                Some(r#ref) => Some(
                                    self.get_image_url("avatar".to_string(), r#ref.to_string()),
                                ),
                                None => None,
                            },
                            None => None,
                        },
                        None => None,
                    },
                    associated: None,
                    viewer: None,
                    labels: None,
                    created_at: None,
                }))
            }
        }
    }

    pub async fn format_and_insert_posts_in_feed(
        &self,
        mut feed: Vec<FeedViewPost>,
        posts: Vec<RecordDescript<Post>>,
    ) -> Result<Vec<FeedViewPost>> {
        if posts.len() == 0 {
            return Ok(feed);
        }
        let last_time: String = match feed.last() {
            None => beginning_of_time(),
            Some(feed_post) => feed_post.post.indexed_at.clone(),
        };
        let mut in_feed = posts
            .into_iter()
            .filter(|p| p.indexed_at > last_time)
            .collect::<Vec<RecordDescript<Post>>>();
        in_feed.reverse();

        let maybe_formatted: Vec<Option<PostView>> = stream::iter(in_feed)
            .then(|p| async move { self.get_post(p).await })
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<Option<PostView>>>>()?;
        let formatted: Vec<PostView> = maybe_formatted.into_iter().flatten().collect();
        for post in formatted {
            let idx = feed
                .iter()
                .position(|fi| fi.post.indexed_at < post.indexed_at);
            let feed_view_post = FeedViewPost {
                post,
                reply: None,
                reason: None,
                feed_context: None,
            };
            match idx {
                None => feed.push(feed_view_post),
                Some(idx) => feed.insert(idx, feed_view_post),
            }
        }
        Ok(feed)
    }

    pub async fn get_post(&self, descript: RecordDescript<Post>) -> Result<Option<PostView>> {
        let RecordDescript {
            uri,
            cid,
            indexed_at,
            record,
        } = descript;
        let author = self.get_profile_basic().await?;
        match author {
            None => Ok(None),
            Some(author) => {
                let embed = match record.embed {
                    None => None,
                    Some(_) => self.format_post_embed(record.clone()).await?,
                };
                Ok(Some(PostView {
                    uri: uri.to_string(),
                    cid: cid.to_string(),
                    author,
                    record: serde_json::to_value(record)?,
                    embed,
                    reply_count: Some(0), // counts presumed to be 0 directly after post creation
                    repost_count: Some(0),
                    like_count: Some(0),
                    indexed_at,
                    viewer: None,
                    labels: None,
                }))
            }
        }
    }

    pub async fn format_post_embed(&self, post: Post) -> Result<Option<EmbedViews>> {
        let embed = post.embed;
        match embed {
            None => Ok(None),
            Some(embed) => match embed {
                Embeds::Images(embed) => Ok(Some(
                    self.format_simple_embed(MediaUnion::Images(embed)).await,
                )),
                Embeds::External(embed) => Ok(Some(
                    self.format_simple_embed(MediaUnion::External(embed)).await,
                )),
                Embeds::Record(embed) => Ok(Some(EmbedViews::RecordView(
                    self.format_record_embed(embed).await?,
                ))),
                Embeds::RecordWithMedia(embed) => Ok(Some(EmbedViews::RecordWithMediaView(
                    self.format_record_with_media_embed(embed).await?,
                ))),
                _ => Ok(None), // @TODO: Handle video
            },
        }
    }

    pub async fn format_simple_embed(&self, embed: MediaUnion) -> EmbedViews {
        match embed {
            MediaUnion::Images(embed) => {
                let images = embed
                    .images
                    .into_iter()
                    .map(|img| ViewImage {
                        thumb: self.get_image_url(
                            "feed_thumbnail".to_string(),
                            img.image.r#ref.clone().unwrap().to_string(),
                        ),
                        fullsize: self.get_image_url(
                            "feed_fullsize".to_string(),
                            img.image.r#ref.unwrap().to_string(),
                        ),
                        alt: img.alt,
                        aspect_ratio: img.aspect_ratio,
                    })
                    .collect::<Vec<ViewImage>>();
                EmbedViews::ImagesView(ImagesView { images })
            }
            MediaUnion::External(embed) => {
                let ExternalObject {
                    uri,
                    title,
                    description,
                    thumb,
                } = embed.external;
                EmbedViews::ExternalView(ExternalView {
                    external: ViewExternal {
                        uri,
                        title,
                        description,
                        thumb: match thumb {
                            None => None,
                            Some(thumb) => Some(self.get_image_url(
                                "feed_thumbnail".to_string(),
                                thumb.r#ref.unwrap().to_string(),
                            )),
                        },
                    },
                })
            }
            _ => panic!("Can't handle video"), // @TODO: Handle video
        }
    }

    pub async fn format_record_embed(&self, embed: Record) -> Result<RecordView> {
        let view = self.format_record_embed_internal(embed.clone()).await?;
        match view {
            None => Ok(RecordView {
                record: record::ViewUnion::ViewNotFound(RecordViewNotFound {
                    uri: embed.record.uri,
                    not_found: true,
                }),
            }),
            Some(view) => Ok(RecordView { record: view }),
        }
    }

    pub async fn format_record_embed_internal(
        &self,
        embed: Record,
    ) -> Result<Option<record::ViewUnion>> {
        match (&self.appview_agent, &self.appview_did) {
            (Some(_), Some(_)) => {
                let collection = AtUri::new(embed.record.uri.clone(), None)?.get_collection();
                if collection == Ids::AppBskyFeedPost.as_str() {
                    let appview_agent = self.get_authenticated_agent_for_nsid(&collection).await?;
                    let res: AppBskyFeedGetPostsOutput = appview_agent
                        .service
                        .app
                        .bsky
                        .feed
                        .get_posts(AppBskyFeedGetPostsParams {
                            data: AppBskyFeedGetPostsData {
                                uris: vec![embed.record.uri],
                            },
                            extra_data: AtriumIpld::Null,
                        })
                        .await?;
                    match res.posts.first() {
                        None => Ok(None),
                        Some(post) => {
                            let post: PostView =
                                serde_json::from_value(serde_json::to_value(&post)?)?;
                            Ok(Some(record::ViewUnion::ViewRecord(ViewRecord {
                                uri: post.uri,
                                cid: post.cid,
                                author: post.author,
                                value: post.record,
                                labels: post.labels,
                                reply_count: None,
                                repost_count: None,
                                like_count: None,
                                embeds: match post.embed {
                                    Some(post_embed) => Some(vec![post_embed]),
                                    None => None,
                                },
                                indexed_at: post.indexed_at,
                            })))
                        }
                    }
                } else if collection == Ids::AppBskyFeedGenerator.as_str() {
                    let appview_agent = self.get_authenticated_agent_for_nsid(&collection).await?;
                    let res: AppBskyFeedGetFeedGeneratorOutput = appview_agent
                        .service
                        .app
                        .bsky
                        .feed
                        .get_feed_generator(AppBskyFeedGetFeedGeneratorParams {
                            data: AppBskyFeedGetFeedGeneratorData {
                                feed: embed.record.uri,
                            },
                            extra_data: AtriumIpld::Null,
                        })
                        .await?;
                    let generator_view: GeneratorView =
                        serde_json::from_value(serde_json::to_value(&res.view)?)?;
                    Ok(Some(record::ViewUnion::GeneratorView(generator_view)))
                } else if collection == Ids::AppBskyGraphList.as_str() {
                    let appview_agent = self.get_authenticated_agent_for_nsid(&collection).await?;
                    let res: AppBskyGraphGetListOutput = appview_agent
                        .service
                        .app
                        .bsky
                        .graph
                        .get_list(AppBskyGraphGetListParams {
                            data: AppBskyGraphGetListData {
                                cursor: None,
                                limit: None,
                                list: embed.record.uri,
                            },
                            extra_data: AtriumIpld::Null,
                        })
                        .await?;
                    let list_view: ListView =
                        serde_json::from_value(serde_json::to_value(&res.list)?)?;
                    Ok(Some(record::ViewUnion::ListView(list_view)))
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }

    pub async fn format_record_with_media_embed(
        &self,
        embed: RecordWithMedia,
    ) -> Result<RecordWithMediaView> {
        let media = match self.format_simple_embed(embed.media).await {
            EmbedViews::ImagesView(media) => MediaViewUnion::ImagesView(media),
            EmbedViews::ExternalView(media) => MediaViewUnion::ExternalView(media),
            _ => bail!("Unexpected enum for media."),
        };
        let record = self.format_record_embed(embed.record).await?;
        Ok(RecordWithMediaView { record, media })
    }

    pub fn update_profile_view_basic(
        &self,
        view: ProfileViewBasic,
        record: Profile,
    ) -> ProfileViewBasic {
        let ProfileViewBasic {
            did,
            handle,
            associated,
            viewer,
            labels,
            created_at,
            ..
        } = view;
        ProfileViewBasic {
            did,
            handle,
            display_name: record.display_name,
            avatar: match record.avatar {
                None => None,
                Some(avatar) => match avatar.r#ref {
                    Some(r#ref) => {
                        Some(self.get_image_url("avatar".to_string(), r#ref.to_string()))
                    }
                    None => None,
                },
            },
            associated,
            viewer,
            labels,
            created_at,
        }
    }

    pub fn update_profile_view(&self, view: ProfileView, record: Profile) -> ProfileView {
        let ProfileView {
            did,
            handle,
            display_name,
            description,
            avatar,
            labels,
            indexed_at,
        } = view;
        let ProfileViewBasic {
            did,
            handle,
            display_name,
            avatar,
            ..
        } = self.update_profile_view_basic(
            ProfileViewBasic {
                did,
                handle,
                display_name,
                avatar,
                associated: None,
                viewer: None,
                labels: Some(labels),
                created_at: None,
            },
            record,
        );
        ProfileView {
            did,
            handle,
            display_name,
            description,
            avatar,
            labels: vec![],
            indexed_at,
        }
    }

    pub fn update_profile_detailed(
        &self,
        view: ProfileViewDetailed,
        record: Profile,
    ) -> ProfileViewDetailed {
        let ProfileViewDetailed {
            did,
            handle,
            display_name,
            description,
            avatar,
            followers_count,
            follows_count,
            posts_count,
            associated,
            joined_via_starter_pack,
            viewer,
            labels,
            indexed_at,
            created_at,
            ..
        } = view;
        let ProfileView {
            did,
            handle,
            display_name,
            description,
            avatar,
            labels,
            indexed_at,
        } = self.update_profile_view(
            ProfileView {
                did,
                handle,
                display_name,
                description,
                avatar,
                labels,
                indexed_at,
            },
            record.clone(),
        );
        ProfileViewDetailed {
            did,
            handle,
            display_name,
            description,
            avatar,
            banner: match record.banner {
                None => None,
                Some(record_banner) => match record_banner.r#ref {
                    Some(r#ref) => {
                        Some(self.get_image_url("banner".to_string(), r#ref.to_string()))
                    }
                    None => None,
                },
            },
            followers_count,
            follows_count,
            posts_count,
            associated,
            joined_via_starter_pack,
            viewer,
            labels,
            indexed_at,
            created_at,
        }
    }

    async fn get_authenticated_agent_for_nsid(
        &self,
        nsid: &String,
    ) -> Result<AtpServiceClient<ReqwestClient>> {
        let auth_headers = self.service_auth_headers(&self.did, nsid).await?;
        let base = match self.appview_agent_str {
            None => bail!("no appview url configured"),
            Some(ref appview_agent_str) => appview_agent_str,
        };
        let client = ReqwestClientBuilder::new(base)
            .client(
                reqwest::ClientBuilder::new()
                    .user_agent(APP_USER_AGENT)
                    .timeout(std::time::Duration::from_millis(1000))
                    .default_headers(auth_headers)
                    .build()
                    .unwrap(),
            )
            .build();
        Ok(AtpServiceClient::new(client))
    }
}

pub async fn get_records_since_rev(actor_store: &ActorStore, rev: String) -> Result<LocalRecords> {
    use crate::schema::pds::record::dsl as RecordSchema;
    use crate::schema::pds::repo_block::dsl as RepoBlockSchema;
    let conn = &mut establish_connection()?;

    let res: Vec<(models::Record, models::RepoBlock)> = RecordSchema::record
        .inner_join(RepoBlockSchema::repo_block.on(RepoBlockSchema::cid.eq(RecordSchema::cid)))
        .select((models::Record::as_select(), models::RepoBlock::as_select()))
        .filter(RecordSchema::did.eq(&actor_store.did))
        .filter(RecordSchema::repoRev.gt(&rev))
        .limit(10)
        .order_by(RecordSchema::repoRev.asc())
        .get_results(conn)?;

    // sanity check to ensure that the clock received is not before _all_ local records
    // (for instance in case of account migration)
    if res.len() > 0 {
        let sanity_checks = RecordSchema::record
            .select(models::Record::as_select())
            .filter(RecordSchema::did.eq(&actor_store.did))
            .filter(RecordSchema::repoRev.le(&rev))
            .limit(1)
            .first(conn)
            .optional()?;
        if sanity_checks.is_none() {
            return Ok(LocalRecords {
                count: 0,
                profile: None,
                posts: vec![],
            });
        }
    }

    // res.reduce() in javascript
    res.into_iter().try_fold(
        LocalRecords {
            count: 0,
            profile: None,
            posts: vec![],
        },
        |mut acc: LocalRecords, cur| {
            let uri: AtUri = AtUri::new(cur.0.uri, None)?;
            if uri.get_collection() == Ids::AppBskyActorProfile.as_str()
                && uri.get_rkey() == "self".to_string()
            {
                let profile: Profile = serde_ipld_dagcbor::from_slice(cur.1.content.as_slice())?;
                let descript = RecordDescript {
                    uri,
                    cid: Cid::from_str(&cur.1.cid)?,
                    indexed_at: cur.0.indexed_at,
                    record: profile,
                };
                acc.profile = Some(descript);
            } else if uri.get_collection() == Ids::AppBskyFeedPost.as_str() {
                let post: Post = serde_ipld_dagcbor::from_slice(cur.1.content.as_slice())?;
                let descript = RecordDescript {
                    uri,
                    cid: Cid::from_str(&cur.1.cid)?,
                    indexed_at: cur.0.indexed_at,
                    record: post,
                };
                acc.posts.push(descript);
            }

            acc.count += 1;
            Ok(acc)
        },
    )
}
