use crate::db::*;
use crate::models::*;
use crate::{ReadReplicaConn, WriteDbConn};
use chrono::offset::Utc;
use chrono::DateTime;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashSet;
use std::time::SystemTime;

pub async fn get_blacksky_posts(
    limit: Option<i64>,
    params_cursor: Option<String>,
    connection: ReadReplicaConn,
) -> Result<AlgoResponse, ValidationErrorMessageResponse> {
    use crate::schema::post::dsl::*;

    let result = connection
        .run(move |conn| {
            let mut query = post
                .limit(limit.unwrap_or(50))
                .select(Post::as_select())
                .order((indexedAt.desc(), cid.desc()))
                .into_boxed();

            if params_cursor.is_some() {
                let cursor_str = params_cursor.unwrap();
                let v = cursor_str
                    .split("::")
                    .take(2)
                    .map(String::from)
                    .collect::<Vec<_>>();
                if let [indexed_at_c, cid_c] = &v[..] {
                    let timestamp = indexed_at_c.parse::<i64>().unwrap();
                    if let Some(dt) = NaiveDateTime::from_timestamp_opt(timestamp, 0) {
                        let timestr = format!("{}", dt.format("%+"));
                        query = query
                            .filter(indexedAt.le(timestr.to_owned()))
                            .filter(cid.lt(cid_c.to_owned()))
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

pub fn is_included(did_: &String, list_: String) -> Result<bool, Box<dyn std::error::Error>> {
    use crate::schema::membership::dsl::*;

    let connection = &mut establish_connection()?;
    let result = membership
        .filter(did.eq(did_.to_string()))
        .filter(list.eq(list_))
        .filter(included.eq(true))
        .limit(1)
        .select(Membership::as_select())
        .load(connection)?;

    if result.len() > 0 {
        Ok(result[0].included)
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
    body: Vec<CreateRequest>,
    connection: WriteDbConn,
) -> Result<(), String> {
    use crate::schema::membership::dsl::*;
    use crate::schema::post::dsl::*;

    let result = connection.run( move |conn| {

        let mut new_posts = Vec::new();
        let mut new_members = Vec::new();
        let mut hellthread_roots = HashSet::new();
        hellthread_roots.insert("bafyreigxvsmbhdenvzaklcfnovbsjc542cu5pjmpqyyc64mdtqwsyimlvi".to_string());

        body
            .into_iter()
            .map(|req_post| {
                let system_time = SystemTime::now();
                let dt: DateTime<Utc> = system_time.into();
                let mut is_hellthread = false;
                let is_blacksky_author = is_included(&req_post.author,"blacksky".into()).unwrap_or(false);
                let post_text: String = req_post.record.text.to_lowercase();
                let hashtags = extract_hashtags(&post_text);

                let mut new_post = Post {
                    uri: req_post.uri,
                    cid: req_post.cid,
                    reply_parent: None,
                    reply_root: None,
                    indexed_at: format!("{}", dt.format("%+")),
                    prev: req_post.prev,
                    sequence: req_post.sequence
                };
                if let Some(reply) = req_post.record.reply {
                    new_post.reply_parent = Some(reply.parent.uri);
                    new_post.reply_root = Some(reply.root.uri);
                    is_hellthread = hellthread_roots.contains(&reply.root.cid);
                }

                if (is_blacksky_author ||
                    hashtags.contains("#blacksky") ||
                    hashtags.contains("#blacktechsky") ||
                    hashtags.contains("#nbablacksky") ||
                    hashtags.contains("#addtoblacksky")) && !is_hellthread  {
                    let uri_ = &new_post.uri;
                    let seq_ = &new_post.sequence;
                    println!("Sequence: {seq_:?} | Uri: {uri_:?} | Blacksky: {is_blacksky_author:?} | Hellthread: {is_hellthread:?} | Hashtags: {hashtags:?}");

                    let new_post = (
                        uri.eq(new_post.uri),
                        cid.eq(new_post.cid),
                        replyParent.eq(new_post.reply_parent),
                        replyRoot.eq(new_post.reply_root),
                        indexedAt.eq(new_post.indexed_at),
                        prev.eq(new_post.prev),
                        sequence.eq(new_post.sequence)
                    );
                    new_posts.push(new_post);

                    if hashtags.contains("#addtoblacksky") && !is_blacksky_author {
                        println!("New member: {:?}", &req_post.author);
                        let new_member = (
                            did.eq(req_post.author),
                            included.eq(true),
                            excluded.eq(false),
                            list.eq("blacksky")
                        );
                        new_members.push(new_member);
                    }
                }
            })
            .for_each(drop);

        diesel::insert_into(post)
            .values(&new_posts)
            .on_conflict(uri)
            .do_nothing()
            .execute(conn)
            .expect("Error inserting post records");

        diesel::insert_into(membership)
            .values(&new_members)
            .on_conflict(did)
            .do_nothing()
            .execute(conn)
            .expect("Error inserting member records");
        Ok(())
    }).await;

    result
}

pub async fn queue_deletion(
    body: Vec<DeleteRequest>,
    connection: WriteDbConn,
) -> Result<(), String> {
    use crate::schema::post::dsl::*;

    let result = connection
        .run(move |conn| {
            let mut delete_posts = Vec::new();
            body.into_iter()
                .map(|req_post| {
                    delete_posts.push(req_post.uri);
                })
                .for_each(drop);

            diesel::delete(post.filter(uri.eq_any(delete_posts)))
                .execute(conn)
                .expect("Error deleting post records");
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
) -> Result<(), Box<dyn std::error::Error>>  {
    use crate::schema::visitor::dsl::*;

    let connection = &mut establish_connection()?;

    let system_time = SystemTime::now();
    let dt: DateTime<Utc> = system_time.into();
    let new_visitor = (did.eq(user), web.eq(service), visited_at.eq(format!("{}", dt.format("%+"))));

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
