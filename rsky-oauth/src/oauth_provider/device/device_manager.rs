use crate::oauth_provider::constants::SESSION_FIXATION_MAX_AGE;
use crate::oauth_provider::device::device_data::DeviceData;
use crate::oauth_provider::device::device_details::{extract_device_details, DeviceDetails};
use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::device::device_store::{DeviceStore, PartialDeviceData};
use crate::oauth_provider::device::session_id::SessionId;
use crate::oauth_provider::errors::OAuthError;
use crate::oauth_provider::now_as_secs;
use rocket::Request;
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

    pub async fn load(
        &mut self,
        req: &Request<'_>,
        force_rotate: bool,
    ) -> Result<DeviceId, OAuthError> {
        let (device_id, session_id, must_rotate) = match self.get_cookie(req).await? {
            None => return self.create(req).await,
            Some(cookie) => cookie,
        };
        self.refresh(
            req,
            device_id,
            session_id,
            Some(must_rotate || force_rotate),
        )
        .await
    }

    pub async fn rotate(
        &mut self,
        req: &Request<'_>,
        device_id: DeviceId,
        data: Option<PartialDeviceData>,
    ) -> Result<(), OAuthError> {
        let session_id = SessionId::generate();

        let data = match data {
            Some(data) => PartialDeviceData {
                user_agent: data.user_agent,
                ip_address: data.ip_address,
                session_id: Some(session_id.clone()),
                last_seen_at: Some(now_as_secs()),
            },
            None => PartialDeviceData {
                user_agent: None,
                ip_address: None,
                session_id: Some(session_id.clone()),
                last_seen_at: Some(now_as_secs()),
            },
        };
        let mut store = self.store.write().await;
        store.update_device(device_id.clone(), data).await?;
        self.set_cookie(req, device_id, session_id);
        Ok(())
    }

    async fn create(&self, req: &Request<'_>) -> Result<DeviceId, OAuthError> {
        let details = self.get_device_details(req);

        let device_id = DeviceId::generate();
        let session_id = SessionId::generate();
        let device_data = DeviceData {
            user_agent: details.user_agent,
            ip_address: details.ip_address,
            session_id: session_id.clone(),
            last_seen_at: now_as_secs(),
        };

        let mut store = self.store.write().await;
        store.create_device(device_id.clone(), device_data).await?;
        self.set_cookie(req, device_id.clone(), session_id);
        Ok(device_id)
    }

    async fn refresh(
        &mut self,
        req: &Request<'_>,
        device_id: DeviceId,
        session_id: SessionId,
        force_rotate: Option<bool>,
    ) -> Result<DeviceId, OAuthError> {
        let mut force_rotate = force_rotate.unwrap_or(false);
        let store = self.store.read().await;
        let data = match store.read_device(device_id.clone()).await? {
            None => {
                return self.create(req).await;
            }
            Some(data) => data,
        };
        drop(store);

        let last_seen_at = data.last_seen_at;
        let age = now_as_secs() - last_seen_at;

        if session_id != data.session_id {
            if age <= SESSION_FIXATION_MAX_AGE {
                // The cookie was probably rotated by a concurrent request. Let's
                // update the cookie with the new sessionId.
                force_rotate = true;
            } else {
                // Something's wrong. Let's create a new session.
                let mut store = self.store.write().await;
                store.delete_device(device_id).await?;
                return self.create(req).await;
            }
        }

        let details = self.get_device_details(req);

        if force_rotate
            || details.ip_address != data.ip_address
            || details.user_agent != data.user_agent
            || age as f64 > self.options.rotation_rate
        {
            let user_agent = match details.user_agent {
                None => data.user_agent,
                Some(user_agent) => Some(user_agent),
            };
            let data = PartialDeviceData {
                user_agent,
                ip_address: Some(details.ip_address),
                session_id: None,
                last_seen_at: None,
            };
            self.rotate(req, device_id.clone(), Some(data)).await?;
        }

        Ok(device_id)
    }

    async fn get_cookie(
        &self,
        req: &Request<'_>,
    ) -> Result<Option<(DeviceId, SessionId, bool)>, OAuthError> {
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
                let mut store = self.store.write().await;
                store.delete_device(device.0).await?;
                drop(store);
            }
            return Ok(None);
        }
        let device = device.unwrap();
        let session = session.unwrap();

        Ok(Some((device.0, session.0, session.1 || device.1)))
    }

    fn set_cookie(&self, req: &Request, device_id: DeviceId, session_id: SessionId) {
        req.cookies()
            .add((self.options.cookie.device.clone(), device_id.into_inner()));
        req.cookies()
            .add((self.options.cookie.session.clone(), session_id.into_inner()));
    }

    fn get_device_details(&self, req: &Request) -> DeviceDetails {
        extract_device_details(req)
    }
}
