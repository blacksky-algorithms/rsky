use crate::apis::ApiError;
use crate::SharedIdResolver;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::identity::ResolveDidOutput;

/// Resolves DID to DID document. Does not bi-directionally verify handle.
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.identity.resolveDid?<did>")]
pub async fn resolve_did(
    did: String,
    id_resolver: &State<SharedIdResolver>,
) -> Result<Json<ResolveDidOutput>, ApiError> {
    let doc = {
        let lock = id_resolver.id_resolver.write().await;
        lock.did.resolve(did.clone(), None).await
    };
    match doc {
        Ok(Some(doc)) => match serde_json::to_value(&doc) {
            Ok(did_doc) => Ok(Json(ResolveDidOutput { did_doc })),
            Err(error) => {
                tracing::error!("@LOG: ERROR: {error}");
                Err(ApiError::RuntimeError)
            }
        },
        Ok(None) => Err(ApiError::BadRequest(
            "DidNotFound".to_string(),
            format!("could not resolve DID: {did}"),
        )),
        Err(error) => {
            tracing::error!("@LOG: ERROR: {error}");
            Err(ApiError::BadRequest(
                "DidNotFound".to_string(),
                format!("could not resolve DID: {did}"),
            ))
        }
    }
}
