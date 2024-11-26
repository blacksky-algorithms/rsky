use crate::db::*;
use crate::explicit_slurs::contains_explicit_slurs;
use crate::models::*;
use crate::{FeedGenConfig, ReadReplicaConn, WriteDbConn};
use chrono::offset::Utc as UtcOffset;
use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::prelude::*;
use diesel::sql_query;
use once_cell::sync::Lazy;
use rand::Rng;
use regex::Regex;
use rocket::State;
use rsky_lexicon::app::bsky::embed::{Embeds, MediaUnion};
use std::collections::HashSet;
use std::fmt::Write;
use std::time::SystemTime;

#[allow(deprecated)]
pub async fn get_posts_by_membership(
    lang: Option<String>,
    limit: Option<i64>,
    params_cursor: Option<&str>,
    only_posts: bool,
    list: String,
    hashtags: Vec<String>,
    connection: ReadReplicaConn,
    config: &State<FeedGenConfig>,
) -> Result<AlgoResponse, ValidationErrorMessageResponse> {
    use crate::schema::membership::dsl as MembershipSchema;
    use crate::schema::post::dsl as PostSchema;
    use diesel::dsl::any;

    let show_sponsored_post = config.show_sponsored_post.clone();
    let sponsored_post_uri = config.sponsored_post_uri.clone();
    let sponsored_post_probability = config.sponsored_post_probability.clone();

    let params_cursor = match params_cursor {
        None => None,
        Some(params_cursor) => Some(params_cursor.to_string()),
    };
    let result = connection
        .run(move |conn| {
            let mut query = PostSchema::post
                .left_join(
                    MembershipSchema::membership.on(PostSchema::author
                        .eq(MembershipSchema::did)
                        .and(MembershipSchema::list.eq(list.clone()))
                        .and(MembershipSchema::included.eq(true))),
                )
                .limit(limit.unwrap_or(30))
                .select(Post::as_select())
                .order((PostSchema::createdAt.desc(), PostSchema::cid.desc()))
                .into_boxed();

            if let Some(lang) = lang {
                query = query.filter(PostSchema::lang.like(format!("%{}%", lang)));
            }

            if let Some(cursor_str) = params_cursor {
                let v = cursor_str
                    .split("::")
                    .take(2)
                    .map(String::from)
                    .collect::<Vec<_>>();
                if let [created_at_c, cid_c] = &v[..] {
                    if let Ok(timestamp) = created_at_c.parse::<i64>() {
                        let nanoseconds = 230 * 1000000;
                        let datetime = DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp(timestamp / 1000, nanoseconds),
                            Utc,
                        );
                        let mut timestr = String::new();
                        match write!(timestr, "{}", datetime.format("%+")) {
                            Ok(_) => {
                                query = query.filter(
                                    PostSchema::createdAt.lt(timestr.to_owned()).or(
                                        PostSchema::createdAt
                                            .eq(timestr.to_owned())
                                            .and(PostSchema::cid.lt(cid_c.to_owned())),
                                    ),
                                );
                            }
                            Err(error) => eprintln!("Error formatting: {error:?}"),
                        }
                    }
                } else {
                    let validation_error = ValidationErrorMessageResponse {
                        code: Some(ErrorCode::ValidationError),
                        message: Some("malformed cursor".into()),
                    };
                    return Err(validation_error);
                }
            }
            if only_posts {
                query = query
                    .filter(PostSchema::replyParent.is_null())
                    .filter(PostSchema::replyRoot.is_null());
            }

            // Adjust the filtering logic
            if hashtags.is_empty() {
                // No hashtags provided, include only posts where author is in the list
                query = query.filter(MembershipSchema::did.is_not_null());
            } else {
                let hashtag_patterns: Vec<String> = hashtags
                    .iter()
                    .map(|hashtag| format!("%#{}%", hashtag))
                    .collect();
                query = query.filter(
                    MembershipSchema::did
                        .is_not_null()
                        .or(PostSchema::text.ilike(any(hashtag_patterns))),
                );
            }

            let results = query.load(conn).expect("Error loading post records");

            let mut post_results = Vec::new();
            let mut cursor: Option<String> = None;

            // https://docs.rs/chrono/0.4.26/chrono/format/strftime/index.html
            if let Some(last_post) = results.last() {
                if let Ok(parsed_time) = NaiveDateTime::parse_from_str(&last_post.created_at, "%+")
                {
                    cursor = Some(format!(
                        "{}::{}",
                        parsed_time.timestamp_millis(),
                        last_post.cid
                    ));
                }
            }

            results
                .into_iter()
                .map(|result| {
                    let post_result = PostResult { post: result.uri };
                    post_results.push(post_result);
                })
                .for_each(drop);

            // Insert the sponsored post if the conditions are met
            if show_sponsored_post && post_results.len() >= 3 && !sponsored_post_uri.is_empty() {
                // Generate a random chance to include the sponsored post based on probability
                let mut rng = rand::thread_rng();
                let random_chance: f64 = rng.gen();

                // Only include the sponsored post if random chance is below the specified probability
                if random_chance < sponsored_post_probability {
                    // Generate a random index to insert the sponsored post (ensure it's not the last position)
                    let replace_index = rng.gen_range(0..(post_results.len() - 1));

                    // Replace a random post with the sponsored post
                    post_results[replace_index] = PostResult {
                        post: sponsored_post_uri.clone(),
                    };
                }
            }

            let new_response = AlgoResponse {
                cursor,
                feed: post_results,
            };
            Ok(new_response)
        })
        .await;

    result
}

