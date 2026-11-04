use rocket::data::{Data, FromData, Outcome, ToByteUnit};
use rocket::form::{Form, FromForm};
use rocket::http::Status;
use rocket::request::Request;
use serde::de::DeserializeOwned;
use std::ops::Deref;

/// Request body for the OAuth protocol endpoints, accepted either as
/// `application/x-www-form-urlencoded` or `application/json`; clients
/// built on other atproto SDKs send JSON.
pub struct OAuthBody<T>(pub T);

impl<T> Deref for OAuthBody<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

#[rocket::async_trait]
impl<'r, T> FromData<'r> for OAuthBody<T>
where
    T: FromForm<'r> + DeserializeOwned + Send,
{
    type Error = String;

    async fn from_data(req: &'r Request<'_>, data: Data<'r>) -> Outcome<'r, Self> {
        let is_json = req.content_type().is_some_and(|ct| ct.is_json());
        if !is_json {
            return match Form::<T>::from_data(req, data).await {
                Outcome::Success(form) => Outcome::Success(OAuthBody(form.into_inner())),
                Outcome::Error((status, errors)) => Outcome::Error((status, errors.to_string())),
                Outcome::Forward(forward) => Outcome::Forward(forward),
            };
        }
        let limit = req.limits().get("json").unwrap_or_else(|| 1.mebibytes());
        let body = match data.open(limit).into_string().await {
            Ok(body) if body.is_complete() => body.into_inner(),
            Ok(_) => {
                return Outcome::Error((
                    Status::PayloadTooLarge,
                    "request body exceeds the limit".to_string(),
                ))
            }
            Err(error) => return Outcome::Error((Status::BadRequest, error.to_string())),
        };
        match serde_json::from_str::<T>(&body) {
            Ok(value) => Outcome::Success(OAuthBody(value)),
            Err(error) => Outcome::Error((Status::BadRequest, error.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OAuthBody;
    use rocket::http::{ContentType, Status};
    use rocket::local::blocking::Client;
    use rocket::{routes, FromForm};
    use serde::Deserialize;

    #[derive(FromForm, Deserialize)]
    struct Params {
        client_id: Option<String>,
        scope: Option<String>,
    }

    #[rocket::post("/echo", data = "<body>")]
    fn echo(body: OAuthBody<Params>) -> String {
        format!(
            "{}|{}",
            body.client_id.clone().unwrap_or_default(),
            body.scope.clone().unwrap_or_default()
        )
    }

    fn client() -> Client {
        Client::tracked(rocket::build().mount("/", routes![echo])).expect("rocket")
    }

    #[test]
    fn accepts_form_encoded_bodies() {
        let client = client();
        let response = client
            .post("/echo")
            .header(ContentType::Form)
            .body("client_id=https%3A%2F%2Fapp.example%2Fmeta.json&scope=atproto%20blob%3A*%2F*")
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        assert_eq!(
            response.into_string().unwrap(),
            "https://app.example/meta.json|atproto blob:*/*"
        );
    }

    #[test]
    fn accepts_json_bodies() {
        let client = client();
        let response = client
            .post("/echo")
            .header(ContentType::JSON)
            .body(r#"{"client_id":"https://app.example/meta.json","scope":"atproto blob:*/*","state":"x"}"#)
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        assert_eq!(
            response.into_string().unwrap(),
            "https://app.example/meta.json|atproto blob:*/*"
        );
    }

    #[test]
    fn rejects_malformed_json() {
        let client = client();
        let response = client
            .post("/echo")
            .header(ContentType::JSON)
            .body("{not json")
            .dispatch();
        assert_eq!(response.status(), Status::BadRequest);
    }

    #[test]
    fn missing_fields_are_absent_in_both_encodings() {
        let client = client();
        for (content_type, body) in [(ContentType::JSON, "{}"), (ContentType::Form, "")] {
            let response = client
                .post("/echo")
                .header(content_type)
                .body(body)
                .dispatch();
            assert_eq!(response.status(), Status::Ok);
            assert_eq!(response.into_string().unwrap(), "|");
        }
    }
}
