use crate::models::JwtParts;
use crate::{FeedGenConfig, ReadReplicaConn, WriteDbConn};
use once_cell::sync::Lazy;
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome};
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::{Request, State};
use std::collections::HashSet;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

#[allow(dead_code)]
pub struct ApiKey<'r>(&'r str);

#[derive(Debug)]
pub struct AccessToken(String);

#[derive(Debug)]
pub enum ApiKeyError {
    Missing,
    Invalid,
}

#[derive(Debug)]
pub enum AccessTokenError {
    Missing,
    Invalid,
}

#[allow(unused_assignments)]
#[rocket::async_trait]
impl<'r> FromRequest<'r> for ApiKey<'r> {
    type Error = ApiKeyError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let mut token: String = "".to_owned();
        if let Ok(token_result) = env::var("RSKY_API_KEY") {
            token = token_result;
        } else {
            return Outcome::Error((Status::BadRequest, ApiKeyError::Invalid));
        }

        match req.headers().get_one("X-RSKY-KEY") {
            None => Outcome::Error((Status::Unauthorized, ApiKeyError::Missing)),
            Some(key) if key == token => Outcome::Success(ApiKey(key)),
            Some(_) => Outcome::Error((Status::Unauthorized, ApiKeyError::Invalid)),
        }
    }
}

#[allow(unused_assignments)]
#[rocket::async_trait]
impl<'r> FromRequest<'r> for AccessToken {
    type Error = AccessTokenError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match req.headers().get_one("Authorization") {
            None => Outcome::Error((Status::Unauthorized, AccessTokenError::Missing)),
            Some(token) if !token.starts_with("Bearer ") => {
                Outcome::Error((Status::Unauthorized, AccessTokenError::Invalid))
            }
            Some(token) => {
                println!("Visited by {token:?}");
                let service_did = env::var("FEEDGEN_SERVICE_DID").unwrap_or("".into());
                let jwt = token.split(" ").map(String::from).collect::<Vec<_>>();
                if let Some(jwtstr) = jwt.last() {
                    match crate::auth::verify_jwt(&jwtstr, &service_did) {
                        Ok(jwt_object) => Outcome::Success(AccessToken(jwt_object)),
                        Err(error) => {
                            eprintln!("Error decoding jwt. {error:?}");
                            Outcome::Error((Status::Unauthorized, AccessTokenError::Invalid))
                        }
                    }
                } else {
                    Outcome::Error((Status::Unauthorized, AccessTokenError::Invalid))
                }
            }
        }
    }
}

