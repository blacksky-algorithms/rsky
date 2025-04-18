use crate::common::{create_account, get_admin_token};
use diesel::row::NamedRow;
use jsonwebtoken::jwk::{
    AlgorithmParameters, CommonParameters, EllipticCurve, EllipticCurveKeyParameters,
    EllipticCurveKeyType, Jwk, JwkSet, KeyAlgorithm, KeyOperations, PublicKeyUse,
};
use rocket::http::{ContentType, Header, Status};
use rocket::yansi::Paint;
use rsky_lexicon::com::atproto::server::CreateInviteCodeOutput;
use rsky_oauth::oauth_provider::oauth_provider::SignInResponse;
use rsky_oauth::oauth_types::{
    BearerMethod, OAuthAuthorizationServerMetadata, OAuthCodeChallengeMethod, OAuthGrantType,
    OAuthIssuerIdentifier, OAuthParResponse, OAuthProtectedResourceMetadata, ValidUri, WebUri,
};
use serde_json::json;
use testcontainers::runners::AsyncRunner;

mod common;

#[tokio::test]
async fn test_index() {
    let postgres = common::get_postgres().await;
    let client = common::get_client(&postgres).await;
    let response = client.get("/").dispatch().await;
    assert_eq!(response.status(), Status::Ok);
}

#[tokio::test]
async fn test_robots_txt() {
    let postgres = common::get_postgres().await;
    let client = common::get_client(&postgres).await;
    let response = client.get("/robots.txt").dispatch().await;
    let response_status = response.status();
    let response_body = response.into_string().await.unwrap();
    assert_eq!(response_status, Status::Ok);
    assert_eq!(
        response_body,
        "# Hello!\n\n# Crawling the public API is allowed\nUser-agent: *\nAllow: /"
    );
}

#[tokio::test]
async fn test_create_invite_code() {
    let postgres = common::get_postgres().await;
    let client = common::get_client(&postgres).await;
    let input = json!({
        "useCount": 1
    });

    let response = client
        .post("/xrpc/com.atproto.server.createInviteCode")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", get_admin_token()))
        .body(input.to_string())
        .dispatch()
        .await;
    let response_status = response.status();

    assert_eq!(response_status, Status::Ok);
    response
        .into_json::<CreateInviteCodeOutput>()
        .await
        .unwrap();
}

#[tokio::test]
async fn test_create_invite_code_and_account() {
    let postgres = common::get_postgres().await;
    let client = common::get_client(&postgres).await;

    let input = json!({
        "useCount": 1
    });

    let response = client
        .post("/xrpc/com.atproto.server.createInviteCode")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", get_admin_token()))
        .body(input.to_string())
        .dispatch()
        .await;
    let invite_code = response
        .into_json::<CreateInviteCodeOutput>()
        .await
        .unwrap()
        .code;

    let account_input = json!({
        "did": "did:plc:khvyd3oiw46vif5gm7hijslk",
        "email": "dummyemail@rsky.com",
        "handle": "dummaccount.rsky.com",
        "password": "password",
        "inviteCode": invite_code
    });

    let response = client
        .post("/xrpc/com.atproto.server.createAccount")
        .header(ContentType::JSON)
        .header(Header::new("Authorization", get_admin_token()))
        .body(account_input.to_string())
        .dispatch()
        .await;
    let response_status = response.status();
    assert_eq!(response_status, Status::Ok);
}

#[tokio::test]
async fn test_create_session() {
    let postgres = common::get_postgres().await;
    let client = common::get_client(&postgres).await;

    let (username, password) = create_account(&client).await;

    // Valid Login
    let session_input = json!({
        "identifier": username,
        "password": password,
    });

    let response = client
        .post("/xrpc/com.atproto.server.createSession")
        .header(ContentType::JSON)
        .body(session_input.to_string())
        .dispatch()
        .await;
    let response_status = response.status();
    assert_eq!(response_status, Status::Ok);

    // Invalid Login
    let session_input = json!({
        "identifier": username,
        "password": password + "1",
    });

    let response = client
        .post("/xrpc/com.atproto.server.createSession")
        .header(ContentType::JSON)
        .body(session_input.to_string())
        .dispatch()
        .await;
    let response_status = response.status();
    assert_eq!(response_status, Status::BadRequest);
}