#[allow(deprecated)]
pub async fn get_blacksky_trending(
    limit: Option<i64>,
    params_cursor: Option<&str>,
    connection: ReadReplicaConn,
    config: &State<FeedGenConfig>,
) -> Result<AlgoResponse, ValidationErrorMessageResponse> {
    use std::env;
    let trending_percentile_min = config.trending_percentile_min.clone();

    let params_cursor = match params_cursor {
        None => None,
        Some(params_cursor) => Some(params_cursor.to_string()),
    };
    let result = connection
        .run(move |conn| {
            let random_percentile = rand::thread_rng().gen_range(trending_percentile_min..=1.0);
            let mut query_str = format!(
                "WITH recent_posts AS (
                SELECT
                    *
                FROM post
                WHERE post.\"indexedAt\" >= (CURRENT_TIMESTAMP - INTERVAL '2 days')::text
            ), recent_likes AS (
                SELECT
                    \"subjectUri\",
                    COUNT(*) AS like_count
                FROM public.like
                WHERE public.like.\"indexedAt\" >= (CURRENT_TIMESTAMP - INTERVAL '24 hours')::text
                GROUP BY \"subjectUri\"
            ), posts_with_likes AS (
                SELECT
                    p.*,
                    COALESCE(l.like_count, 0) AS like_count
                FROM recent_posts p
                LEFT JOIN recent_likes l ON l.\"subjectUri\" = p.uri
            ), ranked_posts AS (
                SELECT
                    *,
                    PERCENT_RANK() OVER (ORDER BY like_count) AS percentile_rank
                FROM posts_with_likes
            )
            SELECT
                uri,
                cid,
                \"replyParent\",
                \"replyRoot\",
                \"indexedAt\",
                prev,
                \"sequence\",
                \"text\",
                lang,
                author,
                \"externalUri\",
                \"externalTitle\",
                \"externalDescription\",
                \"externalThumb\",
                \"quoteCid\",
                \"quoteUri\",
                \"createdAt\"
            FROM ranked_posts
            WHERE percentile_rank >= {:.4}",
                random_percentile
            );

            if params_cursor.is_some() {
                let cursor_str = params_cursor.unwrap();
                let v = cursor_str
                    .split("::")
                    .take(2)
                    .map(String::from)
                    .collect::<Vec<_>>();
                if let [indexed_at_c, cid_c] = &v[..] {
                    if let Ok(timestamp) = indexed_at_c.parse::<i64>() {
                        let nanoseconds = 230 * 1000000;
                        let datetime = DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp(timestamp / 1000, nanoseconds),
                            Utc,
                        );
                        let mut timestr = String::new();
                        match write!(timestr, "{}", datetime.format("%+")) {
                            Ok(_) => {
                                let cursor_filter_str = format!(
                                    " AND (
                                  (\"indexedAt\" < '{0}') OR
                                  (\"indexedAt\" = '{0}' AND cid < '{1}')
                              )",
                                    timestr.to_owned(),
                                    cid_c.to_owned()
                                );
                                query_str = format!("{}{}", query_str, cursor_filter_str);
                            }
                            Err(error) => eprintln!("Error formatting: {error:?}"),
                        }
                    }
                } else {
                    let validation_error = ValidationErrorMessageResponse {
                        code: Some(ErrorCode::ValidationError),
                        message: Some("malformed cursor".into()),
                    };
                    return Err(validation_error);
                }
            }
            let order_str = format!(
                " ORDER BY \"indexedAt\" DESC, cid DESC LIMIT {} ",
                limit.unwrap_or(30)
            );
            let query_str = format!("{}{};", &query_str, &order_str);

            let results = sql_query(query_str)
                .load::<Post>(conn)
                .expect("Error loading post records");

            let mut post_results = Vec::new();
            let mut cursor: Option<String> = None;

            // https://docs.rs/chrono/0.4.26/chrono/format/strftime/index.html
            if let Some(last_post) = results.last() {
                if let Ok(parsed_time) = NaiveDateTime::parse_from_str(&last_post.indexed_at, "%+")
                {
                    cursor = Some(format!(
                        "{}::{}",
                        parsed_time.timestamp_millis(),
                        last_post.cid
                    ));
                }
            }

            results
                .into_iter()
                .map(|result| {
                    let post_result = PostResult { post: result.uri };
                    post_results.push(post_result);
                })
                .for_each(drop);

            let new_response = AlgoResponse {
                cursor,
                feed: post_results,
            };
            Ok(new_response)
        })
        .await;

    result
}

