//! CIDs (Content IDs) are identifiers used for addressing resources by their contents, essentially a hash with limited metadata.
//!
//! [Spec](https://dasl.ing/cid.html)
//!
//! VENDORED from dasl::cid until they make a release with `Hash` derived on Cid
//!
//! this file: Copyright 2025 N0, INC.
//! Apache2.0 / MIT: https://github.com/n0-computer/dasl#license

use std::{fmt::Display, str::FromStr};

use sha2::Digest;
use thiserror::Error;

// pub(crate) use self::serde::{BytesToCidVisitor, CID_SERDE_PRIVATE_IDENTIFIER};

const BASE32_LOWER: data_encoding::Encoding = data_encoding_macro::new_encoding! {
    symbols: "abcdefghijklmnopqrstuvwxyz234567",
};

const CID_VERSION: u8 = 1;
const PREFIX_LEN: usize = 4;
/// Length of a known hash
const HASH_LEN: u8 = 32;
const DATA_LEN: usize = PREFIX_LEN + HASH_LEN as usize;
const HASH_CODE_SHA2_256: u8 = 0x12;
const HASH_CODE_BLAKE3: u8 = 0x1e;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Hash)]
pub struct Cid {
    // - 1 byte CID version
    // - 1 byte Codec
    // - 1 byte hash type
    // - 1 byte Length
    // - 32 bytes hash
    data: [u8; DATA_LEN],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd)]
#[non_exhaustive]
#[repr(u8)]
pub enum Codec {
    Raw = 0x55,
    Drisl = 0x71,
}

#[derive(Debug, Error)]
pub enum ParseCodecError {
    #[error("Unknown codec: 0x{_0:X}")]
    UnknownCodec(u8),
}

impl TryFrom<u8> for Codec {
    type Error = ParseCodecError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x55 => Ok(Self::Raw),
            0x71 => Ok(Self::Drisl),
            _ => Err(ParseCodecError::UnknownCodec(value)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd)]
#[non_exhaustive]
#[repr(u8)]
pub enum Multihash {
    Sha2256 = 0x12,
    Blake3 = 0x1e,
}

impl TryFrom<u8> for Multihash {
    type Error = MultihashParseError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            HASH_CODE_SHA2_256 => Ok(Self::Sha2256),
            HASH_CODE_BLAKE3 => Ok(Self::Blake3),
            _ => Err(MultihashParseError::UnknownHash(value)),
        }
    }
}

#[derive(Debug, Error)]
pub enum CidParseError {
    #[error("Invalid encoding")]
    InvalidEncoding,
    #[error("Too short")]
    TooShort,
    #[error("Invalid CID version: {_0}")]
    InvalidCidVersion(u8),
    #[error("Invalid codec: {_0}")]
    InvalidCodec(ParseCodecError),
    #[error("Invalid multihash: {_0}")]
    InvalidMultihash(MultihashParseError),
}

impl From<ParseCodecError> for CidParseError {
    fn from(err: ParseCodecError) -> Self {
        Self::InvalidCodec(err)
    }
}

impl From<MultihashParseError> for CidParseError {
    fn from(err: MultihashParseError) -> Self {
        Self::InvalidMultihash(err)
    }
}

impl FromStr for Cid {
    type Err = CidParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !s.starts_with('b') {
            return Err(CidParseError::InvalidEncoding);
        }

        // skip base encoding prefix
        let without_prefix = &s.as_bytes()[1..];
        let bytes = BASE32_LOWER
            .decode(without_prefix)
            .map_err(|_e| CidParseError::InvalidEncoding)?;

        Cid::from_bytes_raw(&bytes)
    }
}

impl Cid {
    /// Returns the `Multihash` of this `CID`.
    pub fn hash(&self) -> &[u8] {
        match self.data[3] {
            0 => &[][..], // empty hash
            HASH_LEN => &self.data[PREFIX_LEN..],
            _ => unreachable!("invalid construction"),
        }
    }

    pub fn multihash_type(&self) -> Multihash {
        Multihash::try_from(self.data[2]).expect("invalid construction")
    }

    /// Returns the `Codec` of this `CID`.
    pub fn codec(&self) -> Codec {
        Codec::try_from(self.data[1]).expect("invalid construction")
    }