#[tokio::test]
async fn test_oauth_wellknown() {
    let postgres = common::get_postgres().await;
    let client = common::get_client(&postgres).await;
    let response = client
        .get("/.well-known/oauth-authorization-server")
        .dispatch()
        .await;
    let response_status = response.status();
    let response_body = response
        .into_json::<OAuthAuthorizationServerMetadata>()
        .await
        .unwrap();
    assert_eq!(response_status, Status::Ok);
    let issuer = OAuthIssuerIdentifier::new("https://pds.ripperoni.com").unwrap();
    let expected = OAuthAuthorizationServerMetadata {
        issuer,
        claims_supported: None,
        claims_locales_supported: None,
        claims_parameter_supported: None,
        request_parameter_supported: Some(true),
        request_uri_parameter_supported: Some(true),
        require_request_uri_registration: Some(true),
        scopes_supported: Some(vec![
            "atproto".to_string(),
            "transition:generic".to_string(),
            "transition:chat.bsky".to_string(),
        ]),
        subject_types_supported: Some(vec!["public".to_string()]),
        response_types_supported: Some(vec!["code".to_string()]),
        response_modes_supported: Some(vec![
            "query".to_string(),
            "fragment".to_string(),
            "form_post".to_string(),
        ]),
        grant_types_supported: Some(vec![
            OAuthGrantType::AuthorizationCode,
            OAuthGrantType::RefreshToken,
        ]),
        code_challenge_methods_supported: Some(vec![OAuthCodeChallengeMethod::S256]),
        ui_locales_supported: Some(vec!["en-US".to_string()]),
        id_token_signing_alg_values_supported: None,
        display_values_supported: Some(vec![
            "page".to_string(),
            "popup".to_string(),
            "touch".to_string(),
        ]),
        request_object_signing_alg_values_supported: Some(vec![
            "none".to_string(),
            "RS256".to_string(),
            "RS384".to_string(),
            "RS512".to_string(),
            "PS256".to_string(),
            "PS384".to_string(),
            "PS512".to_string(),
            "ES256".to_string(),
            "ES256K".to_string(),
            "ES384".to_string(),
            "ES512".to_string(),
        ]),
        authorization_response_iss_parameter_supported: Some(true),
        authorization_details_types_supported: None,
        request_object_encryption_alg_values_supported: Some(vec![]),
        request_object_encryption_enc_values_supported: Some(vec![]),
        jwks_uri: Some(WebUri::validate("https://pds.ripperoni.com/oauth/jwks").unwrap()),
        authorization_endpoint: WebUri::validate("https://pds.ripperoni.com/oauth/authorize")
            .unwrap(),
        token_endpoint: WebUri::validate("https://pds.ripperoni.com/oauth/token").unwrap(),
        token_endpoint_auth_methods_supported: Some(vec![
            "none".to_string(),
            "private_key_jwt".to_string(),
        ]),
        token_endpoint_auth_signing_alg_values_supported: Some(vec![
            "RS256".to_string(),
            "RS384".to_string(),
            "RS512".to_string(),
            "PS256".to_string(),
            "PS384".to_string(),
            "PS512".to_string(),
            "ES256".to_string(),
            "ES256K".to_string(),
            "ES384".to_string(),
            "ES512".to_string(),
        ]),
        revocation_endpoint: Some(
            WebUri::validate("https://pds.ripperoni.com/oauth/revoke").unwrap(),
        ),
        introspection_endpoint: Some(
            WebUri::validate("https://pds.ripperoni.com/oauth/introspect").unwrap(),
        ),
        pushed_authorization_request_endpoint: Some(
            WebUri::validate("https://pds.ripperoni.com/oauth/par").unwrap(),
        ),
        require_pushed_authorization_requests: Some(true),
        userinfo_endpoint: None,
        end_session_endpoint: None,
        registration_endpoint: None,
        dpop_signing_alg_values_supported: Some(vec![
            "RS256".to_string(),
            "RS384".to_string(),
            "RS512".to_string(),
            "PS256".to_string(),
            "PS384".to_string(),
            "PS512".to_string(),
            "ES256".to_string(),
            "ES256K".to_string(),
            "ES384".to_string(),
            "ES512".to_string(),
        ]),
        protected_resources: Some(vec![WebUri::validate("https://pds.ripperoni.com").unwrap()]),
        client_id_metadata_document_supported: Some(true),
    };
    assert_eq!(response_body, expected);
}

