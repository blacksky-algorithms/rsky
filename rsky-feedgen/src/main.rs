#[macro_use] extern crate rocket;
use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::Header;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::{Request, Response};
use std::env;

pub struct CORS;

use rocket::request::{FromRequest, Outcome};

struct ApiKey<'r>(&'r str);

#[derive(Debug)]
enum ApiKeyError {
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
            return Outcome::Failure((Status::BadRequest, ApiKeyError::Invalid));
        }

        match req.headers().get_one("X-RSKY-KEY") {
            None => Outcome::Failure((Status::Unauthorized, ApiKeyError::Missing)),
            Some(key) if key == token => Outcome::Success(ApiKey(key)),
            Some(_) => Outcome::Failure((Status::Unauthorized, ApiKeyError::Invalid)),
        }
    }
}

const BLACKSKY: &str = "at://did:plc:w4xbfzo7kqfes5zb7r6qv3rw/app.bsky.feed.generator/blacksky";

#[get("/xrpc/app.bsky.feed.getFeedSkeleton?<feed>&<limit>&<cursor>", format = "json")]
async fn index (
    feed: Option<String>,
    limit: Option<i64>,
    cursor: Option<String>,
) -> Result<
    Json<rsky_feedgen::models::AlgoResponse>,
    status::Custom<Json<rsky_feedgen::models::InternalErrorMessageResponse>>,
> {
    let _blacksky: String = String::from(BLACKSKY);
    match feed {
        Some(_blacksky) => {
            match rsky_feedgen::apis::get_blacksky_posts(limit, cursor).await {
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

#[put("/queue/create", format = "json", data = "<body>")]
async fn queue_creation(
    body: Json<Vec<rsky_feedgen::models::CreateRequest>>,
    _key: ApiKey<'_>,
) -> Result<(), status::Custom<Json<rsky_feedgen::models::InternalErrorMessageResponse>>> {
    match rsky_feedgen::apis::queue_creation(body.into_inner()).await {
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

#[put("/queue/delete", format = "json", data = "<body>")]
async fn queue_deletion(
    body: Json<Vec<rsky_feedgen::models::DeleteRequest>>,
    _key: ApiKey<'_>,
) -> Result<(), status::Custom<Json<rsky_feedgen::models::InternalErrorMessageResponse>>> {
    match rsky_feedgen::apis::queue_deletion(body.into_inner()).await {
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


#[catch(404)]
fn not_found() -> Json<rsky_feedgen::models::PathUnknownErrorMessageResponse> {
    let path_error = rsky_feedgen::models::PathUnknownErrorMessageResponse {
        code: Some(rsky_feedgen::models::NotFoundErrorCode::UndefinedEndpoint),
        message: Some("Not Found".to_string()),
    };
    Json(path_error)
}

#[catch(422)]
fn unprocessable_entity() -> Json<rsky_feedgen::models::ValidationErrorMessageResponse> {
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
fn bad_request() -> Json<rsky_feedgen::models::ValidationErrorMessageResponse> {
    let validation_error = rsky_feedgen::models::ValidationErrorMessageResponse {
        code: Some(rsky_feedgen::models::ErrorCode::ValidationError),
        message: Some("The request was improperly formed.".to_string()),
    };
    Json(validation_error)
}

#[catch(401)]
fn unauthorized() -> Json<rsky_feedgen::models::InternalErrorMessageResponse> {
    let internal_error = rsky_feedgen::models::InternalErrorMessageResponse {
        code: Some(rsky_feedgen::models::InternalErrorCode::Unavailable),
        message: Some("Request could not be processed.".to_string()),
    };
    Json(internal_error)
}

#[catch(default)]
fn default_catcher() -> Json<rsky_feedgen::models::InternalErrorMessageResponse> {
    let internal_error = rsky_feedgen::models::InternalErrorMessageResponse {
        code: Some(rsky_feedgen::models::InternalErrorCode::InternalError),
        message: Some("Internal error.".to_string()),
    };
    Json(internal_error)
}

/// Catches all OPTION requests in order to get the CORS related Fairing triggered.
#[options("/<_..>")]
fn all_options() {
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
    rocket::build()
        .mount(
            "/",
            routes![
                index,
                queue_creation,
                queue_deletion,
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
}