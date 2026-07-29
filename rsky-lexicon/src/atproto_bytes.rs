use base64::engine::general_purpose::STANDARD_NO_PAD;
use base64::Engine;
use serde::de::Error;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serializer};

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

pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    if deserializer.is_human_readable() {
        #[derive(Deserialize)]
        struct BytesObject {
            #[serde(rename = "$bytes")]
            bytes: String,
        }
        let obj = BytesObject::deserialize(deserializer)?;
        STANDARD_NO_PAD.decode(obj.bytes).map_err(Error::custom)
    } else {
        serde_bytes::ByteBuf::deserialize(deserializer).map(serde_bytes::ByteBuf::into_vec)
    }
}

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