#[tokio::test]
async fn test_oauth_jwks() {
    let postgres = common::get_postgres().await;
    let client = common::get_client(&postgres).await;
    let response = client.get("/oauth/jwks").dispatch().await;
    let response_status = response.status();
    assert_eq!(response_status, Status::Ok);
    let response_keys = response.into_json::<JwkSet>().await.unwrap();

    let expected = JwkSet {
        keys: vec![Jwk {
            common: CommonParameters {
                public_key_use: Some(PublicKeyUse::Signature),
                key_operations: Some(vec![KeyOperations::Sign]),
                key_algorithm: Some(KeyAlgorithm::PS256),
                key_id: Some("test".to_string()),
                x509_url: None,
                x509_chain: None,
                x509_sha1_fingerprint: None,
                x509_sha256_fingerprint: None,
            },
            algorithm: AlgorithmParameters::EllipticCurve(EllipticCurveKeyParameters {
                key_type: EllipticCurveKeyType::EC,
                curve: EllipticCurve::P256,
                x: "GgskXhf9OJFxYNovWiwq35akQopFXS6Tzuv0Y-B6q8I".to_string(),
                y: "Cv8TnJVvra7TmYsaO-_nwhpD2jpfdnRE_TAeuvxLgJE".to_string(),
            }),
        }],
    };
    assert_eq!(response_keys, expected);
}

#[tokio::test]
async fn test_protected_resource() {
    let postgres = common::get_postgres().await;
    let client = common::get_client(&postgres).await;
    let response = client
        .get("/.well-known/oauth-protected-resource")
        .dispatch()
        .await;
    let response_status = response.status();
    let response_body = response
        .into_json::<OAuthProtectedResourceMetadata>()
        .await
        .unwrap();
    assert_eq!(response_status, Status::Ok);
    let expected = OAuthProtectedResourceMetadata {
        resource: WebUri::validate("https://pds.ripperoni.com").unwrap(),
        authorization_servers: Some(vec![OAuthIssuerIdentifier::new(
            "https://pds.ripperoni.com",
        )
        .unwrap()]),
        jwks_uri: None,
        scopes_supported: Some(vec![]),
        bearer_methods_supported: Some(vec![BearerMethod::Header]),
        resource_signing_alg_values_supported: None,
        resource_documentation: Some(WebUri::validate("https://atproto.com").unwrap()),
        resource_policy_uri: None,
        resource_tos_uri: None,
    };
    assert_eq!(response_body, expected);
}

