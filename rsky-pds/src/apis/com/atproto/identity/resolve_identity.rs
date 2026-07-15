use crate::account_manager::AccountManager;
use crate::apis::ApiError;
use crate::SharedIdResolver;
use rocket::serde::json::Json;
use rocket::State;
use rsky_common::get_handle;
use rsky_identity::types::DidDocument;
use rsky_lexicon::com::atproto::identity::IdentityInfo;
use rsky_syntax::handle::INVALID_HANDLE;

pub enum Identifier {
    Did(String),
    Handle(String),
}

pub fn classify_identifier(identifier: &str) -> Identifier {
    if identifier.starts_with("did:") {
        Identifier::Did(identifier.to_string())
    } else {
        Identifier::Handle(identifier.to_lowercase())
    }
}

pub fn handles_match(doc_handle: &str, handle: &str) -> bool {
    doc_handle.eq_ignore_ascii_case(handle)
}

/// Resolves a handle to a DID: accounts hosted on this PDS are answered
/// locally, all other handles resolve over the network.
pub async fn resolve_handle_to_did(
    handle: &String,
    id_resolver: &State<SharedIdResolver>,
    account_manager: &AccountManager,
) -> Result<String, ApiError> {
    if let Ok(Some(user)) = account_manager.get_account(handle, None).await {
        return Ok(user.did);
    }
    let resolved = {
        let mut lock = id_resolver.id_resolver.write().await;
        lock.handle.resolve(handle).await
    };
    match resolved {
        Ok(Some(did)) => Ok(did),
        _ => Err(ApiError::BadRequest(
            "HandleNotFound".to_string(),
            format!("unable to resolve handle: {handle}"),
        )),
    }
}

pub async fn resolve_did_doc(
    did: &String,
    force_refresh: bool,
    id_resolver: &State<SharedIdResolver>,
) -> Result<DidDocument, ApiError> {
    let doc = {
        let lock = id_resolver.id_resolver.write().await;
        lock.did.resolve(did.clone(), Some(force_refresh)).await
    };
    match doc {
        Ok(Some(doc)) => Ok(doc),
        _ => Err(ApiError::BadRequest(
            "DidNotFound".to_string(),
            format!("could not resolve DID: {did}"),
        )),
    }
}

pub async fn inner_resolve_identity(
    identifier: String,
    force_refresh: bool,
    id_resolver: &State<SharedIdResolver>,
    account_manager: &AccountManager,
) -> Result<IdentityInfo, ApiError> {
    let (did, input_handle) = match classify_identifier(&identifier) {
        Identifier::Did(did) => (did, None),
        Identifier::Handle(handle) => {
            let did = resolve_handle_to_did(&handle, id_resolver, account_manager).await?;
            (did, Some(handle))
        }
    };
    let doc = resolve_did_doc(&did, force_refresh, id_resolver).await?;
    let doc_handle = get_handle(&doc);
    let handle = match (doc_handle, input_handle) {
        // the input handle resolved to this DID; validated if the doc claims it back
        (Some(doc_handle), Some(input_handle)) if handles_match(&doc_handle, &input_handle) => {
            doc_handle
        }
        (Some(_), Some(_)) => INVALID_HANDLE.to_string(),
        // DID input: bi-directionally verify the handle claimed by the doc
        (Some(doc_handle), None) => {
            match resolve_handle_to_did(&doc_handle.to_lowercase(), id_resolver, account_manager)
                .await
            {
                Ok(resolved_did) if resolved_did == did => doc_handle,
                _ => INVALID_HANDLE.to_string(),
            }
        }
        (None, _) => INVALID_HANDLE.to_string(),
    };
    let did_doc = serde_json::to_value(&doc).map_err(|error| {
        tracing::error!("@LOG: ERROR: {error}");
        ApiError::RuntimeError
    })?;
    Ok(IdentityInfo {
        did,
        handle,
        did_doc,
    })
}

/// Resolves an identity (DID or Handle) to a full identity (DID document and
/// verified handle).
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.identity.resolveIdentity?<identifier>")]
pub async fn resolve_identity(
    identifier: String,
    id_resolver: &State<SharedIdResolver>,
    account_manager: AccountManager,
) -> Result<Json<IdentityInfo>, ApiError> {
    let info = inner_resolve_identity(identifier, false, id_resolver, &account_manager).await?;
    Ok(Json(info))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_dids_and_handles() {
        assert!(matches!(
            classify_identifier("did:plc:w4xbfzo7kqfes5zb7r6qv3rw"),
            Identifier::Did(_)
        ));
        assert!(matches!(
            classify_identifier("did:web:example.com"),
            Identifier::Did(_)
        ));
        match classify_identifier("Alice.Test") {
            Identifier::Handle(handle) => assert_eq!(handle, "alice.test"),
            Identifier::Did(_) => panic!("expected handle"),
        }
    }

    #[test]
    fn matches_handles_case_insensitively() {
        assert!(handles_match("alice.test", "Alice.Test"));
        assert!(!handles_match("alice.test", "bob.test"));
    }
}
