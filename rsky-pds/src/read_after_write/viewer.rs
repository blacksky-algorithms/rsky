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
use crate::INVALID_HANDLE;
use anyhow::{bail, Result};
use atrium_api::app::bsky::feed::get_posts::{
    Output as AppBskyFeedGetPostsOutput, Parameters as AppBskyFeedGetPostsParams,
    ParametersData as AppBskyFeedGetPostsData,
};
use atrium_api::client::AtpServiceClient;
use atrium_ipld::ipld::Ipld as AtriumIpld;
use atrium_xrpc_client::reqwest::{ReqwestClient, ReqwestClientBuilder};
use diesel::*;
use libipld::Cid;
use reqwest::header;
use rsky_lexicon::app::bsky::actor::Profile;
use rsky_lexicon::app::bsky::embed::external::{
    ExternalObject, View as EmbedExternalView, ViewExternal,
};
use rsky_lexicon::app::bsky::embed::images::{View as ViewImages, ViewImage};
use rsky_lexicon::app::bsky::embed::record::{Record, View as EmbedRecordView, ViewRecord};
use rsky_lexicon::app::bsky::embed::{MediaUnion, MediaViewUnion};
use rsky_lexicon::app::bsky::feed::{
    FeedViewPost, GeneratorView, Post, PostView, ProfileViewBasic,
};
use rsky_lexicon::app::bsky::graph::ListView;
use rsky_syntax::aturi::AtUri;
use secp256k1::SecretKey;
use std::env;
use std::str::FromStr;

pub type Agent = AtpServiceClient<ReqwestClient>;

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
    pub appview_agent: Option<String>,
    pub appview_did: Option<String>,
    pub appview_cdn_url_pattern: Option<String>,
}

pub enum FormatRecordEmbedInternalOutput {
    ViewRecord(ViewRecord),
    GeneratorView(GeneratorView),
    ListView(ListView),
}

impl LocalViewer {
    pub fn new(
        actor_store: ActorStore,
        pds_hostname: String,
        appview_agent: Option<String>,
        appview_did: Option<String>,
        appview_cdn_url_pattern: Option<String>,
    ) -> Self {
        LocalViewer {
            did: actor_store.did.clone(),
            actor_store,
            pds_hostname,
            appview_agent,
            appview_did,
            appview_cdn_url_pattern,
        }
    }

    pub fn creator(params: LocalViewerCreatorParams) -> impl Fn(ActorStore) -> LocalViewer {
        return move |actor_store: ActorStore| -> LocalViewer {
            LocalViewer::new(
                actor_store,
                params.pds_hostname.clone(),
                params.appview_agent.clone(),
                params.appview_did.clone(),
                params.appview_cdn_url_pattern.clone(),
            )
        };
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

    pub async fn service_auth_headers(&self, did: &String) -> Result<(String, String)> {
        match &self.appview_did {
            None => bail!("Could not find bsky appview did"),
            Some(aud) => {
                let private_key = env::var("PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX").unwrap();
                let keypair =
                    SecretKey::from_slice(&hex::decode(private_key.as_bytes()).unwrap()).unwrap();
                create_service_auth_headers(ServiceJwtParams {
                    iss: did.clone(),
                    aud: aud.clone(),
                    exp: None,
                    keypair,
                })
                .await
            }
        }
    }

    pub async fn get_atp_agent(&self) -> Result<Option<Agent>> {
        match (&self.appview_agent, &self.appview_did) {
            (Some(appview_agent), Some(appview_did)) => {
                let mut headers = header::HeaderMap::new();
                let service_auth = &self.service_auth_headers(appview_did).await?;
                headers.insert(
                    &service_auth.0,
                    header::HeaderValue::from_str(&service_auth.1)?,
                );

                let client = ReqwestClientBuilder::new(appview_agent)
                    .client(
                        reqwest::ClientBuilder::new()
                            .timeout(std::time::Duration::from_millis(1000))
                            .default_headers(headers)
                            .build()?,
                    )
                    .build();
                Ok(Some(AtpServiceClient::new(client)))
            }
            (Some(appview_agent), None) => Ok(Some(AtpServiceClient::new(ReqwestClient::new(
                appview_agent,
            )))),
            _ => Ok(None),
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
                                Some(r#ref) => {
                                    Some(self.get_image_url("avatar".to_string(), r#ref.link))
                                }
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
        feed: Vec<FeedViewPost>,
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

        let maybe_formatted = in_feed.into_iter().map(|p| self.get_post(p)).collect();

        todo!()
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
                todo!()
            }
        }
    }

    pub async fn format_post_embed(&self, did: String, post: Post) -> Result<Option<String>> {
        todo!()
    }

    pub async fn format_simple_embed(&self, embed: MediaUnion) -> MediaViewUnion {
        match embed {
            MediaUnion::Images(embed) => {
                let images = embed
                    .images
                    .into_iter()
                    .map(|img| ViewImage {
                        thumb: self.get_image_url(
                            "feed_thumbnail".to_string(),
                            img.image.r#ref.clone().unwrap().link,
                        ),
                        fullsize: self.get_image_url(
                            "feed_fullsize".to_string(),
                            img.image.r#ref.unwrap().link,
                        ),
                        alt: img.alt,
                        aspect_ratio: img.aspect_ratio,
                    })
                    .collect::<Vec<ViewImage>>();
                MediaViewUnion::ImagesView(ViewImages { images })
            }
            MediaUnion::External(embed) => {
                let ExternalObject {
                    uri,
                    title,
                    description,
                    thumb,
                } = embed.external;
                MediaViewUnion::ExternalView(EmbedExternalView {
                    external: ViewExternal {
                        uri,
                        title,
                        description,
                        thumb: match thumb {
                            None => None,
                            Some(thumb) => Some(self.get_image_url(
                                "feed_thumbnail".to_string(),
                                thumb.r#ref.unwrap().link,
                            )),
                        },
                    },
                })
            }
        }
    }

    pub async fn format_record_embed_internal(
        &self,
        embed: Record,
    ) -> Result<Option<FormatRecordEmbedInternalOutput>> {
        match (&self.get_atp_agent().await, &self.appview_did) {
            (Ok(Some(appview_agent)), Some(appview_did)) => {
                let collection = AtUri::new(embed.record.uri, None)?.get_collection();
                if collection == Ids::AppBskyFeedPost.as_str() {
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
                            Ok(Some(FormatRecordEmbedInternalOutput::ViewRecord(
                                ViewRecord {
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
                                },
                            )))
                        }
                    }
                } else if collection == Ids::AppBskyFeedGenerator.as_str() {
                    todo!()
                } else if collection == Ids::AppBskyGraphList.as_str() {
                    todo!()
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
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
