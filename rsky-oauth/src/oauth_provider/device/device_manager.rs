use crate::oauth_provider::device::device_id::DeviceId;
use crate::oauth_provider::device::device_store::DeviceStore;

pub struct DeviceManager {
    device_store: DeviceStore,
}

/**
 * This class provides an abstraction for keeping track of DEVICE sessions. It
 * relies on a {@link DeviceStore} to persist session data and a cookie to
 * identify the session.
 */
impl DeviceManager {
    pub fn new(device_store: DeviceStore) -> Self {
        Self { device_store }
    }

    pub async fn load(&self) -> DeviceId {
        unimplemented!()
    }
}