static BANNED_FROM_TV: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    let mut s = HashSet::new();
    s.insert("did:plc:bqlobp4ngysw3a52gdfnxbne"); // HS
    s.insert("did:plc:gdvllvjfpamphmnzn2yl4q2w"); // HS/IM
    s.insert("did:plc:aiycw7ixy2quoweotm3bloml"); // HS
    s.insert("did:plc:4nkcdcu6mltxmootrsvg43q7"); // HS
    s.insert("did:plc:xcltkjpurlj2m7zzs6sh74db"); // Hate Campaign
    s.insert("did:plc:xfuejssf6ox7rqafjsm3azqk"); // Hate Campaign
    s.insert("did:plc:qeoub4zavdlnwoufa4ketosn"); // Hate Campaign
    s.insert("did:plc:tquk7ybcb2tvxv6acgqe4q2e"); // HS
    s.insert("did:plc:gyk5exv532seawdowdfwsn2m"); // Anti-Black
    s.insert("did:plc:smmuzxhbumgqptziqeujv2su"); // Anti-Black
    s.insert("did:plc:vpkthocm76u4rcvw4k2e2l5c"); // Hate Campaign (soyjak)
    s.insert("did:plc:vyxwktjvl4nhybxuirza3l3j"); // Hate Campaign (soyjak)
    s.insert("did:plc:nynnin6sxmfwgbypwqajyfnk"); // Hate Campaign (soyjak)
    s.insert("did:plc:dd3seyd2vwpj5a6e7hgactwx"); // Hate Campaign (soyjak)
    s.insert("did:plc:puovasjyg24e3rxfmze7ag3z"); // Hate Campaign (soyjak)
    s.insert("did:plc:waiym5islzntck2whytzitbo"); // Hate Campaign (soyjak)
    s.insert("did:plc:lswpfsk34m45vxh3gs3tz6p4"); // Hate Campaign (soyjak)
    s.insert("did:plc:a2mosbbxq4i3avulobujxkko"); // Hate Campaign (soyjak)
    s.insert("did:plc:rrygeg5e5sze75absmolbibm"); // Hate Campaign (soyjak)
    s.insert("did:plc:sysv2njdvrhv2i7j2jtmudnn"); // Hate Campaign (soyjak)
    s.insert("did:plc:lqkz7god5gbb53isjvxk7g5n"); // Hate Campaign (soyjak)
    s.insert("did:plc:cco7fsmvzh4mwouwnixgrice"); // HS
    s.insert("did:plc:hsavkkfqhby3ystqwh5jjb4l"); // HS
    s.insert("did:plc:4ncualnqj4asxtsnwvkgrinl"); // Hate Campaign
    s.insert("did:plc:zlrad5ksg7eoqg667poajj6w"); // Troll
    s.insert("did:plc:7ywf4tokx3r3h5s6y4pychyz"); // Troll
    s.insert("did:plc:6xjvvvwtj3ukiqctgxapp5ms"); // Troll
    s.insert("did:plc:s455uh5xrwiicgd4wauoyq25"); // Troll
    s.insert("did:plc:3fbca6a3yrydvs43rlnmjseu"); // Troll
    s.insert("did:plc:wdbjnc3safjqexedt47wew5z"); // Troll
    s.insert("did:plc:ndthbydalxvn6im3xbepv2o3"); // Troll
    s.insert("did:plc:xjvhjq7yba3ru66tg5d4fjmx"); // Troll
    s.insert("did:plc:chsy6zkcy2dz3fh7wm73jlhg"); // Troll
    s.insert("did:plc:jddsovvxzwgvoc2sftrim3o4"); // Troll
    s.insert("did:plc:p3j35p2csk2xjrjmtumak6ud"); // Troll
    s
});

pub(crate) const BLACKSKY: &str =
    "at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.generator/blacksky";
pub(crate) const BLACKSKY_OG: &str =
    "at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.generator/blacksky-op";
pub(crate) const BLACKSKY_TREND: &str =
    "at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.generator/blacksky-trend";
pub(crate) const BLACKSKY_FR: &str =
    "at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.generator/blacksky-fr";
pub(crate) const BLACKSKY_PT: &str =
    "at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.generator/blacksky-pt";
pub(crate) const BLACKSKY_NSFW: &str =
    "at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.generator/blacksky-nsfw";
pub(crate) const BLACKSKY_EDU: &str =
    "at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.generator/blacksky-edu";

fn get_banned_response() -> crate::models::AlgoResponse {
    let banned_notice_uri = env::var("BANNED_NOTICE_POST_URI").unwrap_or("".into());
    let banned_notice_cid = env::var("BANNED_NOTICE_POST_CID").unwrap_or("".into());
    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    let timestamp = since_the_epoch.as_millis();
    let cursor = Some(format!("{}::{}", timestamp, banned_notice_cid));
    let banned_notice = crate::models::PostResult {
        post: banned_notice_uri,
    };
    let banned_response = crate::models::AlgoResponse {
        cursor,
        feed: vec![banned_notice],
    };
    banned_response
}

