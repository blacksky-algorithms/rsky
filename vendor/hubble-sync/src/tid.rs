//! u64 wrapper for atproto Tid types
//!
//! always holds the 8-byte uint version, string format rendered as needed

use std::fmt;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const ALPHABET: &[u8; 32] = b"234567abcdefghijklmnopqrstuvwxyz";

#[derive(Debug, thiserror::Error)]
pub enum TidParseError {
    #[error("wrong length: got {got}, expected 13")]
    WrongLength { got: usize },
    #[error("invalid character {ch:?} at position {pos}")]
    InvalidChar { pos: usize, ch: char },
    #[error("high bit set (value out of range): leading char: {ch:?}")]
    HighBitSet { ch: char },
    #[error("high bit set (value out of range): {value:#x}")]
    OutOfRange { value: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tid(u64);

impl Tid {
    fn from_parts(us: u64, clock: u16) -> Self {
        assert!(clock <= 0x3FF); // lower 10 bits max
        let mut dt = us << 10;
        dt |= clock as u64;
        Self(dt)
    }
    fn into_parts(self) -> (u64, u16) {
        let clock = self.0 & 0x3FF; // 10 bits mask
        let us = self.0 >> 10;
        (us, clock as u16)
    }
    pub fn to_raw_bytes(self) -> [u8; 8] {
        self.0.to_be_bytes()
    }
    pub fn from_raw_bytes(b: [u8; 8]) -> Result<Self, TidParseError> {
        u64::from_be_bytes(b).try_into()
    }
}

impl fmt::Display for Tid {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut out = [0u8; 13];
        for (i, o) in out.iter_mut().enumerate() {
            let shift = 60 - 5 * i;
            let a = ((self.0 >> shift) & 0x1F) as usize;
            *o = ALPHABET[a];
        }
        f.write_str(str::from_utf8(&out).expect("alphabet is ascii"))
    }
}

impl FromStr for Tid {
    type Err = TidParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = s.as_bytes();
        if bytes.len() != 13 {
            return Err(TidParseError::WrongLength { got: bytes.len() });
        }
        let mut val: u64 = 0;
        for (pos, &b) in bytes.iter().enumerate() {
            let v = match b {
                b'2'..=b'7' => b - b'2',
                b'a'..=b'z' => b - b'a' + 6,
                _ => return Err(TidParseError::InvalidChar { pos, ch: b as char }),
            };
            if pos == 0 && v > 7 {
                return Err(TidParseError::HighBitSet { ch: b as char });
            }
            val = (val << 5) | v as u64;
        }
        Ok(Self(val))
    }
}

impl TryFrom<u64> for Tid {
    type Error = TidParseError;
    fn try_from(td: u64) -> Result<Self, Self::Error> {
        if td.leading_zeros() == 0 {
            return Err(TidParseError::OutOfRange { value: td });
        }
        Ok(Self(td))
    }
}

impl From<(SystemTime, u16)> for Tid {
    fn from((st, clock): (SystemTime, u16)) -> Tid {
        let us: u64 = st
            .duration_since(UNIX_EPOCH)
            .expect("time after epoch")
            .as_micros()
            .try_into()
            .expect("time before u64 micros exhaustion");
        assert!(us.leading_zeros() > 0, "high bit set");
        Self::from_parts(us, clock)
    }
}

impl From<Tid> for (SystemTime, u16) {
    fn from(td: Tid) -> (SystemTime, u16) {
        let (us, clock) = td.into_parts();
        let dt = UNIX_EPOCH + Duration::from_micros(us);
        (dt, clock)
    }
}

impl From<jacquard_common::types::tid::Tid> for Tid {
    fn from(jt: jacquard_common::types::tid::Tid) -> Self {
        // jacquard doesn't currently expose the clock id directly, so, do a
        // round-trip through string for now (gross, oh well)
        jt.as_str().parse().expect("jacquard tid to be a valid tid")
    }
}

