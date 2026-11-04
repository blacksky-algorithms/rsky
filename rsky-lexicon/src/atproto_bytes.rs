use base64::alphabet;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig, STANDARD_NO_PAD};
use base64::engine::DecodePaddingMode;
use base64::Engine;
use serde::de::{Error, MapAccess, SeqAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserializer, Serializer};
use std::fmt;

pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    if serializer.is_human_readable() {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("$bytes", &STANDARD_NO_PAD.encode(bytes))?;
        map.end()
    } else {
        serializer.serialize_bytes(bytes)
    }
}

/// Accepts the JSON `{"$bytes": base64}` object (padding optional, as the
/// spec allows) and native CBOR bytes. The format is read from the value
/// rather than `is_human_readable`, which serde's untagged-enum buffering
/// reports as true even for CBOR.
pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(BytesVisitor)
}

struct BytesVisitor;

impl<'de> Visitor<'de> for BytesVisitor {
    type Value = Vec<u8>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("bytes or a {\"$bytes\": base64} object")
    }

    fn visit_bytes<E: Error>(self, v: &[u8]) -> Result<Vec<u8>, E> {
        Ok(v.to_vec())
    }

    fn visit_byte_buf<E: Error>(self, v: Vec<u8>) -> Result<Vec<u8>, E> {
        Ok(v)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u8>, A::Error> {
        let mut bytes = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(byte) = seq.next_element::<u8>()? {
            bytes.push(byte);
        }
        Ok(bytes)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Vec<u8>, A::Error> {
        let Some((key, encoded)) = map.next_entry::<String, String>()? else {
            return Err(Error::custom("expected a \"$bytes\" key"));
        };
        if key != "$bytes" || map.next_key::<String>()?.is_some() {
            return Err(Error::custom("expected a single \"$bytes\" key"));
        }
        BASE64_PADDING_OPTIONAL
            .decode(encoded)
            .map_err(Error::custom)
    }
}

const BASE64_PADDING_OPTIONAL: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

#[cfg(test)]
mod tests {
    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Wrapper {
        #[serde(with = "crate::atproto_bytes")]
        data: Vec<u8>,
    }

    #[test]
    fn json_serializes_to_bytes_object() {
        let wrapper = Wrapper {
            data: vec![1, 2, 3],
        };
        let json = serde_json::to_string(&wrapper).unwrap();
        assert_eq!(json, r#"{"data":{"$bytes":"AQID"}}"#);
    }

    #[test]
    fn json_deserializes_from_bytes_object() {
        let wrapper: Wrapper = serde_json::from_str(r#"{"data":{"$bytes":"AQID"}}"#).unwrap();
        assert_eq!(wrapper.data, vec![1, 2, 3]);
    }

    #[test]
    fn json_rejects_invalid_base64() {
        let result = serde_json::from_str::<Wrapper>(r#"{"data":{"$bytes":"!!!"}}"#);
        assert!(result.is_err());
    }

    #[test]
    fn json_deserializes_padded_base64() {
        let wrapper: Wrapper = serde_json::from_str(r#"{"data":{"$bytes":"AQIDBA=="}}"#).unwrap();
        assert_eq!(wrapper.data, vec![1, 2, 3, 4]);
    }

    #[test]
    fn json_rejects_bytes_object_with_other_keys() {
        assert!(serde_json::from_str::<Wrapper>(r#"{"data":{"$bytes":"AQID","x":1}}"#).is_err());
        assert!(serde_json::from_str::<Wrapper>(r#"{"data":{"$link":"AQID"}}"#).is_err());
        assert!(serde_json::from_str::<Wrapper>(r#"{"data":{}}"#).is_err());
    }

    #[test]
    fn cbor_round_trips_as_native_bytes() {
        let wrapper = Wrapper {
            data: vec![1, 2, 3],
        };
        let cbor = serde_ipld_dagcbor::to_vec(&wrapper).unwrap();
        // major type 2 (byte string) of length 3
        assert!(cbor.ends_with(&[0x43, 1, 2, 3]));
        let back: Wrapper = serde_ipld_dagcbor::from_slice(&cbor).unwrap();
        assert_eq!(back, wrapper);
    }

    #[test]
    fn json_rejects_non_object_bytes() {
        assert!(serde_json::from_str::<Wrapper>(r#"{"data":42}"#).is_err());
    }

    #[test]
    fn json_deserializes_from_seq() {
        let wrapper: Wrapper = serde_json::from_str(r#"[{"$bytes":"AQID"}]"#).unwrap();
        assert_eq!(
            wrapper,
            Wrapper {
                data: vec![1, 2, 3],
            }
        );
    }

    #[test]
    fn serialize_surfaces_writer_errors() {
        struct FailWriter {
            remaining: usize,
        }
        impl std::io::Write for FailWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                if self.remaining >= buf.len() {
                    self.remaining -= buf.len();
                    Ok(buf.len())
                } else {
                    Err(std::io::Error::other("full"))
                }
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let wrapper = Wrapper {
            data: vec![1, 2, 3],
        };
        for limit in [8usize, 12] {
            assert!(serde_json::to_writer(FailWriter { remaining: limit }, &wrapper).is_err());
        }
        std::io::Write::flush(&mut FailWriter { remaining: 0 }).unwrap();
    }

    #[test]
    fn cbor_deserializes_from_seq() {
        let expected = Wrapper {
            data: vec![1, 2, 3],
        };
        let definite = [0x81, 0x43, 0x01, 0x02, 0x03];
        assert_eq!(
            serde_cbor::from_slice::<Wrapper>(&definite).unwrap(),
            expected
        );
        let indefinite = [0x9f, 0x43, 0x01, 0x02, 0x03, 0xff];
        assert_eq!(
            serde_cbor::from_slice::<Wrapper>(&indefinite).unwrap(),
            expected
        );
    }

    #[test]
    fn cbor_roundtrips_as_raw_bytes() {
        let wrapper = Wrapper {
            data: vec![7, 8, 9],
        };
        let bytes = serde_cbor::to_vec(&wrapper).unwrap();
        assert_eq!(serde_cbor::from_slice::<Wrapper>(&bytes).unwrap(), wrapper);
    }
}
