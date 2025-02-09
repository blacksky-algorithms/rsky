#![allow(unused)]
use lexicon_cid::Cid;

use super::error::Error;

/// A car header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CarHeader {
    V1(CarHeaderV1),
}

impl CarHeader {
    pub fn new_v1(roots: Vec<Cid>) -> Self {
        Self::V1(roots.into())
    }

    pub fn decode(buffer: &[u8]) -> Result<Self, Error> {
        let header: CarHeaderV1 =
            serde_ipld_dagcbor::from_slice(buffer).map_err(|e| Error::Parsing(e.to_string()))?;

        if header.roots.is_empty() {
            return Err(Error::Parsing("empty CAR file".to_owned()));
        }

        if header.version != 1 {
            return Err(Error::InvalidFile(
                "Only CAR file version 1 is supported".to_string(),
            ));
        }

        Ok(CarHeader::V1(header))
    }

    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        match self {
            CarHeader::V1(ref header) => {
                let res = serde_ipld_dagcbor::to_vec(header).map_err(|e| Error::Cbor(e))?;
                Ok(res)
            }
        }
    }

    pub fn roots(&self) -> &[Cid] {
        match self {
            CarHeader::V1(header) => &header.roots,
        }
    }

    pub fn version(&self) -> u64 {
        match self {
            CarHeader::V1(_) => 1,
        }
    }
}

/// CAR file header version 1.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarHeaderV1 {
    pub roots: Vec<Cid>,
    pub version: u64,
}

impl CarHeaderV1 {
    /// Creates a new CAR file header
    pub fn new(roots: Vec<Cid>, version: u64) -> Self {
        Self { roots, version }
    }
}

impl From<Vec<Cid>> for CarHeaderV1 {
    fn from(roots: Vec<Cid>) -> Self {
        Self { roots, version: 1 }
    }
}

#[cfg(test)]
mod tests {
    use libipld::cbor::DagCborCodec;
    use libipld::codec::{Decode, Encode};
    use libipld::multihash::{Code, MultihashDigest};

    use super::*;

    #[test]
    fn symmetric_header_v1() {
        let digest = Code::Blake2b256.digest(b"test");
        let cid = Cid::new_v1(DagCborCodec.into(), digest);

        let header = CarHeaderV1::from(vec![cid]);

        let mut bytes = serde_ipld_dagcbor::to_vec(&header).unwrap();

        assert_eq!(
            serde_ipld_dagcbor::from_slice::<CarHeaderV1>(bytes.as_slice()).unwrap(),
            header
        );
    }
}
