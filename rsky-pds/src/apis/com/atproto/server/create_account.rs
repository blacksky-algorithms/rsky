/*{"handle":"rudy-alt-3.blacksky.app","password":"H5r%FH@%hhg6rvYF","email":"him+testcreate@rudyfraser.com","inviteCode":"blacksky-app-fqytt-7473e"}*/
use rocket::serde::json::Json;
use rocket::response::status;
use rocket::http::Status;
use diesel::prelude::*;
use std::time::SystemTime;
use chrono::offset::Utc as UtcOffset;
use chrono::DateTime;
use crate::DbConn;
use rsky_lexicon::com::atproto::server::{CreateAccountInput, CreateAccountOutput, CreateInviteCodeOutput};
use crate::models::{InternalErrorMessageResponse, InternalErrorCode};
use email_address::*;
use secp256k1::{Secp256k1, Keypair};

#[rocket::post("/xrpc/com.atproto.server.createAccount", format = "json", data = "<body>")]
pub async fn create_account(
    body: Json<CreateAccountInput>,
    connection: DbConn
) -> Result<Json<CreateAccountOutput>, status::Custom<Json<InternalErrorMessageResponse>>> {
    use crate::schema::pds::user_account::dsl as UserSchema;
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
    if let Some(email) = &body.email {
        let e_slice: &str = &email[..];  // take a full slice of the string
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
                        message: Some(format!("User already exists with handle '{}'",&body.handle)),
                    };
                    return Err(status::Custom(
                        Status::InternalServerError,
                        Json(internal_error),
                    ));
                },
                Err(error) => println!("Handle is available: {error:?}") // handle is available, lets go
            }
            let mut recovery_key: Option<String> = None;
            if let Some(input_recovery_key) = &body.recovery_key {
                recovery_key = Some(input_recovery_key.to_owned());
            }
            // TO DO: Lookup user by email as well

            // TO DO: If not DID provided in input, use recovery key to create a new one
            // determine the did & any plc ops we need to send
            // if the provided did document is poorly setup, we throw
            // const signingKey = await Secp256k1Keypair.create({ exportable: true })
            // const { did, plcOp } = input.did
            //     ? await validateExistingDid(ctx, handle, input.did, signingKey)
            //     : await createDidAndPlcOp(ctx, handle, input, signingKey)
            let did;
            let secp = Secp256k1::new();
            let signing_key = Keypair::new(&secp, &mut rand::thread_rng());
            match super::create_did_and_plc_op(
                &body.handle,
                signing_key) {
                Ok(did_resp) => did = did_resp,
                Err(error) => {
                    eprintln!("{:?}",error);
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
            /*
            if let Some(input_did) = &body.did {
                did = super::validate_existing_did(&body.handle, input_did, signing_key);
            } else {
                did = super::create_did_and_plc_op(&body.handle, body.into_inner(), signing_key).await?;
            }*/

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
            match diesel::insert_into(UserSchema::user_account)
                .values(&new_user_account)
                .execute(conn) {
                    Ok(_) =>(),
                    Err(error) => eprintln!("Internal Error: {error}")
            };
            match diesel::insert_into(ActorSchema::actor)
                .values(&new_actor)
                .execute(conn) {
                    Ok(_) =>(),
                    Err(error) => eprintln!("Internal Error: {error}")
            };
            todo!();
        })
        .await;
    // TO DO: repoman.InitNewActor?
    // TO DO: Create auth token for user
    // TO DO: Return output
    result
}