use crate::db::*;
use crate::models::*;
use crate::{ReadReplicaConn, WriteDbConn};
use chrono::offset::Utc as UtcOffset;
use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::prelude::*;
use std::fmt::Write;
use std::time::SystemTime;
use lexicon::app::bsky::feed::{Embeds, Media};


#[allow(deprecated)]
pub async fn get_posts(
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
                query = query
                    .filter(PostSchema::lang.like(format!("%{}%", lang)));
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
                                query = query
                                    .filter(PostSchema::indexedAt.le(timestr.to_owned()))
                                    .filter(PostSchema::cid.lt(cid_c.to_owned()));
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

pub async fn queue_creation(
    lex: String,
    body: Vec<CreateRequest>,
    connection: WriteDbConn,
) -> Result<(), String> {
    use crate::schema::like::dsl as LikeSchema;
    use crate::schema::post::dsl as PostSchema;
    use crate::schema::follow::dsl as FollowSchema;
    use crate::schema::image::dsl as ImageSchema;

    let result = connection.run( move |conn| {
        if lex == "posts" {
            let mut new_posts = Vec::new();
            let mut new_images = Vec::new();

            body
                .into_iter()
                .map(|req| {
                    let system_time = SystemTime::now();
                    let dt: DateTime<UtcOffset> = system_time.into();
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
                        lang: None
                    };

                    if let Lexicon::AppBskyFeedPost(post_record) = req.record {
                        post_text = post_record.text.to_lowercase();
                        let post_created_at = format!("{}", post_record.created_at.format("%+"));
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
                                        _ => (),
                                    }
                                },
                                _ => (),
                            }
                        }
                    }

                    new_post.text = Some(post_text.clone());

                    let new_post = (
                        PostSchema::uri.eq(new_post.uri),
                        PostSchema::cid.eq(new_post.cid),
                        PostSchema::replyParent.eq(new_post.reply_parent),
                        PostSchema::replyRoot.eq(new_post.reply_root),
                        PostSchema::indexedAt.eq(new_post.indexed_at),
                        PostSchema::prev.eq(new_post.prev),
                        PostSchema::sequence.eq(new_post.sequence),
                        PostSchema::text.eq(new_post.text),
                        PostSchema::lang.eq(new_post.lang)
                    );
                    new_posts.push(new_post);
                    new_images.extend(post_images);

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

            Ok(())
        } else if lex == "likes" {
            let mut new_likes = Vec::new();

            body
                .into_iter()
                .map(|req| {
                    if let Lexicon::AppBskyFeedLike(like_record) = req.record {
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
    use crate::schema::like::dsl as LikeSchema;
    use crate::schema::post::dsl as PostSchema;
    use crate::schema::follow::dsl as FollowSchema;

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

pub fn add_visitor(user: String, service: String, requested_feed: String) -> Result<(), Box<dyn std::error::Error>> {
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
