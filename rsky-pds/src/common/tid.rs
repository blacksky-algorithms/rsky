use anyhow::{bail, Result};
use std::time::SystemTime;

const TID_LEN: usize = 13;
const S32_CHAR: &str = "234567abcdefghijklmnopqrstuvwxyz";

pub fn s32encode(mut i: usize) -> String {
    let mut s: String = "".to_owned();
    while i > 0 {
        let c = i % 32;
        i = i / 32;
        s = format!("{0}{1}", S32_CHAR.chars().nth(c).unwrap(), s);
    }
    s
}

pub fn s32decode(s: String) -> usize {
    let mut i: usize = 0;
    for c in s.chars() {
        i = i * 32 + S32_CHAR.chars().position(|x| x == c).unwrap();
    }
    i
}

pub fn dedash(str: String) -> String {
    str.replace("-", "")
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TID(pub(crate) String);

impl TID {
    pub fn new(str: String) -> Result<Self> {
        let no_dashes = dedash(str);
        if no_dashes.len() != TID_LEN {
            bail!("Poorly formatted TID: {:?} length", no_dashes.len())
        }
        Ok(TID(no_dashes))
    }

    pub fn from_time(timestamp: usize, clock_id: usize) -> Self {
        let str = format!("{0}{1:2>2}", s32encode(timestamp), s32encode(clock_id));
        TID(str)
    }

    pub fn timestamp(&self) -> usize {
        s32decode(self.0[0..11].to_owned())
    }

    pub fn clock_id(&self) -> usize {
        s32decode(self.0[11..13].to_owned())
    }

    // newer > older
    pub fn compare_to(&self, other: &TID) -> i8 {
        if self.0 > other.0 {
            return 1;
        }
        if self.0 < other.0 {
            return -1;
        }
        0
    }

    pub fn equals(&self, other: &TID) -> bool {
        self.0 == other.0
    }

    pub fn newer_than(&self, other: &TID) -> bool {
        self.compare_to(other) > 0
    }

    pub fn older_than(&self, other: &TID) -> bool {
        self.compare_to(other) < 0
    }

    pub fn to_string(self) -> String {
        self.0
    }
}

/// Based on adenosine/adenosine/src/identifiers.rs
pub struct Ticker {
    last_timestamp: usize,
    clock_id: usize,
}

impl Ticker {
    pub fn new() -> Self {
        let mut ticker = Self {
            last_timestamp: 0,
            // mask to 10 bits
            clock_id: rand::random::<usize>() & 0x03FF,
        };
        // prime the pump
        ticker.next(None);
        ticker
    }

    pub fn next(&mut self, prev: Option<TID>) -> TID {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("timestamp in micros since UNIX epoch")
            .as_micros() as usize;
        // mask to 53 bits
        let now = now & 0x001FFFFFFFFFFFFF;
        if now > self.last_timestamp {
            self.last_timestamp = now;
        } else {
            self.last_timestamp += 1;
        }
        // 53 bits of millis
        let micros = self.last_timestamp & 0x001FFFFFFFFFFFFF;
        // 10 bits of clock ID
        let clock_id = self.clock_id & 0x03FF;

        let tid = TID::from_time(micros, clock_id);
        match prev {
            Some(ref prev) if tid.newer_than(prev) => tid,
            Some(prev) => TID::from_time(prev.timestamp() + 1, clock_id),
            None => tid,
        }
    }
}

impl Default for Ticker {
    fn default() -> Self {
        Self::new()
    }
}
