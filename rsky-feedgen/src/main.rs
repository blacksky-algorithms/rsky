#[macro_use]
extern crate rocket;
use dotenvy::dotenv;
use lazy_static::lazy_static;
use rocket::fairing::{Fairing, Info, Kind};
use rocket::figment::{
    util::map,
    value::{Map, Value},
};
use rocket::http::Header;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::{Request, Response};
use rsky_feedgen::models::JwtParts;
use rsky_feedgen::{ReadReplicaConn, WriteDbConn};
use std::collections::HashSet;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct CORS;

use rocket::request::{FromRequest, Outcome};

#[allow(dead_code)]
struct ApiKey<'r>(&'r str);

#[derive(Debug)]
struct AccessToken(String);

#[derive(Debug)]
enum ApiKeyError {
    Missing,
    Invalid,
}

#[derive(Debug)]
enum AccessTokenError {
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
                    match rsky_feedgen::auth::verify_jwt(&jwtstr, &service_did) {
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

const BLACKSKY: &str = "at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.generator/blacksky";
const BLACKSKY_OG: &str =
    "at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.generator/blacksky-op";
const BLACKSKY_TREND: &str =
    "at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.generator/blacksky-trend";
const BLACKSKY_FR: &str =
    "at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.generator/blacksky-fr";
const BLACKSKY_PT: &str =
    "at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.generator/blacksky-pt";
const BLACKSKY_NSFW: &str =
    "at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.generator/blacksky-nsfw";
const BLACKSKY_EDU: &str =
    "at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.generator/blacksky-edu";

lazy_static! {
    static ref BANNED_FROM_TV: HashSet<&'static str> = {
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
        s
    };
}

fn get_banned_response() -> rsky_feedgen::models::AlgoResponse {
    let banned_notice_uri = env::var("BANNED_NOTICE_POST_URI").unwrap_or("".into());
    let banned_notice_cid = env::var("BANNED_NOTICE_POST_CID").unwrap_or("".into());
    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    let timestamp = since_the_epoch.as_millis();
    let cursor = Some(format!("{}::{}", timestamp, banned_notice_cid));
    let banned_notice = rsky_feedgen::models::PostResult {
        post: banned_notice_uri,
    };
    let banned_response = rsky_feedgen::models::AlgoResponse {
        cursor: cursor,
        feed: vec![banned_notice],
    };
    banned_response
}

#[get(
    "/xrpc/app.bsky.feed.getFeedSkeleton?<feed>&<limit>&<cursor>",
    format = "json"
)]
async fn index(
    feed: Option<&str>,
    limit: Option<i64>,
    cursor: Option<&str>,
    connection: ReadReplicaConn,
    _token: Result<AccessToken, AccessTokenError>,
) -> Result<
    Json<rsky_feedgen::models::AlgoResponse>,
    status::Custom<Json<rsky_feedgen::models::InternalErrorMessageResponse>>,
