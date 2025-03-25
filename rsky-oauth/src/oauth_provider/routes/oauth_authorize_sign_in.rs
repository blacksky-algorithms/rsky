use crate::oauth_provider::routes::{SharedDeviceManager, SharedOAuthProvider};
use rocket::{get, post, State};

#[post("/oauth/authorize/sign-in")]
pub async fn oauth_authorize_sign_in(
    shared_oauth_provider: &State<SharedOAuthProvider>,
    shared_device_manager: &State<SharedDeviceManager>,
) {
    //     let oauth_provider = shared_oauth_provider.oauth_provider.write().await;
    //     let device_manager = shared_device_manager.device_manager.read().await;
    //     // device_manager.load()
    //
    //     let data = oauth_provider
    //         .authorize(
    //             &body.device_id,
    //             &body.credentials,
    //             &body.authorization_request,
    //         )
    //         .await;
    //
    //     match data {
    //         Ok(data) => match data {
    //             AuthorizationResult::Redirect => {}
    //             AuthorizationResult::Authorize => {}
    //         },
    //         Err(e) => {}
    //     }
}