#[tokio::test]
async fn test_pushed_authorization_request() {
    let postgres = common::get_postgres().await;
    let client = common::get_client(&postgres).await;
    let input = json!(
        {
            "redirect_uri":"https://cleanfollow-bsky.pages.dev/",
            "code_challenge":"RLpoJtb7axWTfWVjH1T5bay2uQ38N8alwaMvoGK2Z10",
            "code_challenge_method":"S256",
            "state":"yfhsnwinGQkORB1eV5Tf7A",
            "login_hint":"ripperoni.com",
            "response_mode":"fragment",
            "response_type":"code",
            "display":"page",
            "scope":"atproto transition:generic",
            "client_id":"https://cleanfollow-bsky.pages.dev/client-metadata.json"
        }
    );
    let response = client
        .post("/oauth/par")
        .header(ContentType::JSON)
        .header(Header::new("dpop-nonce", "eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJUbXR3WkNlUFQ0U1UtZDhEOUJjaDUxOUhfU3JweXFQTGhCNDl4UjhHLWY4IiwieSI6IkFMQjd2a04yNlhpeUtkOWNUTW01cElPMFdRMWZlNENqdXQwZGJETHBhbjgifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ0NjcxNDA3LCJqdGkiOiJoNmZtejlyMHE4OjJ1NjVtYnVyd3pxYjEiLCJodG0iOiJQT1NUIiwiaHR1IjoiaHR0cHM6Ly9wZHMucmlwcGVyb25pLmNvbS9vYXV0aC9wYXIifQ.CuECGFWJsDrGmNoA7uOXHFOENbzfzhk7ZW7NFePYG8Mc3lD5dgE-E8padaRDFT92chgWQKeZos9EWcZMt8CSUQ"))
        .header(Header::new("Sec-Fetch-Dest", "empty"))
        .header(Header::new("Sec-Fetch-Mode", "cors"))
        .header(Header::new("Sec-Fetch-Site", "cross-site"))
        .header(Header::new("Content-Length", "1000"))
        .header(Header::new("Accept", "*/*"))
        .body(input.to_string())
        .dispatch()
        .await;
    let response_status = response.status();
    assert_eq!(response_status, Status::BadRequest);
    let input = json!(
        {
            "redirect_uri":"https://cleanfollow-bsky.pages.dev/",
            "code_challenge":"RLpoJtb7axWTfWVjH1T5bay2uQ38N8alwaMvoGK2Z10",
            "code_challenge_method":"S256",
            "state":"yfhsnwinGQkORB1eV5Tf7A",
            "login_hint":"ripperoni.com",
            "response_mode":"fragment",
            "response_type":"code",
            "display":"page",
            "scope":"atproto transition:generic",
            "client_id":"https://cleanfollow-bsky.pages.dev/client-metadata.json"
        }
    );
    let response = client
        .post("/oauth/par")
        .header(ContentType::JSON)
        .header(Header::new("dpop", "eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJEQTRCVWNzR2ZzT2V6NzlPNzAwcF9rMjFIZFNMNklnSFJSbzlUT0Fha2IwIiwieSI6IjBmaHdQUWNwRXBKSk9Zek5uMXd3UkNzTDRuR2lfNVhwdmdOdHBYeUJUN1EifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ0NjYwNDExLCJqdGkiOiJoNmZoeGZhdjc0OjI4enpvdG1ycTU0MCIsImh0bSI6IlBPU1QiLCJodHUiOiJodHRwczovL3Bkcy5yaXBwZXJvbmkuY29tL29hdXRoL3BhciIsIm5vbmNlIjoiaGdtMU5XSmpJLTRybzN0WFN6M19oTWYwamZOVlFvSmtIU05FbDFRT082USJ9.CC0LA2fjqGDP2YgC-ulCDSo9PgmPCh1bk_AvW6nxvuScE18EaDyxHvV1x1vq2emxTaR3aM8pTsD6-3nhw4yQiw"))
        .header(Header::new("Sec-Fetch-Dest", "empty"))
        .header(Header::new("Sec-Fetch-Mode", "cors"))
        .header(Header::new("Sec-Fetch-Site", "cross-site"))
        .header(Header::new("Content-Length", "1000"))
        .header(Header::new("Accept", "*/*"))
        .body(input.to_string())
        .dispatch()
        .await;
    let response_status = response.status();
    assert_eq!(response_status, Status::Created);
}