#[allow(deprecated)]
pub async fn get_all_posts(
    lang: Option<String>,
    limit: Option<i64>,
    params_cursor: Option<&str>,
    only_posts: bool,
    connection: ReadReplicaConn,
    config: &State<FeedGenConfig>,
) -> Result<AlgoResponse, ValidationErrorMessageResponse> {
    use crate::schema::post::dsl as PostSchema;
    let show_sponsored_post = config.show_sponsored_post.clone();
    let sponsored_post_uri = config.sponsored_post_uri.clone();
    let sponsored_post_probability = config.sponsored_post_probability.clone();

    let params_cursor = match params_cursor {
        None => None,
        Some(params_cursor) => Some(params_cursor.to_string()),
    };

    let result = connection
        .run(move |conn| {
            let mut query = PostSchema::post
                .limit(limit.unwrap_or(30))
                .select(Post::as_select())
                .order((PostSchema::createdAt.desc(), PostSchema::cid.desc()))
                .into_boxed();

            if let Some(lang) = lang {
                query = query.filter(PostSchema::lang.like(format!("%{}%", lang)));
            }

            if params_cursor.is_some() {
                let cursor_str = params_cursor.unwrap();
                let v = cursor_str
                    .split("::")
                    .take(2)
                    .map(String::from)
                    .collect::<Vec<_>>();
                if let [created_at_c, cid_c] = &v[..] {
                    if let Ok(timestamp) = created_at_c.parse::<i64>() {
                        let nanoseconds = 230 * 1000000;
                        let datetime = DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp(timestamp / 1000, nanoseconds),
                            Utc,
                        );
                        let mut timestr = String::new();
                        match write!(timestr, "{}", datetime.format("%+")) {
                            Ok(_) => {
                                query = query.filter(
                                    PostSchema::createdAt.lt(timestr.to_owned()).or(
                                        PostSchema::createdAt
                                            .eq(timestr.to_owned())
                                            .and(PostSchema::cid.lt(cid_c.to_owned())),
                                    ),
                                );
                            }
                            Err(error) => eprintln!("Error formatting: {error:?}"),
                        }
                    }
                } else {
                    let validation_error = ValidationErrorMessageResponse {
                        code: Some(ErrorCode::ValidationError),
                        message: Some("malformed cursor".into()),
                    };
                    return Err(validation_error);
                }
            }
            if only_posts {
                query = query
                    .filter(PostSchema::replyParent.is_null())
                    .filter(PostSchema::replyRoot.is_null());
            }
            let results = query.load(conn).expect("Error loading post records");

            let mut post_results = Vec::new();
            let mut cursor: Option<String> = None;

            // Set the cursor
            if let Some(last_post) = results.last() {
                if let Ok(parsed_time) = NaiveDateTime::parse_from_str(&last_post.created_at, "%+")
                {
                    cursor = Some(format!(
                        "{}::{}",
                        parsed_time.timestamp_millis(),
                        last_post.cid
                    ));
                }
            }

            results
                .into_iter()
                .map(|result| {
                    let post_result = PostResult { post: result.uri };
                    post_results.push(post_result);
                })
                .for_each(drop);

            // Insert the sponsored post if the conditions are met
            if show_sponsored_post && post_results.len() >= 3 && !sponsored_post_uri.is_empty() {
                // Generate a random chance to include the sponsored post based on probability
                let mut rng = rand::thread_rng();
                let random_chance: f64 = rng.gen();

                // Only include the sponsored post if random chance is below the specified probability
                if random_chance < sponsored_post_probability {
                    // Generate a random index to insert the sponsored post (ensure it's not the last position)
                    let replace_index = rng.gen_range(0..(post_results.len() - 1));

                    // Replace a random post with the sponsored post
                    post_results[replace_index] = PostResult {
                        post: sponsored_post_uri.clone(),
                    };
                }
            }

            let new_response = AlgoResponse {
                cursor,
                feed: post_results,
            };
            Ok(new_response)
        })
        .await;

    result
}

pub fn is_included(
    dids: Vec<&String>,
    conn: &mut PgConnection,
) -> Result<bool, Box<dyn std::error::Error>> {
    use crate::schema::membership::dsl::*;

    let result = membership
        .filter(did.eq_any(dids))
        .filter(included.eq(true))
        .limit(1)
        .select(Membership::as_select())
        .load(conn)?;

    if result.len() > 0 {
        Ok(result[0].included)
    } else {
        Ok(false)
    }
}

pub fn is_excluded(
    dids: Vec<&String>,
    conn: &mut PgConnection,
) -> Result<bool, Box<dyn std::error::Error>> {
    use crate::schema::membership::dsl::*;

    let result = membership
        .filter(did.eq_any(dids))
        .filter(excluded.eq(true))
        .limit(1)
        .select(Membership::as_select())
        .load(conn)?;

    if result.len() > 0 {
        Ok(result[0].excluded)
    } else {
        Ok(false)
    }
}

fn extract_hashtags(input: &str) -> HashSet<&str> {
    // Define the regex as a Lazy static variable
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\#[a-zA-Z][0-9a-zA-Z_]*").unwrap());

    // Use the regex to find hashtags in the input
    RE.find_iter(input).map(|mat| mat.as_str()).collect()
}

