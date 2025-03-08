use std::collections::BTreeMap;
use std::time::SystemTime;

pub struct ReplayStoreMemory {
    last_cleanup: u64,
    nonces: BTreeMap<String, f64>,
}

impl ReplayStoreMemory {
    pub async fn unique(&mut self, namespace: &str, nonce: &str, time_frame: f64) -> bool {
        self.cleanup();
        let key = format!("{namespace}:{nonce}");
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("timestamp in micros since UNIX epoch")
            .as_micros() as f64;
        self.nonces.insert(key, now + time_frame) == None
    }

    pub fn new() -> Self {
        ReplayStoreMemory {
            last_cleanup: 0,
            nonces: Default::default(),
        }
    }

    pub fn cleanup(&mut self) {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("timestamp in micros since UNIX epoch")
            .as_micros() as u64;

        if self.last_cleanup < now - 60_000 {
            // for (key, expires) in self.nonces {
            //     if expires < now {
            //         self.nonces.remove(&key);
            //     }
            // }
            self.last_cleanup = now;
        }
    }
}
