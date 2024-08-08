use crate::SharedIdResolver;
use anyhow::{bail, Result};
use rocket::State;
use rsky_identity::errors::Error;
use rsky_identity::types::DidDocument;

// provides http-friendly errors during did resolution
pub async fn get_did_doc(
    id_resolver: &State<SharedIdResolver>,
    did: &String,
) -> Result<DidDocument> {
    let mut lock = id_resolver.id_resolver.write().await;
    match lock.did.resolve(did.clone(), None).await {
        Err(err) => match err.downcast_ref() {
            Some(Error::PoorlyFormattedDidDocumentError(_)) => bail!("invalid did document: {did}"),
            _ => bail!("could not resolve did document: {did}"),
        },
        Ok(Some(resolved)) => Ok(resolved),
        _ => bail!("could not resolve did document: {did}"),
    }
}