pub async fn queue_creation(
    lex: String,
    body: Vec<CreateRequest>,
    connection: WriteDbConn,
) -> Result<(), String> {
    use crate::schema::follow::dsl as FollowSchema;
    use crate::schema::image::dsl as ImageSchema;
    use crate::schema::like::dsl as LikeSchema;
    use crate::schema::membership::dsl as MembershipSchema;
    use crate::schema::post::dsl as PostSchema;
    use crate::schema::video::dsl as VideoSchema;

    let result = connection.run( move |conn| {
        if lex == "posts" {
            let mut new_posts = Vec::new();
            let mut new_members = Vec::new();
            let mut new_images = Vec::new();
            let mut new_videos = Vec::new();
            let mut members_to_rm = Vec::new();

            body
                .into_iter()
                .map(|req| {
                    let system_time = SystemTime::now();
                    let dt: DateTime<UtcOffset> = system_time.into();
                    // let mut root_author = String::new();
                    let is_member = is_included(vec![&req.author].into(), conn).unwrap_or(false);
                    let is_blocked = is_excluded(vec![&req.author].into(), conn).unwrap_or(false);
                    let mut post_text = String::new();
                    let mut post_text_original = String::new();
                    let mut post_images = Vec::new();
                    let mut post_videos = Vec::new();
                    let mut new_post = Post {
                        uri: req.uri.clone(),
                        cid: req.cid,
                        reply_parent: None,
                        reply_root: None,
                        indexed_at: format!("{}", dt.format("%+")),
                        prev: req.prev,
                        sequence: req.sequence,
                        text: None,
                        lang: None,
                        author: req.author.clone(),
                        external_uri: None,
                        external_title: None,
                        external_description: None,
                        external_thumb: None,
                        quote_cid: None,
                        quote_uri: None,
                        created_at: format!("{}", dt.format("%+")), // use now() as a default
                    };

                    if let Lexicon::AppBskyFeedPost(post_record) = req.record {
                        post_text_original = post_record.text.clone();
                        post_text = post_record.text.to_lowercase();
                        let post_created_at = format!("{}", post_record.created_at.format("%+"));
                        new_post.created_at = post_created_at.clone();
                        // If posts are received out of order, use indexed_at
                        // mainly capturing created_at for back_dated posts
                        if new_post.created_at > new_post.indexed_at {
                            new_post.created_at = new_post.indexed_at.clone();
                        }
                        if let Some(reply) = post_record.reply {
                            //root_author = reply.root.uri[5..37].into();
                            new_post.reply_parent = Some(reply.parent.uri);
                            new_post.reply_root = Some(reply.root.uri);
                        }
                        if let Some(langs) = post_record.langs {
                            new_post.lang = Some(langs.join(","));
                        }
                        if let Some(embed) = post_record.embed {
                            match embed {
                                Embeds::Images(e) => {
                                    for image in e.images {
                                        let labels: Vec<Option<String>> = vec![];
                                        if let Some(image_cid) = image.image.cid {
                                            let new_image = (
                                                ImageSchema::cid.eq(image_cid),
                                                ImageSchema::alt.eq(image.alt),
                                                ImageSchema::postCid.eq(new_post.cid.clone()),
                                                ImageSchema::postUri.eq(new_post.uri.clone()),
                                                ImageSchema::indexedAt.eq(new_post.indexed_at.clone()),
                                                ImageSchema::createdAt.eq(post_created_at.clone()),
                                                ImageSchema::labels.eq(labels),
                                            );
                                            post_images.push(new_image);
                                        } else {
                                            println!("Legacy image: {image:?}")
                                        };
                                    }
                                },
                                Embeds::Video(ref e) => {
                                    let labels: Vec<Option<String>> = vec![];
                                    match (&e.video.cid, e.video.r#ref) {
                                        (Some(video_cid), _) => {
                                            let new_video = (
                                                VideoSchema::cid.eq(video_cid.clone()),
                                                VideoSchema::alt.eq(e.alt.clone()),
                                                VideoSchema::postCid.eq(new_post.cid.clone()),
                                                VideoSchema::postUri.eq(new_post.uri.clone()),
                                                VideoSchema::indexedAt.eq(new_post.indexed_at.clone()),
                                                VideoSchema::createdAt.eq(post_created_at.clone()),
                                                VideoSchema::labels.eq(labels),
                                            );
                                            post_videos.push(new_video);
                                        },
                                        (_, Some(video_ref)) => {
                                            let new_video = (
                                                VideoSchema::cid.eq(video_ref.to_string()),
                                                VideoSchema::alt.eq(e.alt.clone()),
                                                VideoSchema::postCid.eq(new_post.cid.clone()),
                                                VideoSchema::postUri.eq(new_post.uri.clone()),
                                                VideoSchema::indexedAt.eq(new_post.indexed_at.clone()),
                                                VideoSchema::createdAt.eq(post_created_at.clone()),
                                                VideoSchema::labels.eq(labels),
                                            );
                                            post_videos.push(new_video);
                                        },
                                        _ => eprintln!("Unknown video type: {e:?}")
                                    };
                                }
                                Embeds::RecordWithMedia(e) => {
                                    match e.media {
                                        MediaUnion::Images(m) => {
                                            for image in m.images {
                                                let labels: Vec<Option<String>> = vec![];
                                                if let Some(image_cid) = image.image.cid {
                                                    let new_image = (
                                                        ImageSchema::cid.eq(image_cid),
                                                        ImageSchema::alt.eq(image.alt),
                                                        ImageSchema::postCid.eq(new_post.cid.clone()),
                                                        ImageSchema::postUri.eq(new_post.uri.clone()),
                                                        ImageSchema::indexedAt.eq(new_post.indexed_at.clone()),
                                                        ImageSchema::createdAt.eq(post_created_at.clone()),
                                                        ImageSchema::labels.eq(labels),
                                                    );
                                                    post_images.push(new_image);
                                                } else {
                                                    println!("Legacy image: {image:?}")
                                                };
                                            }
                                        },
                                        MediaUnion::Video(ref v) => {
                                            let labels: Vec<Option<String>> = vec![];
                                            match (&v.video.cid, v.video.r#ref) {
                                                (Some(video_cid), _) => {
                                                    let new_video = (
                                                        VideoSchema::cid.eq(video_cid.clone()),
                                                        VideoSchema::alt.eq(v.alt.clone()),
                                                        VideoSchema::postCid.eq(new_post.cid.clone()),
                                                        VideoSchema::postUri.eq(new_post.uri.clone()),
                                                        VideoSchema::indexedAt.eq(new_post.indexed_at.clone()),
                                                        VideoSchema::createdAt.eq(post_created_at.clone()),
                                                        VideoSchema::labels.eq(labels),
                                                    );
                                                    post_videos.push(new_video);
                                                },
                                                (_, Some(video_ref)) => {
                                                    let new_video = (
                                                        VideoSchema::cid.eq(video_ref.to_string()),
                                                        VideoSchema::alt.eq(v.alt.clone()),
                                                        VideoSchema::postCid.eq(new_post.cid.clone()),
                                                        VideoSchema::postUri.eq(new_post.uri.clone()),
                                                        VideoSchema::indexedAt.eq(new_post.indexed_at.clone()),
                                                        VideoSchema::createdAt.eq(post_created_at.clone()),
                                                        VideoSchema::labels.eq(labels),
                                                    );
                                                    post_videos.push(new_video);
                                                },
                                                _ => eprintln!("Unknown video type: {v:?}")
                                            };
                                        }
                                        MediaUnion::External(e) => {
                                            new_post.external_uri = Some(e.external.uri);
                                            new_post.external_title = Some(e.external.title);
                                            new_post.external_description = Some(e.external.description);
                                            if let Some(thumb_blob) = e.external.thumb {
                                                if let Some(thumb_cid) = thumb_blob.cid {
                                                    new_post.external_thumb = Some(thumb_cid);
                                                };
                                            };
                                        },
                                    }
                                },
                                Embeds::External(e) => {
                                    new_post.external_uri = Some(e.external.uri);
                                    new_post.external_title = Some(e.external.title);
                                    new_post.external_description = Some(e.external.description);
                                    if let Some(thumb_blob) = e.external.thumb {
                                        if let Some(thumb_cid) = thumb_blob.cid {
                                            new_post.external_thumb = Some(thumb_cid);
                                        };
                                    };
                                },
                                Embeds::Record(e) => {
                                    new_post.quote_cid = Some(e.record.cid);
                                    new_post.quote_uri = Some(e.record.uri);
                                },
                            }
                        }
                    }

                    let hashtags = extract_hashtags(&post_text);
                    new_post.text = Some(post_text_original.clone());
                    if (is_member ||
                        hashtags.contains("#blacksky") ||
                        hashtags.contains("#blackhairsky") ||
                        hashtags.contains("#locsky") ||
                        hashtags.contains("#blackbluesky") ||
                        hashtags.contains("#blacktechsky") ||
                        hashtags.contains("#nbablacksky") ||
                        hashtags.contains("#addtoblacksky") ||
                        hashtags.contains("#blackademics") ||
                        hashtags.contains("#addtoblackskytravel") ||
                        hashtags.contains("#blackskytravel") ||
                        hashtags.contains("#addtoblackmedsky") ||
                        hashtags.contains("#blackmedsky") ||
                        hashtags.contains("#addtoblackedusky") ||
                        hashtags.contains("#blackedusky")) &&
                        !is_blocked &&
                        !hashtags.contains("#private") &&
                        !hashtags.contains("#nofeed") &&
                        !hashtags.contains("#removefromblacksky") &&
                        !contains_explicit_slurs(post_text_original.as_str()) {
                        let uri_ = &new_post.uri;
                        let seq_ = &new_post.sequence;
                        println!("Sequence: {seq_:?} | Uri: {uri_:?} | Member: {is_member:?} | Hashtags: {hashtags:?}");

                        let new_post = (
                            PostSchema::uri.eq(new_post.uri),
                            PostSchema::cid.eq(new_post.cid),
                            PostSchema::replyParent.eq(new_post.reply_parent),
                            PostSchema::replyRoot.eq(new_post.reply_root),
                            PostSchema::indexedAt.eq(new_post.indexed_at),
                            PostSchema::prev.eq(new_post.prev),
                            PostSchema::sequence.eq(new_post.sequence),
                            PostSchema::text.eq(new_post.text),
                            PostSchema::lang.eq(new_post.lang),
                            PostSchema::author.eq(new_post.author),
                            PostSchema::externalUri.eq(new_post.external_uri),
                            PostSchema::externalTitle.eq(new_post.external_title),
                            PostSchema::externalDescription.eq(new_post.external_description),
                            PostSchema::externalThumb.eq(new_post.external_thumb),
                            PostSchema::quoteCid.eq(new_post.quote_cid),
                            PostSchema::quoteUri.eq(new_post.quote_uri),
                            PostSchema::createdAt.eq(new_post.created_at),
                        );
                        new_posts.push(new_post);
                        new_images.extend(post_images);
                        new_videos.extend(post_videos);

                        if hashtags.contains("#addtoblacksky") && !is_member {
                            println!("New Blacksky member: {:?}", &req.author);
                            let new_member = (
                                MembershipSchema::did.eq(req.author.clone()),
                                MembershipSchema::included.eq(true),
                                MembershipSchema::excluded.eq(false),
                                MembershipSchema::list.eq("blacksky")
                            );
                            new_members.push(new_member);
                        }
                        if hashtags.contains("#addtoblackskytravel") && !is_member {
                            println!("New BlackskyTravel member: {:?}", &req.author);
                            let new_member = (
                                MembershipSchema::did.eq(req.author.clone()),
                                MembershipSchema::included.eq(true),
                                MembershipSchema::excluded.eq(false),
                                MembershipSchema::list.eq("blacksky-travel")
                            );
                            new_members.push(new_member);
                        }
                        if hashtags.contains("#addtoblackmedsky") && !is_member {
                            println!("New BlackMedSky member: {:?}", &req.author);
                            let new_member = (
                                MembershipSchema::did.eq(req.author.clone()),
                                MembershipSchema::included.eq(true),
                                MembershipSchema::excluded.eq(false),
                                MembershipSchema::list.eq("blacksky-med")
                            );
                            new_members.push(new_member);
                        }
                        if hashtags.contains("#addtoblackedusky") && !is_member {
                            println!("New BlackEduSky member: {:?}", &req.author);
                            let new_member = (
                                MembershipSchema::did.eq(req.author.clone()),
                                MembershipSchema::included.eq(true),
                                MembershipSchema::excluded.eq(false),
                                MembershipSchema::list.eq("blacksky-scholastic")
                            );
                            new_members.push(new_member);
                        }
                        /* TEMP REMOVING THIS FEATURE AS IT'S CREATING SPAM
                        if hashtags.contains("#addtoblacksky") &&
                            is_member &&
                            !root_author.is_empty() {
                            println!("New member: {:?}", &root_author);
                            let new_member = (
                                MembershipSchema::did.eq(root_author),
                                MembershipSchema::included.eq(true),
                                MembershipSchema::excluded.eq(false),
                                MembershipSchema::list.eq("blacksky")
                            );
                            new_members.push(new_member);
                        }*/
                    }
                    if is_member &&
                        hashtags.contains("#removefromblacksky") {
                        println!("Removing member: {:?}", &req.author);
                        members_to_rm.push(req.author.clone());
                    }
                    // @TODO: Report to Ozone
                    if contains_explicit_slurs(post_text_original.as_str()) {
                        println!(
                            "@LOG: EXPLICIT SLUR DETECTED: text:`{}`; uri:`{}`;",
                            post_text_original,
                            req.uri
                        );
                    }
                })
                .for_each(drop);

            diesel::insert_into(PostSchema::post)
                .values(&new_posts)
                .on_conflict(PostSchema::uri)
                .do_nothing()
                .execute(conn)
                .expect("Error inserting post records");

            diesel::insert_into(ImageSchema::image)
                .values(&new_images)
                .on_conflict(ImageSchema::cid)
                .do_nothing()
                .execute(conn)
                .expect("Error inserting image records");

            diesel::insert_into(VideoSchema::video)
                .values(&new_videos)
                .on_conflict(VideoSchema::cid)
                .do_nothing()
                .execute(conn)
                .expect("Error inserting video records");

            diesel::insert_into(MembershipSchema::membership)
                .values(&new_members)
                .on_conflict((MembershipSchema::did,MembershipSchema::list))
                .do_nothing()
                .execute(conn)
                .expect("Error inserting member records");

            diesel::delete(MembershipSchema::membership
                .filter(MembershipSchema::did.eq_any(&members_to_rm)))
                .execute(conn)
                .expect("Error deleting member records");
            Ok(())
        } else if lex == "likes" {
            let mut new_likes = Vec::new();

            body
                .into_iter()
                .map(|req| {
                    if let Lexicon::AppBskyFeedLike(like_record) = req.record {
                        let subject_author: &String = &like_record.subject.uri[5..37].into(); // parse DID:PLC from URI
                        let is_member = is_included(vec![&req.author, subject_author].into(), conn).unwrap_or(false);
                        if is_member {
                            let system_time = SystemTime::now();
                            let dt: DateTime<UtcOffset> = system_time.into();
                            let new_like = (
                                LikeSchema::uri.eq(req.uri),
                                LikeSchema::cid.eq(req.cid),
                                LikeSchema::author.eq(req.author),
                                LikeSchema::subjectCid.eq(like_record.subject.cid),
                                LikeSchema::subjectUri.eq(like_record.subject.uri),
                                LikeSchema::createdAt.eq(like_record.created_at),
                                LikeSchema::indexedAt.eq(format!("{}", dt.format("%+"))),
                                LikeSchema::prev.eq(req.prev),
                                LikeSchema::sequence.eq(req.sequence)
                            );
                            new_likes.push(new_like);
                        }
                    }
                })
                .for_each(drop);

            diesel::insert_into(LikeSchema::like)
                .values(&new_likes)
                .on_conflict(LikeSchema::uri)
                .do_nothing()
                .execute(conn)
                .expect("Error inserting like records");

            Ok(())
        } else if lex == "follows" {
            let mut new_follows = Vec::new();

            body
                .into_iter()
                .map(|req| {
                    if let Lexicon::AppBskyFeedFollow(follow_record) = req.record {
                        let is_member = is_included(vec![&req.author, &follow_record.subject].into(), conn).unwrap_or(false);
                        if is_member {
                            let system_time = SystemTime::now();
                            let dt: DateTime<UtcOffset> = system_time.into();
                            let new_follow = (
                                FollowSchema::uri.eq(req.uri),
                                FollowSchema::cid.eq(req.cid),
                                FollowSchema::author.eq(req.author),
                                FollowSchema::subject.eq(follow_record.subject),
                                FollowSchema::createdAt.eq(follow_record.created_at),
                                FollowSchema::indexedAt.eq(format!("{}", dt.format("%+"))),
                                FollowSchema::prev.eq(req.prev),
                                FollowSchema::sequence.eq(req.sequence)
                            );
                            new_follows.push(new_follow);
                        }
                    }
                })
                .for_each(drop);

            diesel::insert_into(FollowSchema::follow)
                .values(&new_follows)
                .on_conflict(FollowSchema::uri)
                .do_nothing()
                .execute(conn)
                .expect("Error inserting like records");

            Ok(())
        } else {
            Err(format!("Unknown lexicon received {lex:?}"))
        }
    }).await;
    result
}