    /// Tries to decode a `CID` from binary encoding.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CidParseError> {
        if bytes.is_empty() {
            return Err(CidParseError::TooShort);
        }
        if bytes[0] != 0x0 {
            return Err(CidParseError::InvalidEncoding);
        }
        Self::from_bytes_raw(&bytes[1..])
    }

    /// Tries to decode a `CID` from its raw binary components.
    pub fn from_bytes_raw(bytes: &[u8]) -> Result<Self, CidParseError> {
        const MIN_LEN: usize = 3;

        if bytes.len() < MIN_LEN {
            return Err(CidParseError::TooShort);
        }
        if bytes.len() > DATA_LEN {
            return Err(MultihashParseError::InvalidLength(bytes.len()).into());
        }

        if bytes[0] != CID_VERSION {
            return Err(CidParseError::InvalidCidVersion(bytes[0]));
        }
        let mut data = [0u8; DATA_LEN];
        let _codec = Codec::try_from(bytes[1])?;
        let _multihash = Multihash::try_from(bytes[2])?;

        let len = bytes[3];
        match len {
            0 => {
                if bytes.len() > 4 {
                    return Err(MultihashParseError::InvalidLength(bytes.len()).into());
                }
                data[..PREFIX_LEN].copy_from_slice(&bytes[..PREFIX_LEN]);
            }
            HASH_LEN => {
                if bytes.len() != DATA_LEN {
                    return Err(MultihashParseError::InvalidLength(bytes.len()).into());
                }
                data.copy_from_slice(bytes);
            }
            _ => return Err(MultihashParseError::InvalidLengthPrefix.into()),
        }

        Ok(Cid { data })
    }

    /// Encode the `CID` in its raw binary format.
    pub fn as_bytes(&self) -> &[u8] {
        match self.data[3] {
            0 => &self.data[..PREFIX_LEN],
            HASH_LEN => &self.data,
            _ => unreachable!("invalid construction"),
        }
    }

    pub fn digest_sha2(codec: Codec, data: impl AsRef<[u8]>) -> Self {
        let hash = sha2::Sha256::digest(data);
        let mut data = [0u8; DATA_LEN];
        data[0] = CID_VERSION;
        data[1] = codec as u8;
        data[2] = HASH_CODE_SHA2_256;
        data[3] = HASH_LEN;
        data[PREFIX_LEN..].copy_from_slice(&hash);
        Self { data }
    }

    // pub fn digest_blake3(codec: Codec, data: impl AsRef<[u8]>) -> Self {
    //     let hash = blake3::hash(data.as_ref());
    //     let mut data = [0u8; DATA_LEN];
    //     data[0] = CID_VERSION;
    //     data[1] = codec as u8;
    //     data[2] = HASH_CODE_BLAKE3;
    //     data[3] = HASH_LEN;
    //     data[PREFIX_LEN..].copy_from_slice(hash.as_bytes());
    //     Self { data }
    // }

    pub fn empty_sha2_256(codec: Codec) -> Self {
        let mut data = [0u8; DATA_LEN];
        data[0] = CID_VERSION;
        data[1] = codec as u8;
        data[2] = HASH_CODE_SHA2_256;
        data[3] = 0;
        Self { data }
    }

    pub fn empty_blake3(codec: Codec) -> Self {
        let mut data = [0u8; DATA_LEN];
        data[0] = CID_VERSION;
        data[1] = codec as u8;
        data[2] = HASH_CODE_BLAKE3;
        data[3] = 0;
        Self { data }
    }
}

impl Display for Cid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "b")?;
        let out = self.as_bytes();
        BASE32_LOWER.encode_write(out, f)?;

        Ok(())
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MultihashParseError {
    #[error("Invalid length: {_0}")]
    InvalidLength(usize),
    #[error("Unknown hash: {_0:x}")]
    UnknownHash(u8),
    #[error("Invalid length prefix")]
    InvalidLengthPrefix,
}

mod serde {
    //! CID Serde (de)serialization
    //!
    //! CIDs cannot directly be represented in any of the native Serde Data model types. In order to
    //! work around that limitation. a newtype struct is introduced, that is used as a marker for Serde
    //! (de)serialization.
    //!
    //! Based on <https://github.com/multiformats/rust-cid/blob/master/src/serde.rs>

    use core::fmt;
    use std::{format, vec::Vec};

    use serde::{de, ser};
    use serde_bytes::ByteBuf;

    use super::Cid;

    /// An identifier that is used internally by Serde implementations that support [`Cid`]s.
    // TODO: should this be different than the one in `rust-cid`?
    pub const CID_SERDE_PRIVATE_IDENTIFIER: &str = "$__private__serde__identifier__for__cid";