#[tokio::test]
async fn test_oauth_sign_in() {
    let postgres = common::get_postgres().await;
    let client = common::get_client(&postgres).await;
    let (username, password) = create_account(&client).await;

    let input = json!(
        {
            "redirect_uri":"https://cleanfollow-bsky.pages.dev/",
            "code_challenge":"RLpoJtb7axWTfWVjH1T5bay2uQ38N8alwaMvoGK2Z10",
            "code_challenge_method":"S256",
            "state":"yfhsnwinGQkORB1eV5Tf7A",
            "login_hint":"dummaccount.rsky.com",
            "response_mode":"fragment",
            "response_type":"code",
            "display":"page",
            "scope":"atproto transition:generic",
            "client_id":"https://cleanfollow-bsky.pages.dev/client-metadata.json"
        }
    );
    let response = client
        .post("/oauth/par")
        .header(ContentType::JSON)
        .header(Header::new("dpop", "eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJEQTRCVWNzR2ZzT2V6NzlPNzAwcF9rMjFIZFNMNklnSFJSbzlUT0Fha2IwIiwieSI6IjBmaHdQUWNwRXBKSk9Zek5uMXd3UkNzTDRuR2lfNVhwdmdOdHBYeUJUN1EifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ0NjYwNDExLCJqdGkiOiJoNmZoeGZhdjc0OjI4enpvdG1ycTU0MCIsImh0bSI6IlBPU1QiLCJodHUiOiJodHRwczovL3Bkcy5yaXBwZXJvbmkuY29tL29hdXRoL3BhciIsIm5vbmNlIjoiaGdtMU5XSmpJLTRybzN0WFN6M19oTWYwamZOVlFvSmtIU05FbDFRT082USJ9.CC0LA2fjqGDP2YgC-ulCDSo9PgmPCh1bk_AvW6nxvuScE18EaDyxHvV1x1vq2emxTaR3aM8pTsD6-3nhw4yQiw"))
        .header(Header::new("Sec-Fetch-Dest", "empty"))
        .header(Header::new("Sec-Fetch-Mode", "cors"))
        .header(Header::new("Sec-Fetch-Site", "cross-site"))
        .header(Header::new("Content-Length", "1000"))
        .header(Header::new("Accept", "*/*"))
        .body(input.to_string())
        .dispatch()
        .await;
    let response_status = response.status();
    assert_eq!(response_status, Status::Created);
    let response_body = response.into_json::<OAuthParResponse>().await.unwrap();

    let input = json!(
        {
            "csrf_token":"b4b3c69fffeb5925",
            "request_uri":response_body.request_uri(),
            "client_id":"https://cleanfollow-bsky.pages.dev/client-metadata.json",
            "credentials":{
                "username":username,
                "password":password,
                "remember":true
            }
        }
    );
    let response = client
        .post("/oauth/authorize/sign-in")
        .header(ContentType::JSON)
        .header(Header::new("dpop", "eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJEQTRCVWNzR2ZzT2V6NzlPNzAwcF9rMjFIZFNMNklnSFJSbzlUT0Fha2IwIiwieSI6IjBmaHdQUWNwRXBKSk9Zek5uMXd3UkNzTDRuR2lfNVhwdmdOdHBYeUJUN1EifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ0NjYwNDExLCJqdGkiOiJoNmZoeGZhdjc0OjI4enpvdG1ycTU0MCIsImh0bSI6IlBPU1QiLCJodHUiOiJodHRwczovL3Bkcy5yaXBwZXJvbmkuY29tL29hdXRoL3BhciIsIm5vbmNlIjoiaGdtMU5XSmpJLTRybzN0WFN6M19oTWYwamZOVlFvSmtIU05FbDFRT082USJ9.CC0LA2fjqGDP2YgC-ulCDSo9PgmPCh1bk_AvW6nxvuScE18EaDyxHvV1x1vq2emxTaR3aM8pTsD6-3nhw4yQiw"))
        .header(Header::new("Sec-Fetch-Dest", "empty"))
        .header(Header::new("Sec-Fetch-Mode", "cors"))
        .header(Header::new("Sec-Fetch-Site", "cross-site"))
        .header(Header::new("Content-Length", "1000"))
        .header(Header::new("Accept", "*/*"))
        .body(input.to_string())
        .dispatch()
        .await;
    let response_status = response.status();
    assert_eq!(response_status, Status::Ok);
}