#[rocket::get(
    "/xrpc/app.bsky.feed.getFeedSkeleton?<feed>&<limit>&<cursor>",
    format = "json"
)]
pub async fn index(
    feed: Option<&str>,
    limit: Option<i64>,
    cursor: Option<&str>,
    connection: ReadReplicaConn,
    config: &State<FeedGenConfig>,
    _token: Result<AccessToken, AccessTokenError>,
) -> Result<
    Json<crate::models::AlgoResponse>,
    status::Custom<Json<crate::models::InternalErrorMessageResponse>>,
> {
    let mut is_banned = false;
    let feed = feed.unwrap_or("".into());
    if let Ok(jwt) = _token {
        match serde_json::from_str::<JwtParts>(&jwt.0) {
            Ok(jwt_obj) => {
                let did = jwt_obj.iss;
                match crate::apis::add_visitor(did.clone(), jwt_obj.aud, feed.to_string()) {
                    Ok(_) => {
                        if BANNED_FROM_TV.contains(&did.as_str()) {
                            is_banned = true;
                        }
                        ()
                    }
                    Err(_) => eprintln!("Failed to write visitor."),
                }
            }
            Err(_) => eprintln!("Failed to parse jwt string."),
        }
    } else {
        let service_did = env::var("FEEDGEN_SERVICE_DID").unwrap_or("".into());
        match crate::apis::add_visitor("anonymous".into(), service_did, feed.to_string()) {
            Ok(_) => (),
            Err(_) => eprintln!("Failed to write anonymous visitor."),
        }
    }
    match feed {
        _blacksky if _blacksky == BLACKSKY && !is_banned => {
            match crate::apis::get_all_posts(None, limit, cursor, true, connection, config).await {
                Ok(response) => Ok(Json(response)),
                Err(error) => {
                    eprintln!("Internal Error: {error}");
                    let internal_error = crate::models::InternalErrorMessageResponse {
                        code: Some(crate::models::InternalErrorCode::InternalError),
                        message: Some(error.to_string()),
                    };
                    Err(status::Custom(
                        Status::InternalServerError,
                        Json(internal_error),
                    ))
                }
            }
        }
        _blacksky_og if _blacksky_og == BLACKSKY_OG && !is_banned => {
            match crate::apis::get_all_posts(None, limit, cursor, false, connection, config).await {
                Ok(response) => Ok(Json(response)),
                Err(error) => {
                    eprintln!("Internal Error: {error}");
                    let internal_error = crate::models::InternalErrorMessageResponse {
                        code: Some(crate::models::InternalErrorCode::InternalError),
                        message: Some(error.to_string()),
                    };
                    Err(status::Custom(
                        Status::InternalServerError,
                        Json(internal_error),
                    ))
                }
            }
        }
        _blacksky_trend if _blacksky_trend == BLACKSKY_TREND && !is_banned => {
            match crate::apis::get_blacksky_trending(limit, cursor, connection).await {
                Ok(response) => Ok(Json(response)),
                Err(error) => {
                    eprintln!("Internal Error: {error}");
                    let internal_error = crate::models::InternalErrorMessageResponse {
                        code: Some(crate::models::InternalErrorCode::InternalError),
                        message: Some(error.to_string()),
                    };
                    Err(status::Custom(
                        Status::InternalServerError,
                        Json(internal_error),
                    ))
                }
            }
        }
        _blacksky_edu if _blacksky_edu == BLACKSKY_EDU && !is_banned => {
            match crate::apis::get_posts_by_membership(
                None,
                limit,
                cursor,
                true,
                "blacksky-edu".into(),
                connection,
            )
            .await
            {
                Ok(response) => Ok(Json(response)),
                Err(error) => {
                    eprintln!("Internal Error: {error}");
                    let internal_error = crate::models::InternalErrorMessageResponse {
                        code: Some(crate::models::InternalErrorCode::InternalError),
                        message: Some(error.to_string()),
                    };
                    Err(status::Custom(
                        Status::InternalServerError,
                        Json(internal_error),
                    ))
                }
            }
        }
        _blacksky_fr if _blacksky_fr == BLACKSKY_FR && !is_banned => {
            match crate::apis::get_all_posts(
                Some("fr".into()),
                limit,
                cursor,
                true,
                connection,
                config,
            )
            .await
            {
                Ok(response) => Ok(Json(response)),
                Err(error) => {
                    eprintln!("Internal Error: {error}");
                    let internal_error = crate::models::InternalErrorMessageResponse {
                        code: Some(crate::models::InternalErrorCode::InternalError),
                        message: Some(error.to_string()),
                    };
                    Err(status::Custom(
                        Status::InternalServerError,
                        Json(internal_error),
                    ))
                }
            }
        }
        _blacksky_pt if _blacksky_pt == BLACKSKY_PT && !is_banned => {
            match crate::apis::get_all_posts(
                Some("pt".into()),
                limit,
                cursor,
                true,
                connection,
                config,
            )
            .await
            {
                Ok(response) => Ok(Json(response)),
                Err(error) => {
                    eprintln!("Internal Error: {error}");
                    let internal_error = crate::models::InternalErrorMessageResponse {
                        code: Some(crate::models::InternalErrorCode::InternalError),
                        message: Some(error.to_string()),
                    };
                    Err(status::Custom(
                        Status::InternalServerError,
                        Json(internal_error),
                    ))
                }
            }
        }
        _blacksky_nsfw if _blacksky_nsfw == BLACKSKY_NSFW && !is_banned => {
            match crate::apis::get_blacksky_nsfw(limit, cursor, connection).await {
                Ok(response) => Ok(Json(response)),
                Err(error) => {
                    eprintln!("Internal Error: {error}");
                    let internal_error = crate::models::InternalErrorMessageResponse {
                        code: Some(crate::models::InternalErrorCode::InternalError),
                        message: Some(error.to_string()),
                    };
                    Err(status::Custom(
                        Status::InternalServerError,
                        Json(internal_error),
                    ))
                }
            }
        }
        _blacksky if _blacksky == BLACKSKY && is_banned => {
            let banned_response = get_banned_response();
            Ok(Json(banned_response))
        }
        _blacksky_og if _blacksky_og == BLACKSKY_OG && is_banned => {
            let banned_response = get_banned_response();
            Ok(Json(banned_response))
        }
        _blacksky_trend if _blacksky_trend == BLACKSKY_TREND && is_banned => {
            let banned_response = get_banned_response();
            Ok(Json(banned_response))
        }
        _blacksky_fr if _blacksky_fr == BLACKSKY_FR && is_banned => {
            let banned_response = get_banned_response();
            Ok(Json(banned_response))
        }
        _blacksky_pt if _blacksky_pt == BLACKSKY_PT && is_banned => {
            let banned_response = get_banned_response();
            Ok(Json(banned_response))
        }
        _blacksky_nsfw if _blacksky_nsfw == BLACKSKY_NSFW && is_banned => {
            let banned_response = get_banned_response();
            Ok(Json(banned_response))
        }
        _blacksky_edu if _blacksky_edu == BLACKSKY_EDU && is_banned => {
            let banned_response = get_banned_response();
            Ok(Json(banned_response))
        }
        _ => {
            let internal_error = crate::models::InternalErrorMessageResponse {
                code: Some(crate::models::InternalErrorCode::InternalError),
                message: Some("Not Found".to_string()),
            };
            Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ))
        }
    }
}

