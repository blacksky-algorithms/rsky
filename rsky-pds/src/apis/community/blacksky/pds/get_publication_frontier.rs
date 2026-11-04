//! `community.blacksky.pds.getPublicationFrontier` -- how far an account's
//! publication history on this server reaches, for a downstream index that
//! reconciles against it. Admin auth: the answer names revisions and hosting
//! history that are nobody else's business.

use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AdminToken;
use crate::config::ServerConfig;
use crate::frontier::{publication_frontier, PublicationFrontier};
use crate::lifecycle::LifecycleStore;
use crate::plc;
use crate::SharedSequencer;
use rocket::serde::json::Json;
use rocket::State;

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/community.blacksky.pds.getPublicationFrontier?<did>")]
pub async fn get_publication_frontier(
    _admin: AdminToken,
    did: String,
    actor_store: &State<ActorStore>,
    sequencer: &State<SharedSequencer>,
    lifecycle: &State<LifecycleStore>,
    cfg: &State<ServerConfig>,
) -> Result<Json<PublicationFrontier>, ApiError> {
    let plc = plc::Client::new(cfg.identity.plc_url.clone());
    let frontier = publication_frontier(
        actor_store,
        sequencer,
        lifecycle,
        &plc,
        &cfg.service.public_url,
        &did,
    )
    .await?;
    Ok(Json(frontier))
}