#[tokio::test]
async fn test_oauth_reject() {
    let postgres = common::get_postgres().await;
    let client = common::get_client(&postgres).await;
    let (username, password) = create_account(&client).await;

    let input = json!(
        {
            "redirect_uri":"https://cleanfollow-bsky.pages.dev/",
            "code_challenge":"RLpoJtb7axWTfWVjH1T5bay2uQ38N8alwaMvoGK2Z10",
            "code_challenge_method":"S256",
            "state":"yfhsnwinGQkORB1eV5Tf7A",
            "login_hint":"dummaccount.rsky.com",
            "response_mode":"fragment",
            "response_type":"code",
            "display":"page",
            "scope":"atproto transition:generic",
            "client_id":"https://cleanfollow-bsky.pages.dev/client-metadata.json"
        }
    );
    let response = client
        .post("/oauth/par")
        .header(ContentType::JSON)
        .header(Header::new("dpop", "eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJEQTRCVWNzR2ZzT2V6NzlPNzAwcF9rMjFIZFNMNklnSFJSbzlUT0Fha2IwIiwieSI6IjBmaHdQUWNwRXBKSk9Zek5uMXd3UkNzTDRuR2lfNVhwdmdOdHBYeUJUN1EifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ0NjYwNDExLCJqdGkiOiJoNmZoeGZhdjc0OjI4enpvdG1ycTU0MCIsImh0bSI6IlBPU1QiLCJodHUiOiJodHRwczovL3Bkcy5yaXBwZXJvbmkuY29tL29hdXRoL3BhciIsIm5vbmNlIjoiaGdtMU5XSmpJLTRybzN0WFN6M19oTWYwamZOVlFvSmtIU05FbDFRT082USJ9.CC0LA2fjqGDP2YgC-ulCDSo9PgmPCh1bk_AvW6nxvuScE18EaDyxHvV1x1vq2emxTaR3aM8pTsD6-3nhw4yQiw"))
        .header(Header::new("Sec-Fetch-Dest", "empty"))
        .header(Header::new("Sec-Fetch-Mode", "cors"))
        .header(Header::new("Sec-Fetch-Site", "cross-site"))
        .header(Header::new("Content-Length", "1000"))
        .header(Header::new("Accept", "*/*"))
        .body(input.to_string())
        .dispatch()
        .await;
    let response_status = response.status();
    assert_eq!(response_status, Status::Created);
    let response_body = response.into_json::<OAuthParResponse>().await.unwrap();

    let input = json!(
        {
            "csrf_token":"b4b3c69fffeb5925",
            "request_uri":response_body.request_uri(),
            "client_id":"https://cleanfollow-bsky.pages.dev/client-metadata.json",
            "credentials":{
                "username":username,
                "password":password,
                "remember":true
            }
        }
    );
    let response = client
        .post("/oauth/authorize/reject?")
        .header(ContentType::JSON)
        .header(Header::new("dpop", "eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJEQTRCVWNzR2ZzT2V6NzlPNzAwcF9rMjFIZFNMNklnSFJSbzlUT0Fha2IwIiwieSI6IjBmaHdQUWNwRXBKSk9Zek5uMXd3UkNzTDRuR2lfNVhwdmdOdHBYeUJUN1EifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ0NjYwNDExLCJqdGkiOiJoNmZoeGZhdjc0OjI4enpvdG1ycTU0MCIsImh0bSI6IlBPU1QiLCJodHUiOiJodHRwczovL3Bkcy5yaXBwZXJvbmkuY29tL29hdXRoL3BhciIsIm5vbmNlIjoiaGdtMU5XSmpJLTRybzN0WFN6M19oTWYwamZOVlFvSmtIU05FbDFRT082USJ9.CC0LA2fjqGDP2YgC-ulCDSo9PgmPCh1bk_AvW6nxvuScE18EaDyxHvV1x1vq2emxTaR3aM8pTsD6-3nhw4yQiw"))
        .header(Header::new("Sec-Fetch-Dest", "empty"))
        .header(Header::new("Sec-Fetch-Mode", "cors"))
        .header(Header::new("Sec-Fetch-Site", "cross-site"))
        .header(Header::new("Content-Length", "1000"))
        .header(Header::new("Accept", "*/*"))
        .body(input.to_string())
        .dispatch()
        .await;
    let response_status = response.status();
    assert_eq!(response_status, Status::Ok);
}