pub async fn queue_deletion(
    lex: String,
    body: Vec<DeleteRequest>,
    connection: WriteDbConn,
) -> Result<(), String> {
    use crate::schema::follow::dsl as FollowSchema;
    use crate::schema::like::dsl as LikeSchema;
    use crate::schema::post::dsl as PostSchema;

    let result = connection
        .run(move |conn| {
            let mut delete_rows = Vec::new();
            body.into_iter()
                .map(|req| {
                    delete_rows.push(req.uri);
                })
                .for_each(drop);
            if lex == "posts" {
                diesel::delete(PostSchema::post.filter(PostSchema::uri.eq_any(delete_rows)))
                    .execute(conn)
                    .expect("Error deleting post records");
            } else if lex == "likes" {
                diesel::delete(LikeSchema::like.filter(LikeSchema::uri.eq_any(delete_rows)))
                    .execute(conn)
                    .expect("Error deleting like records");
            } else if lex == "follows" {
                diesel::delete(FollowSchema::follow.filter(FollowSchema::uri.eq_any(delete_rows)))
                    .execute(conn)
                    .expect("Error deleting follow records");
            } else {
                eprintln!("Unknown lexicon received {lex:?}");
            }
            Ok(())
        })
        .await;
    result
}

pub async fn update_cursor(
    service_: String,
    sequence: i64,
    connection: WriteDbConn,
) -> Result<(), String> {
    use crate::schema::sub_state::dsl::*;

    let result = connection
        .run(move |conn| {
            let update_state = (service.eq(service_), cursor.eq(&sequence));

            diesel::insert_into(sub_state)
                .values(&update_state)
                .on_conflict(service)
                .do_update()
                .set(cursor.eq(&sequence))
                .execute(conn)
                .expect("Error updating cursor records");
            Ok(())
        })
        .await;

    result
}