impl From<Tid> for jacquard_common::types::tid::Tid {
    fn from(td: Tid) -> jacquard_common::types::tid::Tid {
        let (us, clock) = td.into_parts();
        jacquard_common::types::tid::Tid::from_time(us, clock as u32)
    }
}

/// Tid internal serialization helper (stores raw u64 instead of string form)
pub mod tid_raw_u64 {
    use super::Tid;
    use serde::{Deserialize, Deserializer, Serializer, de};
    pub fn serialize<S: Serializer>(t: &Tid, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_u64(t.0)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Tid, D::Error> {
        u64::deserialize(de)?
            .try_into()
            .map_err(|_| de::Error::custom("corrupted Tid"))
    }
}

/// Tid internal serialization helper (stores optional raw u64 instead of string form)
pub mod tid_raw_u64_opt {
    use super::Tid;
    use serde::{Deserialize, Deserializer, Serializer, de};
    pub fn serialize<S: Serializer>(t: &Option<Tid>, ser: S) -> Result<S::Ok, S::Error> {
        match t {
            Some(tid) => ser.serialize_some(&tid.0),
            None => ser.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Option<Tid>, D::Error> {
        Option::<u64>::deserialize(de)?
            .map(TryInto::try_into)
            .transpose()
            .map_err(|_| de::Error::custom("corrupted Tid"))
    }
}

/// Tid atproto-compliant serialization helper (string form)
pub mod tid_atproto {
    use super::Tid;
    use serde::{Deserialize, Deserializer, Serializer, de};
    use std::borrow::Cow;
    pub fn serialize<S: Serializer>(t: &Tid, ser: S) -> Result<S::Ok, S::Error> {
        ser.collect_str(t)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Tid, D::Error> {
        Cow::<str>::deserialize(de)?
            .parse()
            .map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_renders_as_thirteen_twos() {
        assert_eq!(Tid(0).to_string(), "2222222222222");
    }

    #[test]
    fn round_trip_renders_back_to_input() {
        // valid-shape examples (first char in 234567ab)
        for s in [
            "2222222222222",
            "3jzfcijpj2z2a",
            "7zzzzzzzzzzzz",
            "baaaaaaaaaaaa",
        ] {
            let tid: Tid = s.parse().expect("parse");
            assert_eq!(tid.to_string(), s, "round-trip {s}");
        }
    }

    #[test]
    fn wrong_length_rejected() {
        assert!(matches!(
            "abc".parse::<Tid>(),
            Err(TidParseError::WrongLength { got: 3 })
        ));
        assert!(matches!(
            "234567abcdefgh".parse::<Tid>(),
            Err(TidParseError::WrongLength { got: 14 })
        ));
    }

    #[test]
    fn invalid_chars_rejected() {
        // '0','1','8','9' aren't in the alphabet; uppercase isn't either
        for bad in ["0aaaaaaaaaaaa", "aaaaaaaaaaaa9", "ABCDEFGHIJKLM"] {
            assert!(matches!(
                bad.parse::<Tid>(),
                Err(TidParseError::InvalidChar { .. })
            ));
        }
    }

    #[test]
    fn high_bit_set_rejected_at_parse() {
        // first char 'c' (alphabet pos 8) → spec bit 63 set, fits in u64
        // but spec-reserved
        assert!(matches!(
            "caaaaaaaaaaaa".parse::<Tid>(),
            Err(TidParseError::HighBitSet { ch: 'c' })
        ));
        // first char 'k' (alphabet pos 16) → would overflow u64
        assert!(matches!(
            "kaaaaaaaaaaaa".parse::<Tid>(),
            Err(TidParseError::HighBitSet { ch: 'k' })
        ));
        // 'z' (alphabet pos 31, max) → likewise
        assert!(matches!(
            "zzzzzzzzzzzzz".parse::<Tid>(),
            Err(TidParseError::HighBitSet { ch: 'z' })
        ));
    }

    #[test]
    fn all_valid_first_chars_accepted() {
        for first in ['2', '3', '4', '5', '6', '7', 'a', 'b'] {
            let s: String = std::iter::once(first)
                .chain("aaaaaaaaaaaa".chars())
                .collect();
            assert!(s.parse::<Tid>().is_ok(), "{first:?} should parse");
        }
    }

    #[test]
    fn out_of_range_u64_rejected() {
        assert!(matches!(
            Tid::try_from(1u64 << 63),
            Err(TidParseError::OutOfRange { .. })
        ));
        // u64::MAX likewise
        assert!(matches!(
            Tid::try_from(u64::MAX),
            Err(TidParseError::OutOfRange { .. })
        ));
    }

    #[test]
    fn in_range_u64_round_trips_through_parts() {
        let inner: u64 = (1_700_000_000_000_000u64 << 10) | 42;
        let tid = Tid::try_from(inner).expect("in-range");
        let (us, clock) = tid.into_parts();
        assert_eq!(us, 1_700_000_000_000_000);
        assert_eq!(clock, 42);
    }

    #[test]
    fn lexicographic_string_order_matches_numeric_order() {
        // the whole point of the sortable alphabet
        let a = Tid(1_000_000);
        let b = Tid(2_000_000);
        let c = Tid((1u64 << 62) | 12345);
        assert!(a < b && b < c);
        let (sa, sb, sc) = (a.to_string(), b.to_string(), c.to_string());
        assert!(sa < sb && sb < sc);
    }

    #[test]
    fn parts_round_trip() {
        let tid = Tid::from_parts(1_234_567, 1023); // 1023 is max valid clock
        let (us, clock) = tid.into_parts();
        assert_eq!(us, 1_234_567);
        assert_eq!(clock, 1023);
    }

    #[test]
    fn systemtime_round_trip() {
        let st = UNIX_EPOCH + Duration::from_micros(1_700_000_000_000_000);
        let tid: Tid = (st, 123).into();
        let (got_st, got_clock) = tid.into();
        assert_eq!(got_st, st);
        assert_eq!(got_clock, 123);
    }

    #[test]
    fn serde_with_tid_raw_u64_round_trip_and_validation() {
        // transparent so the wire form is just the inner u64 — lets us
        // hand-craft a bad value as a bare u64 and deserialize it through
        // the wrapper to exercise the validation path.
        #[derive(serde::Serialize, serde::Deserialize)]
        #[serde(transparent)]
        struct Wrap(#[serde(with = "super::tid_raw_u64")] Tid);

        let good = Wrap(Tid::from_parts(1_700_000_000_000_000, 42));
        let bytes = dasl::drisl::to_vec(&good).expect("ser ok");
        let back: Wrap = dasl::drisl::from_slice(&bytes).expect("de ok");
        assert_eq!(back.0, good.0);

        let bad: u64 = 1u64 << 63;
        let bad_bytes = dasl::drisl::to_vec(&bad).expect("ser raw u64 ok");
        assert!(
            dasl::drisl::from_slice::<Wrap>(&bad_bytes).is_err(),
            "high-bit-set u64 must not deserialize into Tid"
        );
    }

    #[test]
    fn serde_with_tid_atproto_round_trip_and_validation() {
        // transparent so the wire form is just the inner u64 — lets us
        // hand-craft a bad value as a bare u64 and deserialize it through
        // the wrapper to exercise the validation path.
        #[derive(serde::Serialize, serde::Deserialize)]
        #[serde(transparent)]
        struct Wrap(#[serde(with = "super::tid_atproto")] Tid);

        let good = Wrap(Tid::from_parts(1_700_000_000_000_000, 42));
        let bytes = dasl::drisl::to_vec(&good).expect("ser ok");
        let back: Wrap = dasl::drisl::from_slice(&bytes).expect("de ok");
        assert_eq!(back.0, good.0);

        let bad: u64 = 1u64 << 63;
        let bad_bytes = dasl::drisl::to_vec(&bad).expect("ser atproto ok");
        assert!(
            dasl::drisl::from_slice::<Wrap>(&bad_bytes).is_err(),
            "high-bit-set u64 must not deserialize into Tid"
        );
    }
}