#[rocket::put("/cursor?<service>&<sequence>")]
pub async fn update_cursor(
    service: &str,
    sequence: i64,
    _key: ApiKey<'_>,
    connection: WriteDbConn,
) -> Result<(), status::Custom<Json<crate::models::InternalErrorMessageResponse>>> {
    match crate::apis::update_cursor(service.to_string(), sequence, connection).await {
        Ok(_) => Ok(()),
        Err(error) => {
            eprintln!("Internal Error: {error}");
            let internal_error = crate::models::InternalErrorMessageResponse {
                code: Some(crate::models::InternalErrorCode::InternalError),
                message: Some(error.to_string()),
            };
            Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ))
        }
    }
}

#[rocket::get("/cursor?<service>", format = "json")]
pub async fn get_cursor(
    service: &str,
    _key: ApiKey<'_>,
    connection: ReadReplicaConn,
) -> Result<
    Json<crate::models::SubState>,
    status::Custom<Json<crate::models::PathUnknownErrorMessageResponse>>,
> {
    match crate::apis::get_cursor(service.to_string(), connection).await {
        Ok(response) => Ok(Json(response)),
        Err(error) => {
            eprintln!("Internal Error: {error}");
            let path_error = crate::models::PathUnknownErrorMessageResponse {
                code: Some(crate::models::NotFoundErrorCode::NotFoundError),
                message: Some("Not Found".to_string()),
            };
            Err(status::Custom(Status::NotFound, Json(path_error)))
        }
    }
}

