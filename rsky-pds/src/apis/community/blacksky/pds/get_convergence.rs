//! `community.blacksky.pds.getConvergence` -- whether an account's state on
//! this server agrees with what it has published, with nothing owed to any
//! worker. Admin auth.

use crate::account_manager::AccountManager;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AdminToken;
use crate::convergence::{convergence, Convergence, ConvergenceContext};
use crate::lifecycle::LifecycleStore;
use crate::repair::RepairStore;
use crate::SharedSequencer;
use rocket::serde::json::Json;
use rocket::State;

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/community.blacksky.pds.getConvergence?<did>")]
pub async fn get_convergence(
    _admin: AdminToken,
    did: String,
    actor_store: &State<ActorStore>,
    account_manager: AccountManager,
    sequencer: &State<SharedSequencer>,
    lifecycle: &State<LifecycleStore>,
    repairs: &State<RepairStore>,
) -> Result<Json<Convergence>, ApiError> {
    let report = convergence(
        &ConvergenceContext {
            actor_store,
            account_manager: &account_manager,
            sequencer,
            lifecycle,
            repairs,
        },
        &did,
    )
    .await?;
    Ok(Json(report))
}
