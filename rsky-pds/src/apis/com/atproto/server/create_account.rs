/*{"handle":"rudy-alt-3.blacksky.app","password":"H5r%FH@%hhg6rvYF","email":"him+testcreate@rudyfraser.com","inviteCode":"blacksky-app-fqytt-7473e"}*/
use crate::models::{InternalErrorCode, InternalErrorMessageResponse};
use crate::DbConn;
use chrono::offset::Utc as UtcOffset;
use chrono::DateTime;
use diesel::prelude::*;
use email_address::*;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rsky_lexicon::com::atproto::server::{CreateAccountInput, CreateAccountOutput};
use secp256k1::{Keypair, Secp256k1, SecretKey};
use std::env;
use std::time::SystemTime;
use aws_sdk_s3::config::BehaviorVersion;
use futures::executor;
use crate::repo::{ActorStore, Repo};
use crate::repo::aws::s3::S3BlobStore;
use crate::storage::SqlRepoReader;

#[rocket::post(
    "/xrpc/com.atproto.server.createAccount",
    format = "json",
    data = "<body>"
)]
pub async fn create_account(
    body: Json<CreateAccountInput>,
    connection: DbConn,
) -> Result<Json<CreateAccountOutput>, status::Custom<Json<InternalErrorMessageResponse>>> {
    use crate::schema::pds::account::dsl as UserSchema;
    use crate::schema::pds::actor::dsl as ActorSchema;
    // TO DO: Check if there is an invite code
    // TO DO: Throw error for any plcOp input

    let mut error_msg: Option<String> = None;
    if body.email.is_none() {
        error_msg = Some("Email is required".to_owned());
    };
    if body.password.is_none() {
        error_msg = Some("Password is required".to_owned());
    };
    if body.did.is_some() {
        error_msg = Some("Not yet allowing people to bring their own DID".to_owned());
    };
    if let Some(email) = &body.email {
        let e_slice: &str = &email[..]; // take a full slice of the string
        if !EmailAddress::is_valid(e_slice) {
            error_msg = Some("Invalid email".to_owned());
        }
    }
    // TO DO: Normalize handle as well
    if !super::validate_handle(&body.handle) {
        error_msg = Some("Invalid handle".to_owned());
    };
    // TO DO: Check that the invite code still has uses

    if error_msg.is_some() {
        let internal_error = InternalErrorMessageResponse {
            code: Some(InternalErrorCode::InternalError),
            message: error_msg,
        };
        return Err(status::Custom(
            Status::InternalServerError,
            Json(internal_error),
        ));
    };
    let result = connection
        .run(move |conn| {
            match super::lookup_user_by_handle(&body.handle, conn) {
                Ok(_) => {
                    let internal_error = InternalErrorMessageResponse {
                        code: Some(InternalErrorCode::InternalError),
                        message: Some(format!(
                            "User already exists with handle '{}'",
                            &body.handle
                        )),
                    };
                    return Err(status::Custom(
                        Status::InternalServerError,
                        Json(internal_error),
                    ));
                }
                Err(error) => println!("Handle is available: {error:?}"), // handle is available, lets go
            }
            let mut recovery_key: Option<String> = None;
            if let Some(input_recovery_key) = &body.recovery_key {
                recovery_key = Some(input_recovery_key.to_owned());
            }
            // TO DO: Lookup user by email as well
            

            let did;
            let secp = Secp256k1::new();
            let private_key = env::var("PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX").unwrap();
            let secret_key =
                SecretKey::from_slice(&hex::decode(private_key.as_bytes()).unwrap()).unwrap();
            let signing_key = Keypair::from_secret_key(&secp, &secret_key);
            match super::create_did_and_plc_op(&body.handle, &body, signing_key) {
                Ok(did_resp) => did = did_resp,
                Err(error) => {
                    eprintln!("{:?}", error);
                    let internal_error = InternalErrorMessageResponse {
                        code: Some(InternalErrorCode::InternalError),
                        message: Some("Failed to create DID.".to_owned()),
                    };
                    return Err(status::Custom(
                        Status::InternalServerError,
                        Json(internal_error),
                    ));
                }
            }
            
            // TO DO: Move this to main.rs
            let config = async {
                return aws_config::load_defaults(BehaviorVersion::v2023_11_09()).await;
            };
            let config = executor::block_on(config);
            
            let mut actor_store = ActorStore::new(
                SqlRepoReader::new(None),
                S3BlobStore::new(did.clone(), &config)
            );
            let commit = match actor_store.create_repo(
                did.clone(),
                signing_key,
                Vec::new()
            ) {
                Ok(commit) => commit,
                Err(error) => {
                    eprintln!("{:?}", error);
                    let internal_error = InternalErrorMessageResponse {
                        code: Some(InternalErrorCode::InternalError),
                        message: Some("Failed to create account.".to_owned()),
                    };
                    return Err(status::Custom(
                        Status::InternalServerError,
                        Json(internal_error),
                    ));
                }
            };
            
            let system_time = SystemTime::now();
            let dt: DateTime<UtcOffset> = system_time.into();

            let new_user_account = (
                UserSchema::did.eq(did.clone()),
                UserSchema::email.eq(body.email.clone().unwrap()),
                UserSchema::password.eq(body.password.clone().unwrap()),
                UserSchema::recoveryKey.eq(&recovery_key),
                UserSchema::createdAt.eq(format!("{}", dt.format("%+"))),
            );
            let new_actor = (
                ActorSchema::did.eq(did.clone()),
                ActorSchema::handle.eq(body.handle.clone()),
                ActorSchema::createdAt.eq(format!("{}", dt.format("%+"))),
            );
            match diesel::insert_into(UserSchema::account)
                .values(&new_user_account)
                .execute(conn)
            {
                Ok(_) => (),
                Err(error) => eprintln!("Internal Error: {error}"),
            };
            match diesel::insert_into(ActorSchema::actor)
                .values(&new_actor)
                .execute(conn)
            {
                Ok(_) => (),
                Err(error) => eprintln!("Internal Error: {error}"),
            };
            todo!();
        })
        .await;
    // TO DO: repoman.InitNewActor?
    // TO DO: Create auth token for user
    // TO DO: Return output
    result
}
