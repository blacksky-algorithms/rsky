use crate::oauth_provider::device::device_data::DeviceData;
use crate::oauth_provider::device::device_details::{extract_device_details, DeviceDetails};
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::device::device_store::DeviceStore;
use crate::oauth_provider::device::session_id::{generate_session_id, SessionId};
use crate::oauth_provider::errors::OAuthError;
use rocket::yansi::Paint;
use rocket::{Request, Response};
use std::sync::Arc;
use tokio::sync::RwLock;

struct CookieOptions {
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
    pub cookie: CookieOptions,
}

impl Default for DeviceManagerOptions {
    fn default() -> Self {
        let cookie_options = CookieOptions {
            device: "device-id".to_string(),
            session: "session-id".to_string(),
            path: "/oauth/authorize".to_string(),
            age: Some(10f64 * 365.2 * 24f64 * 60f64 * 60e3),
            secure: true,
            same_site: "lax".to_string(),
        };
        Self {
            trust_proxy: true,
            rotation_rate: 5f64 * 60e3f64,
            cookie: cookie_options,
        }
    }
}

pub struct DeviceManager {
    store: Arc<RwLock<dyn DeviceStore>>,
    options: DeviceManagerOptions,
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
        let options = options.unwrap_or_else(|| DeviceManagerOptions::default());
        Self { store, options }
    }

    pub async fn load(&self, req: &Request<'_>, res: Response<'_>, force_rotate: bool) -> DeviceId {
        unimplemented!()
        // let cookie = self.get_cookie().await;
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

    pub async fn rotate(
        &mut self,
        req: Request<'_>,
        res: Response<'_>,
        device_id: DeviceId,
        data: Option<DeviceData>,
    ) {
        let session_id = generate_session_id().await;

        let data = match data {
            Some(data) => DeviceData {
                user_agent: data.user_agent,
                ip_address: data.ip_address,
                session_id: session_id.clone(),
                last_seen_at: 0,
            },
            None => DeviceData {
                user_agent: None,
                ip_address: "".to_string(),
                session_id: session_id.clone(),
                last_seen_at: 0,
            },
        };
        let _ = self
            .store
            .blocking_write()
            .update_device(device_id.clone(), data);
        self.set_cookie(req, res, device_id, session_id)
    }

    async fn create(&self, req: &Request<'_>) {
        unimplemented!()
    }

    async fn refresh(&self, req: Request<'_>, device_id: DeviceId, session_id: SessionId) {
        let data = self
            .store
            .blocking_read()
            .read_device(device_id)
            .await
            .unwrap();
        if data.is_none() {
            return self.create(&req).await;
        }

        if session_id != data.unwrap().session_id {}
    }

    async fn get_cookie(&self, req: Request<'_>) -> Result<(), OAuthError> {
        let cookies = req.cookies();
        let device = match cookies.get(self.options.cookie.device.as_str()) {
            None => None,
            Some(device_cookie) => {
                if let Ok(device_id) = DeviceId::new(device_cookie.value()) {
                    Some((device_id, false))
                } else {
                    None
                }
            }
        };
        let session = match cookies.get(self.options.cookie.session.as_str()) {
            None => None,
            Some(session_cookie) => {
                if let Ok(session_id) = SessionId::new(session_cookie.value()) {
                    Some((session_id, false))
                } else {
                    None
                }
            }
        };

        // Silently ignore invalid cookies
        if device.is_none() || session.is_none() {
            // If the device cookie is valid, let's cleanup the DB
            if let Some(device) = device {
                self.store.blocking_write().delete_device(device.0).await?
            }
        }

        //TODO
        Ok(())
    }

    fn set_cookie(&self, req: Request, res: Response, device_id: DeviceId, session_id: SessionId) {
        req.cookies()
            .add((self.options.cookie.device.clone(), device_id.into_inner()));
        req.cookies()
            .add((self.options.cookie.session.clone(), session_id.into_inner()));
    }

    fn get_device_details(&self, req: &Request) -> DeviceDetails {
        extract_device_details(req)
    }
}