#[tokio::test]
async fn test_oauth_accept() {
    let postgres = common::get_postgres().await;
    let client = common::get_client(&postgres).await;
    let (username, password) = create_account(&client).await;

    let input = json!(
        {
            "redirect_uri":"https://cleanfollow-bsky.pages.dev/",
            "code_challenge":"RLpoJtb7axWTfWVjH1T5bay2uQ38N8alwaMvoGK2Z10",
            "code_challenge_method":"S256",
            "state":"yfhsnwinGQkORB1eV5Tf7A",
            "login_hint":"dummaccount.rsky.com",
            "response_mode":"fragment",
            "response_type":"code",
            "display":"page",
            "scope":"atproto transition:generic",
            "client_id":"https://cleanfollow-bsky.pages.dev/client-metadata.json"
        }
    );
    let response = client
        .post("/oauth/par")
        .header(ContentType::JSON)
        .header(Header::new("dpop", "eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJEQTRCVWNzR2ZzT2V6NzlPNzAwcF9rMjFIZFNMNklnSFJSbzlUT0Fha2IwIiwieSI6IjBmaHdQUWNwRXBKSk9Zek5uMXd3UkNzTDRuR2lfNVhwdmdOdHBYeUJUN1EifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ0NjYwNDExLCJqdGkiOiJoNmZoeGZhdjc0OjI4enpvdG1ycTU0MCIsImh0bSI6IlBPU1QiLCJodHUiOiJodHRwczovL3Bkcy5yaXBwZXJvbmkuY29tL29hdXRoL3BhciIsIm5vbmNlIjoiaGdtMU5XSmpJLTRybzN0WFN6M19oTWYwamZOVlFvSmtIU05FbDFRT082USJ9.CC0LA2fjqGDP2YgC-ulCDSo9PgmPCh1bk_AvW6nxvuScE18EaDyxHvV1x1vq2emxTaR3aM8pTsD6-3nhw4yQiw"))
        .header(Header::new("Sec-Fetch-Dest", "empty"))
        .header(Header::new("Sec-Fetch-Mode", "cors"))
        .header(Header::new("Sec-Fetch-Site", "cross-site"))
        .header(Header::new("Content-Length", "1000"))
        .header(Header::new("Accept", "*/*"))
        .body(input.to_string())
        .dispatch()
        .await;
    let response_status = response.status();
    assert_eq!(response_status, Status::Created);
    let response_body = response.into_json::<OAuthParResponse>().await.unwrap();

    let input = json!(
        {
            "csrf_token":"b4b3c69fffeb5925",
            "request_uri":response_body.request_uri().clone(),
            "client_id":"https://cleanfollow-bsky.pages.dev/client-metadata.json",
            "credentials":{
                "username":username.clone(),
                "password":password.clone(),
                "remember":true
            }
        }
    );
    let response = client
        .post("/oauth/authorize/sign-in")
        .header(ContentType::JSON)
        .header(Header::new("dpop", "eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiJEQTRCVWNzR2ZzT2V6NzlPNzAwcF9rMjFIZFNMNklnSFJSbzlUT0Fha2IwIiwieSI6IjBmaHdQUWNwRXBKSk9Zek5uMXd3UkNzTDRuR2lfNVhwdmdOdHBYeUJUN1EifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ0NjYwNDExLCJqdGkiOiJoNmZoeGZhdjc0OjI4enpvdG1ycTU0MCIsImh0bSI6IlBPU1QiLCJodHUiOiJodHRwczovL3Bkcy5yaXBwZXJvbmkuY29tL29hdXRoL3BhciIsIm5vbmNlIjoiaGdtMU5XSmpJLTRybzN0WFN6M19oTWYwamZOVlFvSmtIU05FbDFRT082USJ9.CC0LA2fjqGDP2YgC-ulCDSo9PgmPCh1bk_AvW6nxvuScE18EaDyxHvV1x1vq2emxTaR3aM8pTsD6-3nhw4yQiw"))
        .header(Header::new("Sec-Fetch-Dest", "empty"))
        .header(Header::new("Sec-Fetch-Mode", "cors"))
        .header(Header::new("Sec-Fetch-Site", "cross-site"))
        .header(Header::new("Content-Length", "1000"))
        .header(Header::new("Accept", "*/*"))
        .body(input.to_string())
        .dispatch()
        .await;
    let response_status = response.status();
    assert_eq!(response_status, Status::Ok);
    let sign_in_body = response.into_json::<SignInResponse>().await.unwrap();

    let url = format!("/oauth/authorize/accept?request_uri={request_uri}&account_sub={sub}&client_id={client_id}&csrf_token={csrf_token}", request_uri = response_body.request_uri(), sub = sign_in_body.account.sub, client_id = "https://cleanfollow-bsky.pages.dev/client-metadata.json", csrf_token = "temp");
    let response = client.get(url).dispatch().await;
}

#[tokio::test]
async fn test_oauth_introspect() {
    unimplemented!()
}

#[tokio::test]
async fn test_oauth_token() {
    unimplemented!()
}

#[tokio::test]
async fn test_oauth_revoke() {
    unimplemented!()
}
