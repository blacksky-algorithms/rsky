use crate::oauth_provider::constants::DPOP_NONCE_MAX_AGE;
use hex::ToHex;
use rand::Rng;
use ring::digest;
use ring::digest::digest;
use std::time::SystemTime;

#[derive(Clone)]
pub struct DpopNonce {
    secret: Vec<u8>,
    counter: u64,
    prev: String,
    now: String,
    next: String,
    step: u64,
}

#[derive(Clone)]
pub enum DpopNonceInput {
    String(String),
    Uint8Array(Vec<u8>),
    DpopNonce(DpopNonce),
}

impl DpopNonce {
    pub fn new(secret: Vec<u8>, step: u64) -> Self {
        if secret.len() != 32 {
            panic!("Expected 32 bytes")
        }
        if step > DPOP_NONCE_MAX_AGE / 3 {
            panic!("Invalid step")
        }

        let current_time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("timestamp in millis since UNIX epoch")
            .as_millis() as u64;
        let counter = current_time / step;
        let prev = compute(counter - 1);
        let now = compute(counter);
        let next = compute(counter + 1);
        DpopNonce {
            secret,
            counter,
            prev,
            now,
            next,
            step,
        }
    }

    pub fn next(&mut self) -> String {
        self.rotate();
        self.next.clone()
    }

    fn rotate(&mut self) {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("timestamp in micros since UNIX epoch")
            .as_millis() as u64;
        let counter = now / self.step;
        match counter - self.counter {
            0 => return,
            1 => {
                self.prev = self.now.clone();
                self.now = self.next.clone();
                self.next = compute(counter + 1);
            }
            2 => {
                self.prev = self.next.clone();
                self.now = compute(counter);
                self.next = compute(counter + 1);
            }
            _ => {
                self.prev = compute(counter - 1);
                self.now = compute(counter);
                self.next = compute(counter + 1)
            }
        }
        self.counter = counter;
    }

    pub fn check(&self, nonce: String) -> bool {
        self.next == nonce || self.now == nonce || self.prev == nonce
    }

    pub fn from(input: Option<DpopNonceInput>, _step: Option<u64>) -> DpopNonce {
        let step = _step.unwrap_or(DPOP_NONCE_MAX_AGE / 3);
        match input {
            None => {
                let random_bytes = rand::rng().random::<[u8; 32]>();
                let secret = random_bytes.to_vec();
                DpopNonce::new(secret, step)
            }
            Some(dpop_nonce_input) => match dpop_nonce_input {
                DpopNonceInput::String(res) => {
                    let secret = hex::decode(res).expect("Decoding failed");
                    DpopNonce::new(secret, step)
                }
                DpopNonceInput::Uint8Array(secret) => DpopNonce::new(secret, step),
                DpopNonceInput::DpopNonce(res) => res,
            },
        }
    }
}

fn compute(counter: u64) -> String {
    let res = digest(&digest::SHA256, &num_to_64_bits(counter));
    res.encode_hex()
}

fn num_to_64_bits(num: u64) -> [u8; 8] {
    let b1: u8 = ((num >> 56) & 0xff) as u8;
    let b2: u8 = ((num >> 48) & 0xff) as u8;
    let b3: u8 = ((num >> 40) & 0xff) as u8;
    let b4: u8 = ((num >> 32) & 0xff) as u8;
    let b5: u8 = ((num >> 24) & 0xff) as u8;
    let b6: u8 = ((num >> 16) & 0xff) as u8;
    let b7: u8 = ((num >> 8) & 0xff) as u8;
    let b8: u8 = (num & 0xff) as u8;
    [b1, b2, b3, b4, b5, b6, b7, b8]
}
