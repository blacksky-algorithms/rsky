use crate::db::*;
use crate::models::*;
use crate::{ReadReplicaConn, WriteDbConn};
use chrono::offset::Utc as UtcOffset;
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use diesel::prelude::*;
use diesel::sql_query;
use lazy_static::lazy_static;
use regex::Regex;
use rsky_lexicon::app::bsky::feed::{Embeds, Media};
use std::collections::HashSet;
use std::fmt::Write;
use std::time::SystemTime;

#[allow(deprecated)]
pub async fn get_posts_by_membership(
    lang: Option<String>,
    limit: Option<i64>,
    params_cursor: Option<String>,
    only_posts: bool,
    list: String,
    connection: ReadReplicaConn,
) -> Result<AlgoResponse, ValidationErrorMessageResponse> {
    use crate::schema::membership::dsl as MembershipSchema;
    use crate::schema::post::dsl as PostSchema;

    let result = connection
        .run(move |conn| {
            let mut query = PostSchema::post
                .inner_join(
                    MembershipSchema::membership.on(PostSchema::author
                        .eq(MembershipSchema::did)
                        .and(MembershipSchema::list.eq(list))
                        .and(MembershipSchema::included.eq(true))),
                )
                .limit(limit.unwrap_or(30))
                .select(Post::as_select())
                .order((PostSchema::indexedAt.desc(), PostSchema::cid.desc()))
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
                                query = query.filter(
                                    PostSchema::indexedAt.lt(timestr.to_owned()).or(
                                        PostSchema::indexedAt
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
                cursor: cursor,
                feed: post_results,
            };
            Ok(new_response)
        })
        .await;

    result
}

#[allow(deprecated)]
pub async fn get_blacksky_nsfw(
    limit: Option<i64>,
    params_cursor: Option<String>,
    connection: ReadReplicaConn,
) -> Result<AlgoResponse, ValidationErrorMessageResponse> {
    use crate::schema::image::dsl as ImageSchema;
    use crate::schema::post::dsl as PostSchema;

    let result = connection
        .run(move |conn| {
            let mut query = PostSchema::post
                .limit(limit.unwrap_or(30))
                .select(Post::as_select())
                .order((PostSchema::indexedAt.desc(), PostSchema::cid.desc()))
                .into_boxed();

            query = query.filter(
                PostSchema::cid.eq_any(
                    ImageSchema::image
                        .filter(ImageSchema::labels.contains(vec!["sexy"]))
                        .filter(ImageSchema::alt.is_not_null())
                        .select(ImageSchema::postCid),
                ),
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
                                query = query.filter(
                                    PostSchema::indexedAt.lt(timestr.to_owned()).or(
                                        PostSchema::indexedAt
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

            let results = query.load(conn).expect("Error loading post records");

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
                cursor: cursor,
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
    params_cursor: Option<String>,
    connection: ReadReplicaConn,
) -> Result<AlgoResponse, ValidationErrorMessageResponse> {
    let result = connection
        .run(move |conn| {
            let dt = Utc::now() - Duration::days(2);
            let mut query_str = format!("SELECT
                hydrated.uri,
                hydrated.cid,
                hydrated.\"replyParent\",
                hydrated.\"replyRoot\",
                hydrated.trendingDate as \"indexedAt\",
                hydrated.prev,
                hydrated.\"sequence\",
                hydrated.\"text\",
                hydrated.lang,
                hydrated.author,
                hydrated.\"externalUri\",
                hydrated.\"externalTitle\",
                hydrated.\"externalDescription\",
                hydrated.\"externalThumb\",
                hydrated.\"quoteCid\",
                hydrated.\"quoteUri\"
            FROM(
                SELECT
                    post.*,
                    twelfth.\"indexedAt\" as trendingDate 
                FROM post
                JOIN (
                    SELECT public.like.\"subjectUri\", public.like.\"indexedAt\", ROW_NUMBER() OVER (PARTITION BY public.like.\"subjectUri\" ORDER BY public.like.\"indexedAt\" NULLS LAST) AS RowNum FROM public.like
                ) twelfth
                    ON twelfth.\"subjectUri\" = post.uri
                        and twelfth.RowNum = 12
                WHERE post.\"indexedAt\" > '{0}'
            ) hydrated", dt.format("%F"));

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
                                let cursor_filter_str = format!(" WHERE ((hydrated.trendingDate < '{0}') OR (hydrated.trendingDate = '{0}' AND hydrated.cid < '{1}'))", timestr.to_owned(), cid_c.to_owned());
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
            let order_str = format!(" ORDER BY hydrated.trendingDate DESC, hydrated.cid DESC LIMIT {} ", limit.unwrap_or(30));
            let query_str = format!("{}{};", &query_str, &order_str);

            let results = sql_query(query_str)
                .load::<crate::models::Post>(conn)
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
                cursor: cursor,
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
    params_cursor: Option<String>,
    only_posts: bool,
    connection: ReadReplicaConn,
) -> Result<AlgoResponse, ValidationErrorMessageResponse> {
    use crate::schema::post::dsl as PostSchema;

    let result = connection
        .run(move |conn| {
            let mut query = PostSchema::post
                .limit(limit.unwrap_or(30))
                .select(Post::as_select())
                .order((PostSchema::indexedAt.desc(), PostSchema::cid.desc()))
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
                                query = query.filter(
                                    PostSchema::indexedAt.lt(timestr.to_owned()).or(
                                        PostSchema::indexedAt
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
                cursor: cursor,
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
    lazy_static! {
        static ref RE: Regex = Regex::new(r"\#[a-zA-Z][0-9a-zA-Z_]*").unwrap();
    }
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

    let result = connection.run( move |conn| {
        if lex == "posts" {
            let mut new_posts = Vec::new();
            let mut new_members = Vec::new();
            let mut new_images = Vec::new();
            let mut members_to_rm = Vec::new();
            let mut hellthread_roots = HashSet::new();
            hellthread_roots.insert("bafyreigxvsmbhdenvzaklcfnovbsjc542cu5pjmpqyyc64mdtqwsyimlvi".to_string());

            body
                .into_iter()
                .map(|req| {
                    let system_time = SystemTime::now();
                    let dt: DateTime<UtcOffset> = system_time.into();
                    let mut is_hellthread = false;
                    let mut root_author = String::new();
                    let is_member = is_included(vec![&req.author].into(), conn).unwrap_or(false);
                    let is_blocked = is_excluded(vec![&req.author].into(), conn).unwrap_or(false);
                    let mut post_text = String::new();
                    let mut post_images = Vec::new();
                    let mut new_post = Post {
                        uri: req.uri,
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
                        quote_uri: None
                    };

                    if let Lexicon::AppBskyFeedPost(post_record) = req.record {
                        post_text = post_record.text.to_lowercase();
                        let post_created_at = format!("{}", post_record.created_at.format("%+"));
                        if let Some(reply) = post_record.reply {
                            root_author = reply.root.uri[5..37].into();
                            new_post.reply_parent = Some(reply.parent.uri);
                            new_post.reply_root = Some(reply.root.uri);
                            is_hellthread = hellthread_roots.contains(&reply.root.cid);
                        }
                        if let Some(langs) = post_record.langs {
                            new_post.lang = Some(langs.join(","));
                        }
                        if let Some(embed) = post_record.embed {
                            match embed {
                                Embeds::Images(e) => {
                                    for image in e.images {
                                        let labels: Vec<Option<String>> = vec![];
                                        if let Some(image_cid) = image.image.r#ref {
                                            let new_image = (
                                                ImageSchema::cid.eq(image_cid.to_string()),
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
                                Embeds::RecordWithMedia(e) => {
                                    match e.media {
                                        Media::Images(m) => {
                                            for image in m.images {
                                                let labels: Vec<Option<String>> = vec![];
                                                if let Some(image_cid) = image.image.r#ref {
                                                    let new_image = (
                                                        ImageSchema::cid.eq(image_cid.to_string()),
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
                                        Media::External(e) => {
                                            new_post.external_uri = Some(e.external.uri);
                                            new_post.external_title = Some(e.external.title);
                                            new_post.external_description = Some(e.external.description);
                                            if let Some(thumb_blob) = e.external.thumb {
                                                if let Some(thumb_cid) = thumb_blob.r#ref {
                                                    new_post.external_thumb = Some(thumb_cid.to_string());
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
                                        if let Some(thumb_cid) = thumb_blob.r#ref {
                                            new_post.external_thumb = Some(thumb_cid.to_string());
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
                    new_post.text = Some(post_text.clone());

                    if (is_member ||
                        hashtags.contains("#blacksky") ||
                        hashtags.contains("#blacktechsky") ||
                        hashtags.contains("#nbablacksky") ||
                        hashtags.contains("#addtoblacksky")) && 
                        !is_blocked &&
                        !is_hellthread &&
                        !hashtags.contains("#private") && 
                        !hashtags.contains("#nofeed") && 
                        !hashtags.contains("#removefromblacksky") {
                        let uri_ = &new_post.uri;
                        let seq_ = &new_post.sequence;
                        println!("Sequence: {seq_:?} | Uri: {uri_:?} | Member: {is_member:?} | Hellthread: {is_hellthread:?} | Hashtags: {hashtags:?}");

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
                        );
                        new_posts.push(new_post);
                        new_images.extend(post_images);

                        if hashtags.contains("#addtoblacksky") && !is_member {
                            println!("New member: {:?}", &req.author);
                            let new_member = (
                                MembershipSchema::did.eq(req.author.clone()),
                                MembershipSchema::included.eq(true),
                                MembershipSchema::excluded.eq(false),
                                MembershipSchema::list.eq("blacksky")
                            );
                            new_members.push(new_member);
                        }

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
                        }
                    }
                    if is_member &&
                        hashtags.contains("#removefromblacksky") &&
                        !is_hellthread {
                        println!("Removing member: {:?}", &req.author);
                        members_to_rm.push(req.author.clone());
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