    /// Serialize a CID into the Serde data model as enum.
    ///
    /// Custom types are not supported by Serde, hence we map a CID into an enum that can be identified
    /// as a CID by implementations that support CIDs. The corresponding Rust type would be:
    ///
    /// ```text
    /// struct $__private__serde__identifier__for__cid(serde_bytes::BytesBuf);
    /// ```
    impl ser::Serialize for Cid {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: ser::Serializer,
        {
            // Prefix 0x00
            let raw = self.as_bytes();
            let mut bytes = vec![0u8; 1 + raw.len()];
            bytes[1..].copy_from_slice(raw);
            let value = ByteBuf::from(bytes);
            serializer.serialize_newtype_struct(CID_SERDE_PRIVATE_IDENTIFIER, &value)
        }
    }

    /// Visitor to transform bytes into a CID.
    pub struct BytesToCidVisitor;

    impl<'de> de::Visitor<'de> for BytesToCidVisitor {
        type Value = Cid;

        fn expecting(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
            write!(fmt, "a valid CID in bytes")
        }

        fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Cid::from_bytes_raw(value)
                .map_err(|err| de::Error::custom(format!("Failed to deserialize CID: {err}")))
        }

        /// Some Serde data formats interpret a byte stream as a sequence of bytes (e.g. `serde_json`).
        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut bytes = Vec::new();
            while let Some(byte) = seq.next_element()? {
                bytes.push(byte);
            }
            Cid::from_bytes_raw(&bytes)
                .map_err(|err| de::Error::custom(format!("Failed to deserialize CID: {err}")))
        }
    }

    /// Deserialize a CID into a newtype struct.
    ///
    /// Deserialize a CID that was serialized as a newtype struct, so that can be identified as a CID.
    /// Its corresponding Rust type would be:
    ///
    /// ```text
    /// struct $__private__serde__identifier__for__cid(serde_bytes::BytesBuf);
    /// ```
    impl<'de> de::Deserialize<'de> for Cid {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: de::Deserializer<'de>,
        {
            /// Main visitor to deserialize a CID.
            ///
            /// This visitor has only a single entry point to deserialize CIDs, it's
            /// `visit_new_type_struct()`. This ensures that it isn't accidentally used to decode CIDs
            /// to bytes.
            struct MainEntryVisitor;

            impl<'de> de::Visitor<'de> for MainEntryVisitor {
                type Value = Cid;

                fn expecting(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
                    write!(fmt, "a valid CID in bytes, wrapped in an newtype struct")
                }

                fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
                where
                    D: de::Deserializer<'de>,
                {
                    deserializer.deserialize_bytes(BytesToCidVisitor)
                }
            }

            deserializer.deserialize_newtype_struct(CID_SERDE_PRIVATE_IDENTIFIER, MainEntryVisitor)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_sha2_256() {
        // Sha2 256: "foo"
        let cid_str = "bafkreibme22gw2h7y2h7tg2fhqotaqjucnbc24deqo72b6mkl2egezxhvy";
        let parsed: Cid = cid_str.parse().unwrap();
        assert_eq!(parsed.codec(), Codec::Raw);
        assert!(matches!(parsed.multihash_type(), Multihash::Sha2256));

        let cid_str_back = parsed.to_string();
        assert_eq!(cid_str_back, cid_str);
    }

    #[test]
    fn test_base_blake3() {
        // Blake3: "foo"
        let cid_str = "bafkr4iae4c5tt4yldi76xcpvg3etxykqkvec352im5fqbutolj2xo5yc5e";
        let parsed: Cid = cid_str.parse().unwrap();
        assert_eq!(parsed.codec(), Codec::Raw);
        assert!(matches!(parsed.multihash_type(), Multihash::Blake3));

        let cid_str_back = parsed.to_string();
        assert_eq!(cid_str_back, cid_str);
    }

    #[test]
    fn test_digest_sha2_256() {
        let cid_str = "bafkreibme22gw2h7y2h7tg2fhqotaqjucnbc24deqo72b6mkl2egezxhvy";
        assert_eq!(Cid::digest_sha2(Codec::Raw, b"foo").to_string(), cid_str);
    }

    // #[test]
    // fn test_digest_blake3() {
    //     let cid_str = "bafkr4iae4c5tt4yldi76xcpvg3etxykqkvec352im5fqbutolj2xo5yc5e";
    //     assert_eq!(Cid::digest_blake3(Codec::Raw, b"foo").to_string(), cid_str);
    // }
}