> {
    let mut is_banned = false;
    let feed = feed.unwrap_or("".into());
    if let Ok(jwt) = _token {
        match serde_json::from_str::<JwtParts>(&jwt.0) {
            Ok(jwt_obj) => {
                let did = jwt_obj.iss;
                match rsky_feedgen::apis::add_visitor(did.clone(), jwt_obj.aud, feed.to_string()) {
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
        match rsky_feedgen::apis::add_visitor("anonymous".into(), service_did, feed.to_string()) {
            Ok(_) => (),
            Err(_) => eprintln!("Failed to write anonymous visitor."),
        }
    }
    match feed {
        _blacksky if _blacksky == BLACKSKY && !is_banned => {
            match rsky_feedgen::apis::get_all_posts(None, limit, cursor, true, connection).await {
                Ok(response) => Ok(Json(response)),
                Err(error) => {
                    eprintln!("Internal Error: {error}");
                    let internal_error = rsky_feedgen::models::InternalErrorMessageResponse {
                        code: Some(rsky_feedgen::models::InternalErrorCode::InternalError),
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
            match rsky_feedgen::apis::get_all_posts(None, limit, cursor, false, connection).await {
                Ok(response) => Ok(Json(response)),
                Err(error) => {
                    eprintln!("Internal Error: {error}");
                    let internal_error = rsky_feedgen::models::InternalErrorMessageResponse {
                        code: Some(rsky_feedgen::models::InternalErrorCode::InternalError),
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
            match rsky_feedgen::apis::get_blacksky_trending(limit, cursor, connection).await {
                Ok(response) => Ok(Json(response)),
                Err(error) => {
                    eprintln!("Internal Error: {error}");
                    let internal_error = rsky_feedgen::models::InternalErrorMessageResponse {
                        code: Some(rsky_feedgen::models::InternalErrorCode::InternalError),
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
            match rsky_feedgen::apis::get_posts_by_membership(
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
                    let internal_error = rsky_feedgen::models::InternalErrorMessageResponse {
                        code: Some(rsky_feedgen::models::InternalErrorCode::InternalError),
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
            match rsky_feedgen::apis::get_all_posts(
                Some("fr".into()),
                limit,
                cursor,
                true,
                connection,
            )
            .await
            {
                Ok(response) => Ok(Json(response)),
                Err(error) => {
                    eprintln!("Internal Error: {error}");
                    let internal_error = rsky_feedgen::models::InternalErrorMessageResponse {
                        code: Some(rsky_feedgen::models::InternalErrorCode::InternalError),
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
            match rsky_feedgen::apis::get_all_posts(
                Some("pt".into()),
                limit,
                cursor,
                true,
                connection,
            )
            .await
            {
                Ok(response) => Ok(Json(response)),
                Err(error) => {
                    eprintln!("Internal Error: {error}");
                    let internal_error = rsky_feedgen::models::InternalErrorMessageResponse {
                        code: Some(rsky_feedgen::models::InternalErrorCode::InternalError),
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
            match rsky_feedgen::apis::get_blacksky_nsfw(limit, cursor, connection).await {
                Ok(response) => Ok(Json(response)),
                Err(error) => {
                    eprintln!("Internal Error: {error}");
                    let internal_error = rsky_feedgen::models::InternalErrorMessageResponse {
                        code: Some(rsky_feedgen::models::InternalErrorCode::InternalError),
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
            let internal_error = rsky_feedgen::models::InternalErrorMessageResponse {
                code: Some(rsky_feedgen::models::InternalErrorCode::InternalError),
                message: Some("Not Found".to_string()),
            };
            Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ))
        }
    }
}

#[put("/cursor?<service>&<sequence>")]
async fn update_cursor(
    service: &str,
    sequence: i64,
    _key: ApiKey<'_>,
    connection: WriteDbConn,
) -> Result<(), status::Custom<Json<rsky_feedgen::models::InternalErrorMessageResponse>>> {
    match rsky_feedgen::apis::update_cursor(service.to_string(), sequence, connection).await {
        Ok(_) => Ok(()),
        Err(error) => {
            eprintln!("Internal Error: {error}");
            let internal_error = rsky_feedgen::models::InternalErrorMessageResponse {
                code: Some(rsky_feedgen::models::InternalErrorCode::InternalError),
                message: Some(error.to_string()),
            };
            Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ))
        }
    }
}

#[get("/cursor?<service>", format = "json")]
async fn get_cursor(
    service: &str,
    _key: ApiKey<'_>,
    connection: ReadReplicaConn,
) -> Result<
    Json<rsky_feedgen::models::SubState>,
    status::Custom<Json<rsky_feedgen::models::PathUnknownErrorMessageResponse>>,
> {
    match rsky_feedgen::apis::get_cursor(service.to_string(), connection).await {
        Ok(response) => Ok(Json(response)),
        Err(error) => {
            eprintln!("Internal Error: {error}");
            let path_error = rsky_feedgen::models::PathUnknownErrorMessageResponse {
                code: Some(rsky_feedgen::models::NotFoundErrorCode::NotFoundError),
                message: Some("Not Found".to_string()),
            };
            Err(status::Custom(Status::NotFound, Json(path_error)))
        }
    }
}

#[put("/queue/<lex>/create", format = "json", data = "<body>")]
async fn queue_creation(
    lex: &str,
    body: Json<Vec<rsky_feedgen::models::CreateRequest>>,
    _key: ApiKey<'_>,
    connection: WriteDbConn,
) -> Result<(), status::Custom<Json<rsky_feedgen::models::InternalErrorMessageResponse>>> {
    match rsky_feedgen::apis::queue_creation(lex.to_string(), body.into_inner(), connection).await {
        Ok(_) => Ok(()),
        Err(error) => {
            eprintln!("Internal Error: {error}");
            let internal_error = rsky_feedgen::models::InternalErrorMessageResponse {
                code: Some(rsky_feedgen::models::InternalErrorCode::InternalError),
                message: Some(error.to_string()),
            };
            Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ))
        }
    }
}

#[put("/queue/<lex>/delete", format = "json", data = "<body>")]
async fn queue_deletion(
    lex: &str,
    body: Json<Vec<rsky_feedgen::models::DeleteRequest>>,
    _key: ApiKey<'_>,
    connection: WriteDbConn,
) -> Result<(), status::Custom<Json<rsky_feedgen::models::InternalErrorMessageResponse>>> {
    match rsky_feedgen::apis::queue_deletion(lex.to_string(), body.into_inner(), connection).await {
        Ok(_) => Ok(()),
        Err(error) => {
            eprintln!("Internal Error: {error}");
            let internal_error = rsky_feedgen::models::InternalErrorMessageResponse {
                code: Some(rsky_feedgen::models::InternalErrorCode::InternalError),
                message: Some(error.to_string()),
            };
            Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ))
        }
    }
}

#[get("/.well-known/did.json", format = "json")]
async fn well_known() -> Result<
    Json<rsky_feedgen::models::WellKnown>,
    status::Custom<Json<rsky_feedgen::models::PathUnknownErrorMessageResponse>>,
> {
    match env::var("FEEDGEN_SERVICE_DID") {
        Ok(service_did) => {
            let hostname = env::var("FEEDGEN_HOSTNAME").unwrap_or("".into());
            if !service_did.ends_with(hostname.as_str()) {
                let path_error = rsky_feedgen::models::PathUnknownErrorMessageResponse {
                    code: Some(rsky_feedgen::models::NotFoundErrorCode::NotFoundError),
                    message: Some("Not Found".to_string()),
                };
                Err(status::Custom(Status::NotFound, Json(path_error)))
            } else {
                let known_service = rsky_feedgen::models::KnownService {
                    id: "#bsky_fg".to_owned(),
                    r#type: "BskyFeedGenerator".to_owned(),
                    service_endpoint: format!("https://{}", hostname),
                };
                let result = rsky_feedgen::models::WellKnown {
                    context: vec!["https://www.w3.org/ns/did/v1".into()],
                    id: service_did,
                    service: vec![known_service],
                };
                Ok(Json(result))
            }
        }
        Err(_) => {
            let path_error = rsky_feedgen::models::PathUnknownErrorMessageResponse {
                code: Some(rsky_feedgen::models::NotFoundErrorCode::NotFoundError),
                message: Some("Not Found".to_string()),
            };
            Err(status::Custom(Status::NotFound, Json(path_error)))
        }
    }
}

#[catch(404)]
async fn not_found() -> Json<rsky_feedgen::models::PathUnknownErrorMessageResponse> {
    let path_error = rsky_feedgen::models::PathUnknownErrorMessageResponse {
        code: Some(rsky_feedgen::models::NotFoundErrorCode::UndefinedEndpoint),
        message: Some("Not Found".to_string()),
    };
    Json(path_error)
}

#[catch(422)]
async fn unprocessable_entity() -> Json<rsky_feedgen::models::ValidationErrorMessageResponse> {
    let validation_error = rsky_feedgen::models::ValidationErrorMessageResponse {
        code: Some(rsky_feedgen::models::ErrorCode::ValidationError),
        message: Some(
            "The request was well-formed but was unable to be followed due to semantic errors."
                .to_string(),
        ),
    };
    Json(validation_error)
}

#[catch(400)]
async fn bad_request() -> Json<rsky_feedgen::models::ValidationErrorMessageResponse> {
    let validation_error = rsky_feedgen::models::ValidationErrorMessageResponse {
        code: Some(rsky_feedgen::models::ErrorCode::ValidationError),
        message: Some("The request was improperly formed.".to_string()),
    };
    Json(validation_error)
}

#[catch(401)]
async fn unauthorized() -> Json<rsky_feedgen::models::InternalErrorMessageResponse> {
    let internal_error = rsky_feedgen::models::InternalErrorMessageResponse {
        code: Some(rsky_feedgen::models::InternalErrorCode::Unavailable),
        message: Some("Request could not be processed.".to_string()),
    };
    Json(internal_error)
}

#[catch(default)]
async fn default_catcher() -> Json<rsky_feedgen::models::InternalErrorMessageResponse> {
    let internal_error = rsky_feedgen::models::InternalErrorMessageResponse {
        code: Some(rsky_feedgen::models::InternalErrorCode::InternalError),
        message: Some("Internal error.".to_string()),
    };
    Json(internal_error)
}

/// Catches all OPTION requests in order to get the CORS related Fairing triggered.
#[options("/<_..>")]
async fn all_options() {
    /* Intentionally left empty */
}

#[rocket::async_trait]
impl Fairing for CORS {
    fn info(&self) -> Info {
        Info {
            name: "Add CORS headers to responses",
            kind: Kind::Response,
        }
    }

    async fn on_response<'r>(&self, _request: &'r Request<'_>, response: &mut Response<'r>) {
        response.set_header(Header::new("Access-Control-Allow-Origin", "*"));
        response.set_header(Header::new(
            "Access-Control-Allow-Methods",
            "POST, GET, PATCH, OPTIONS, DELETE",
        ));
        response.set_header(Header::new("Access-Control-Allow-Headers", "*"));
        response.set_header(Header::new("Access-Control-Allow-Credentials", "true"));
    }
}

#[launch]
fn rocket() -> _ {
    dotenv().ok();

    let write_database_url = env::var("DATABASE_URL").unwrap_or("".into());
    let read_database_url = env::var("READ_REPLICA_URL").unwrap_or("".into());

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
        .mount(
            "/",
            routes![
                index,
                queue_creation,
                queue_deletion,
                well_known,
                get_cursor,
                update_cursor,
                all_options
            ],
        )
        .register(
            "/",
            catchers![
                default_catcher,
                unprocessable_entity,
                bad_request,
                not_found,
                unauthorized
            ],
        )
        .attach(CORS)
        .attach(WriteDbConn::fairing())
        .attach(ReadReplicaConn::fairing())
}