pub fn add_visitor(
    user: String,
    service: String,
    requested_feed: String,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::schema::visitor::dsl::*;

    let connection = &mut establish_connection()?;

    let system_time = SystemTime::now();
    let dt: DateTime<UtcOffset> = system_time.into();
    let new_visitor = (
        did.eq(user),
        web.eq(service),
        visited_at.eq(format!("{}", dt.format("%+"))),
        feed.eq(requested_feed),
    );

    diesel::insert_into(visitor)
        .values(&new_visitor)
        .execute(connection)?;
    Ok(())
}

pub fn is_banned_from_tv(subject: &String) -> Result<bool, Box<dyn std::error::Error>> {
    use crate::schema::banned_from_tv::dsl::*;

    let connection = &mut establish_connection()?;

    let subject_to_check = subject.clone();
    let count = banned_from_tv
        .filter(did.eq(subject_to_check))
        .count()
        .get_result(connection)
        .unwrap_or(0);

    return if count > 0 { Ok(true) } else { Ok(false) };
}

pub async fn get_cursor(
    service_: String,
    connection: ReadReplicaConn,
) -> Result<SubState, PathUnknownErrorMessageResponse> {
    use crate::schema::sub_state::dsl::*;

    let result = connection
        .run(move |conn| {
            let mut result = sub_state
                .filter(service.eq(service_))
                .order(cursor.desc())
                .limit(1)
                .select(SubState::as_select())
                .load(conn)
                .expect("Error loading cursor records");

            if let Some(cursor_) = result.pop() {
                Ok(cursor_)
            } else {
                let not_found_error = crate::models::PathUnknownErrorMessageResponse {
                    code: Some(crate::models::NotFoundErrorCode::NotFoundError),
                    message: Some("Not found.".into()),
                };
                Err(not_found_error)
            }
        })
        .await;

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::{index, BLACKSKY};
    use rocket::figment::map;
    use rocket::figment::value::{Map, Value};
    use rocket::http::Status;
    use rocket::local::asynchronous::Client;
    use rocket::{routes, Build, Rocket};
    use std::env;
    use temp_env::async_with_vars;

    fn before(config: FeedGenConfig) -> Rocket<Build> {
        // Initialize Rocket with ReadReplicaConn and other necessary fairings/routes
        let write_database_url = env::var("DATABASE_URL").unwrap();
        let read_database_url = env::var("READ_REPLICA_URL").unwrap();

        let write_db: Map<_, Value> = map! {
            "url" => write_database_url.into(),
            "pool_size" => 20.into(),
            "timeout" => 30.into(),
        };

        let read_db: Map<_, Value> = map! {
            "url" => read_database_url.into(),
            "pool_size" => 20.into(),
            "timeout" => 30.into(),
        };

        let figment = rocket::Config::figment().merge((
            "databases",
            map!["pg_read_replica" => read_db, "pg_db" => write_db],
        ));

        rocket::custom(figment)
            .attach(WriteDbConn::fairing())
            .attach(ReadReplicaConn::fairing())
            .mount("/", routes![index])
            .manage(config)
    }

    #[rocket::async_test]
    async fn test_no_sponsored_post_when_show_sponsored_post_is_false() {
        // Set environment variables temporarily using temp_env for this test
        async_with_vars(
            vec![
                (
                    "DATABASE_URL",
                    Some("postgresql://postgres@localhost:5432/local"),
                ),
                (
                    "READ_REPLICA_URL",
                    Some("postgresql://postgres@localhost:5432/local"),
                ),
            ],
            async {
                let config = FeedGenConfig {
                    show_sponsored_post: false,
                    sponsored_post_uri: "at://did:example/sponsored-post".to_string(),
                    sponsored_post_probability: 1.0,
                };
                let rocket = before(config.clone());

                // Create a client for testing
                let client = Client::tracked(rocket)
                    .await
                    .expect("valid rocket instance");

                // Make a request to the `get_all_posts` endpoint (adjust the route as necessary)
                let response = client
                    .get(format!(
                        "/xrpc/app.bsky.feed.getFeedSkeleton?feed={}",
                        BLACKSKY
                    ))
                    .dispatch()
                    .await;

                // Ensure the request succeeded
                assert_eq!(
                    response.status(),
                    Status::Ok,
                    "{}",
                    format!("{:?}", response.into_string().await)
                );

                // Extract the response body and check that the sponsored post is not present
                let body = response.into_json::<AlgoResponse>().await.unwrap();
                assert!(
                    !body
                        .feed
                        .iter()
                        .any(|post| &post.post == &config.sponsored_post_uri),
                    "Sponsored post should not be returned"
                );
            },
        )
        .await;
    }

    #[rocket::async_test]
    async fn test_sponsored_post_always_returned_when_probability_is_1() {
        async_with_vars(
            vec![
                (
                    "DATABASE_URL",
                    Some("postgresql://postgres@localhost:5432/local"),
                ),
                (
                    "READ_REPLICA_URL",
                    Some("postgresql://postgres@localhost:5432/local"),
                ),
            ],
            async {
                let config = FeedGenConfig {
                    show_sponsored_post: true,
                    sponsored_post_uri: "at://did:example/sponsored-post".to_string(),
                    sponsored_post_probability: 1.0,
                };
                let rocket = before(config.clone());

                // Create a client for testing
                let client = Client::tracked(rocket)
                    .await
                    .expect("valid rocket instance");

                // Make a request to the `get_all_posts` endpoint (adjust the route as necessary)
                let response = client
                    .get(format!(
                        "/xrpc/app.bsky.feed.getFeedSkeleton?feed={}",
                        BLACKSKY
                    ))
                    .dispatch()
                    .await;

                // Ensure the request succeeded
                assert_eq!(
                    response.status(),
                    Status::Ok,
                    "{}",
                    format!("{:?}", response.into_string().await)
                );

                // Extract the response body and check that the sponsored post is not present
                let body = response.into_json::<AlgoResponse>().await.unwrap();
                assert!(
                    body.feed
                        .iter()
                        .any(|post| &post.post == &config.sponsored_post_uri),
                    "Sponsored post should be returned"
                );
            },
        )
        .await;
    }

    #[rocket::async_test]
    async fn test_sponsored_post_never_returned_when_limit_is_2() {
        async_with_vars(
            vec![
                (
                    "DATABASE_URL",
                    Some("postgresql://postgres@localhost:5432/local"),
                ),
                (
                    "READ_REPLICA_URL",
                    Some("postgresql://postgres@localhost:5432/local"),
                ),
            ],
            async {
                let config = FeedGenConfig {
                    show_sponsored_post: true,
                    sponsored_post_uri: "at://did:example/sponsored-post".to_string(),
                    sponsored_post_probability: 1.0,
                };
                let rocket = before(config.clone());

                // Create a client for testing
                let client = Client::tracked(rocket)
                    .await
                    .expect("valid rocket instance");

                // Make a request to the `get_all_posts` endpoint (adjust the route as necessary)
                let response = client
                    .get(format!(
                        "/xrpc/app.bsky.feed.getFeedSkeleton?feed={}&limit=2",
                        BLACKSKY
                    ))
                    .dispatch()
                    .await;

                // Ensure the request succeeded
                assert_eq!(
                    response.status(),
                    Status::Ok,
                    "{}",
                    format!("{:?}", response.into_string().await)
                );

                // Extract the response body and check that the sponsored post is not present
                let body = response.into_json::<AlgoResponse>().await.unwrap();
                assert!(
                    !body
                        .feed
                        .iter()
                        .any(|post| &post.post == &config.sponsored_post_uri),
                    "Sponsored post should not be returned when limit is 2"
                );
            },
        )
        .await;
    }

    #[rocket::async_test]
    async fn test_sponsored_post_returned_50_percent_of_the_time() {
        async_with_vars(
            vec![
                (
                    "DATABASE_URL",
                    Some("postgresql://postgres@localhost:5432/local"),
                ),
                (
                    "READ_REPLICA_URL",
                    Some("postgresql://postgres@localhost:5432/local"),
                ),
            ],
            async {
                let config = FeedGenConfig {
                    show_sponsored_post: true,
                    sponsored_post_uri: "at://did:example/sponsored-post".to_string(),
                    sponsored_post_probability: 0.5,
                };
                let rocket = before(config.clone());

                // Create a client for testing
                let client = Client::tracked(rocket)
                    .await
                    .expect("valid rocket instance");

                let mut sponsored_count = 0;
                let iterations = 100;
                let mut cursor: Option<String> = None;

                for _ in 0..iterations {
                    let path = match cursor {
                        None => format!(
                            "/xrpc/app.bsky.feed.getFeedSkeleton?feed={}&limit=5",
                            BLACKSKY
                        ),
                        Some(cursor) => format!(
                            "/xrpc/app.bsky.feed.getFeedSkeleton?feed={}&limit=5&cursor={}",
                            BLACKSKY, cursor
                        ),
                    };
                    let response = client.get(path).dispatch().await;
                    let body = response.into_json::<AlgoResponse>().await.unwrap();

                    if body
                        .feed
                        .iter()
                        .any(|post| &post.post == &config.sponsored_post_uri)
                    {
                        sponsored_count += 1;
                    }
                    if let Some(c) = body.cursor {
                        cursor = Some(c);
                    } else {
                        cursor = None;
                    }
                }

                let proportion = sponsored_count as f64 / iterations as f64;
                assert!(
                    (0.45..=0.55).contains(&proportion),
                    "Sponsored post should be returned ~50% of the time, actual: {}",
                    proportion
                );
            },
        )
        .await;
    }

    #[rocket::async_test]
    async fn test_sponsored_post_never_last() {
        async_with_vars(
            vec![
                (
                    "DATABASE_URL",
                    Some("postgresql://postgres@localhost:5432/local"),
                ),
                (
                    "READ_REPLICA_URL",
                    Some("postgresql://postgres@localhost:5432/local"),
                ),
            ],
            async {
                let config = FeedGenConfig {
                    show_sponsored_post: true,
                    sponsored_post_uri: "at://did:example/sponsored-post".to_string(),
                    sponsored_post_probability: 1.0,
                };
                let rocket = before(config.clone());

                // Create a client for testing
                let client = Client::tracked(rocket)
                    .await
                    .expect("valid rocket instance");

                let mut times_last = 0;
                let iterations = 100;
                let mut cursor: Option<String> = None;

                for _ in 0..iterations {
                    let path = match cursor {
                        None => format!(
                            "/xrpc/app.bsky.feed.getFeedSkeleton?feed={}&limit=5",
                            BLACKSKY
                        ),
                        Some(cursor) => format!(
                            "/xrpc/app.bsky.feed.getFeedSkeleton?feed={}&limit=5&cursor={}",
                            BLACKSKY, cursor
                        ),
                    };
                    let response = client.get(path).dispatch().await;
                    let body = response.into_json::<AlgoResponse>().await.unwrap();

                    if let Some(last_post) = body.feed.iter().last() {
                        if &last_post.post == &config.sponsored_post_uri {
                            times_last += 1;
                        }
                    }

                    if let Some(c) = body.cursor {
                        cursor = Some(c);
                    } else {
                        cursor = None;
                    }
                }

                assert_eq!(times_last, 0);
            },
        )
        .await;
    }
}
