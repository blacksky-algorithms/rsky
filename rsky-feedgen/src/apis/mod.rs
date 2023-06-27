use crate::models::*;
use diesel::prelude::*;
use crate::db::*;
use chrono::{ NaiveDateTime};
use chrono::offset::Utc;
use chrono::DateTime;
use std::time::SystemTime;
use std::collections::HashSet;
use regex::Regex;
use lazy_static::lazy_static;

pub async fn get_blacksky_posts (
    limit: Option<i64>,
    params_cursor: Option<String>,
) -> Result<AlgoResponse, Box<dyn std::error::Error>> {
    use crate::schema::post::dsl::*;

    let connection = &mut establish_connection();

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
            return Err(Box::new(validation_error));
        }
    }

    let results = query
        .load(connection)
        .expect("Error loading post records");

    let mut post_results = Vec::new();
    let mut cursor: Option<String> = None;

    // https://docs.rs/chrono/0.4.26/chrono/format/strftime/index.html
    if let Some(last_post) = results.last() {
        if let Ok(parsed_time) = NaiveDateTime::parse_from_str(&last_post.indexed_at, "%+") {
            cursor = Some(format!("{}::{}",parsed_time.timestamp_millis(),last_post.cid));
        }
    }

    results
        .into_iter()
        .map(|result| {
            let post_result = PostResult {
                post: result.uri
            };
            post_results.push(post_result);
        })
        .for_each(drop);

    let new_response = AlgoResponse {
        cursor: cursor,
        feed: post_results,
    };
    Ok(new_response)
}

pub fn is_included(
    did_: String,
    list_: String
) -> Result<bool, Box<dyn std::error::Error>> {
    use crate::schema::membership::dsl::*;

    let connection = &mut establish_connection();
    let result = membership
        .filter(did.eq(did_))
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

pub async fn queue_creation(
    body: Vec<CreateRequest>
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::schema::post::dsl::*;

    let connection = &mut establish_connection();

    let mut new_posts = Vec::new();
    let mut hellthread_roots = HashSet::new();
    hellthread_roots.insert("bafyreigxvsmbhdenvzaklcfnovbsjc542cu5pjmpqyyc64mdtqwsyimlvi".to_string());

    lazy_static! {
        static ref RE: Regex = Regex::new(r"/#[^\s#\.\;]*/gmi").unwrap();
    }

    body
        .into_iter()
        .map(|req_post| {
            let system_time = SystemTime::now();
            let dt: DateTime<Utc> = system_time.into();
            let mut is_hellthread = false;
            let is_blacksky_author = is_included(req_post.author,"blacksky".into()).unwrap_or(false);
            let hashtags = RE.captures_iter(&req_post.record.text).collect::<Vec<_>>();
            
            let mut new_post = Post {
                uri: req_post.uri,
                cid: req_post.cid,
                reply_parent: None,
                reply_root: None,
                indexed_at: format!("{}", dt.format("%+")),
            };
            if let Some(reply) = req_post.record.reply {
                new_post.reply_parent = Some(reply.parent.uri);
                new_post.reply_root = Some(reply.root.uri);

                is_hellthread = hellthread_roots.contains(&reply.root.cid);
            }
            let uri_ = &new_post.uri;
            println!("uri: {uri_:?} | Blacksky: {is_blacksky_author:?} | Hellthread: {is_hellthread:?} | Hashtags: {hashtags:?}");

            let new_post = (
                uri.eq(new_post.uri),
                cid.eq(new_post.cid),
                replyParent.eq(new_post.reply_parent),
                replyRoot.eq(new_post.reply_root),
                indexedAt.eq(new_post.indexed_at)
            );
            new_posts.push(new_post);
        })
        .for_each(drop);

    diesel::insert_into(post)
        .values(&new_posts)
        .on_conflict(uri)
        .do_nothing()
        .execute(connection)?;
    Ok(())
}

pub async fn queue_deletion(
    body: Vec<DeleteRequest>,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::schema::post::dsl::*;

    let connection = &mut establish_connection();

    let mut delete_posts = Vec::new();
    body
        .into_iter()
        .map(|req_post| {
            delete_posts.push(req_post.uri);
        })
        .for_each(drop);

    diesel::delete(
            post.filter(uri.eq_any(delete_posts))
        )
        .execute(connection)?;
    Ok(())
}