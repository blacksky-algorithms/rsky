use crate::jwk::Keyset;
use crate::oauth_provider::client::client_manager::{ClientManager, ClientManagerCreator};
use crate::oauth_provider::client::client_store::ClientStore;
use crate::oauth_provider::device::device_details::{extract_device_details, DeviceDetails};
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::device::device_store::DeviceStore;
use crate::oauth_provider::device::session_id::SessionId;
use crate::oauth_types::OAuthAuthorizationServerMetadata;
use rocket::Request;
use std::cmp::PartialEq;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct DeviceManagerOptions {
    /**
     * Controls whether the IP address is read from the `X-Forwarded-For` header
     * (if `true`), or from the `req.socket.remoteAddress` property (if `false`).
     *
     * @default true // (nowadays, most requests are proxied)
     */
    pub trust_proxy: bool,

    /**
     * Amount of time (in ms) after which session IDs will be rotated
     *
     * @default 300e3 // (5 minutes)
     */
    pub rotation_rate: f64,

    /**
     * Cookie options
     */
    // cookie: {
    // keys: undefined as undefined | Keygrip,

    /**
     * Name of the cookie used to identify the device
     *
     * @default 'session-id'
     */
    pub device: String,

    /**
     * Name of the cookie used to identify the session
     *
     * @default 'session-id'
     */
    pub session: String,

    /**
     * Url path for the cookie
     *
     * @default '/oauth/authorize'
     */
    pub path: String,

    /**
     * Amount of time (in ms) after which the session cookie will expire.
     * If set to `null`, the cookie will be a session cookie (deleted when the
     * browser is closed).
     *
     * @default 10 * 365.2 * 24 * 60 * 60e3 // 10 years (in ms)
     */
    pub age: Option<f64>,

    /**
     * Controls whether the cookie is only sent over HTTPS (if `true`), or also
     * over HTTP (if `false`). This should **NOT** be set to `false` in
     * production.
     */
    pub secure: bool,

    /**
     * Controls whether the cookie is sent along with cross-site requests.
     *
     * @default 'lax'
     */
    pub same_site: String,
}

impl Default for DeviceManagerOptions {
    fn default() -> Self {
        Self {
            trust_proxy: true,
            rotation_rate: 5f64 * 60e3f64,
            device: "device-id".to_string(),
            session: "session-id".to_string(),
            path: "/oauth/authorize".to_string(),
            age: Some(10f64 * 365.2 * 24f64 * 60f64 * 60e3),
            secure: true,
            same_site: "lax".to_string(),
        }
    }
}

pub struct DeviceManager {
    store: Arc<RwLock<dyn DeviceStore>>,
    device_manager_options: DeviceManagerOptions,
}

pub type DeviceManagerCreator = Box<
    dyn Fn(Arc<RwLock<dyn DeviceStore>>, Option<DeviceManagerOptions>) -> DeviceManager
        + Send
        + Sync,
>;

/**
 * This class provides an abstraction for keeping track of DEVICE sessions. It
 * relies on a {@link DeviceStore} to persist session data and a cookie to
 * identify the session.
 */
impl DeviceManager {
    pub fn creator() -> DeviceManagerCreator {
        Box::new(
            move |store: Arc<RwLock<dyn DeviceStore>>,
                  options: Option<DeviceManagerOptions>|
                  -> DeviceManager { DeviceManager::new(store, options) },
        )
    }

    pub fn new(store: Arc<RwLock<dyn DeviceStore>>, options: Option<DeviceManagerOptions>) -> Self {
        let device_manager_options = options.unwrap_or_else(|| DeviceManagerOptions::default());
        Self {
            store,
            device_manager_options,
        }
    }

    pub async fn load(&self, req: &Request<'_>) -> DeviceId {
        unimplemented!()
        // let cookie = self.get_cookie().await;
        // match cookie {
        //     None => {
        //         self.create(req).await
        //     }
        //     Some(cookie) => {
        //
        //     }
        // }
    }

    async fn create(&self, req: &Request<'_>) {}

    async fn refresh(&self, req: &Request<'_>, device_id: DeviceId, session_id: SessionId) {
        unimplemented!()
        // let data = self.store.read_device(device_id).await;
        // if data.is_none() {
        //     return self.create(req).await
        // }
        //
        // if session_id != data.unwrap().session_id {
        //
        // }
    }

    async fn get_cookie(&self) {}

    fn set_cookie(&self) {}

    fn write_cookie(&self, name: &str, value: Option<&str>) {}

    fn get_device_details(&self, req: &Request) -> DeviceDetails {
        extract_device_details(req)
    }
}