#[rocket::put("/queue/<lex>/create", format = "json", data = "<body>")]
pub async fn queue_creation(
    lex: &str,
    body: Json<Vec<crate::models::CreateRequest>>,
    _key: ApiKey<'_>,
    connection: WriteDbConn,
) -> Result<(), status::Custom<Json<crate::models::InternalErrorMessageResponse>>> {
    match crate::apis::queue_creation(lex.to_string(), body.into_inner(), connection).await {
        Ok(_) => Ok(()),
        Err(error) => {
            eprintln!("Internal Error: {error}");
            let internal_error = crate::models::InternalErrorMessageResponse {
                code: Some(crate::models::InternalErrorCode::InternalError),
                message: Some(error.to_string()),
            };
            Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ))
        }
    }
}

#[rocket::put("/queue/<lex>/delete", format = "json", data = "<body>")]
pub async fn queue_deletion(
    lex: &str,
    body: Json<Vec<crate::models::DeleteRequest>>,
    _key: ApiKey<'_>,
    connection: WriteDbConn,
) -> Result<(), status::Custom<Json<crate::models::InternalErrorMessageResponse>>> {
    match crate::apis::queue_deletion(lex.to_string(), body.into_inner(), connection).await {
        Ok(_) => Ok(()),
        Err(error) => {
            eprintln!("Internal Error: {error}");
            let internal_error = crate::models::InternalErrorMessageResponse {
                code: Some(crate::models::InternalErrorCode::InternalError),
                message: Some(error.to_string()),
            };
            Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ))
        }
    }
}

#[rocket::get("/.well-known/did.json", format = "json")]
pub async fn well_known() -> Result<
    Json<crate::models::WellKnown>,
    status::Custom<Json<crate::models::PathUnknownErrorMessageResponse>>,
> {
    match env::var("FEEDGEN_SERVICE_DID") {
        Ok(service_did) => {
            let hostname = env::var("FEEDGEN_HOSTNAME").unwrap_or("".into());
            if !service_did.ends_with(hostname.as_str()) {
                let path_error = crate::models::PathUnknownErrorMessageResponse {
                    code: Some(crate::models::NotFoundErrorCode::NotFoundError),
                    message: Some("Not Found".to_string()),
                };
                Err(status::Custom(Status::NotFound, Json(path_error)))
            } else {
                let known_service = crate::models::KnownService {
                    id: "#bsky_fg".to_owned(),
                    r#type: "BskyFeedGenerator".to_owned(),
                    service_endpoint: format!("https://{}", hostname),
                };
                let result = crate::models::WellKnown {
                    context: vec!["https://www.w3.org/ns/did/v1".into()],
                    id: service_did,
                    service: vec![known_service],
                };
                Ok(Json(result))
            }
        }
        Err(_) => {
            let path_error = crate::models::PathUnknownErrorMessageResponse {
                code: Some(crate::models::NotFoundErrorCode::NotFoundError),
                message: Some("Not Found".to_string()),
            };
            Err(status::Custom(Status::NotFound, Json(path_error)))
        }
    }
}
