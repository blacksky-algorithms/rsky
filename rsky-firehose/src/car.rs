use libipld::cid::Cid;
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{Cursor, Read};

#[derive(Debug, PartialEq)]
pub enum Error {
    UvarintEof,
    UvarintBad,
    ChunkEof,
    HeaderCbor(String),
    BlockCid(String),
    BlockData,
}

fn read_uvarint64<T: Read>(reader: &mut T) -> Result<u64, Error> {
    let mut out = 0;
    let mut buf = [0; 1];

    let mut i = 0;
    while let Ok(_) = reader.read_exact(&mut buf) {
        let b = buf[0] as u64;
        let k = b & 0x7F;
        out |= k << (i * 7);

        if b & 0x80 != 0 {
            // not final byte
        } else if b == 0 && i > 0 {
            // invalid data; "more minimally"
            return Err(Error::UvarintBad);
        } else {
            return Ok(out);
        }

        i += 1;
    }

    Err(Error::UvarintEof)
}

fn read_chunk<T: Read>(reader: &mut T) -> Result<Vec<u8>, Error> {
    let chunk_size = read_uvarint64(reader)? as usize;
    let mut buf = vec![0; chunk_size];
    reader.read_exact(&mut buf).map_err(|_| Error::ChunkEof)?;
    Ok(buf)
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Header {
    pub version: u8,
    pub roots: Vec<Cid>,
}

pub fn read_header<T: Read>(reader: &mut T) -> Result<Header, Error> {
    let mut reader = Cursor::new(read_chunk(reader)?);
    let header = serde_ipld_dagcbor::from_reader::<Header, _>(&mut reader)
        .map_err(|e| Error::HeaderCbor(e.to_string()))?;
    Ok(header)
}

pub fn read_blocks<T: Read>(mut reader: &mut T) -> Result<HashMap<Cid, Vec<u8>>, Error> {
    let mut blocks = HashMap::new();

    while let Ok(chunk) = read_chunk(&mut reader) {
        let mut block_reader = Cursor::new(chunk);

        let cid = Cid::read_bytes(&mut block_reader).map_err(|e| Error::BlockCid(e.to_string()))?;
        let mut block = Vec::new();
        block_reader
            .read_to_end(&mut block)
            .map_err(|_| Error::BlockData)?;
        blocks.insert(cid, block);
    }

    Ok(blocks)
}

#[cfg(test)]
mod tests {

    use libipld::Multihash;

    use super::*;

    #[test]
    fn test_read_uvarint() {
        let mut reader = Cursor::new(vec![0x01, 0x02, 0x03]);
        assert_eq!(read_uvarint64(&mut reader).unwrap(), 1);
        assert_eq!(read_uvarint64(&mut reader).unwrap(), 2);
        assert_eq!(read_uvarint64(&mut reader).unwrap(), 3);
        assert_eq!(read_uvarint64(&mut reader).unwrap_err(), Error::UvarintEof);
    }

    #[test]
    fn test_read_chunk() {
        let mut reader = Cursor::new(vec![0x01, 0x02, 0x01, 0x03, 0x01]);
        assert_eq!(read_chunk(&mut reader).unwrap(), vec![0x02]);
        assert_eq!(read_chunk(&mut reader).unwrap(), vec![0x03]);
        assert_eq!(read_chunk(&mut reader), Err(Error::ChunkEof));

        reader = Cursor::new(vec![0x02, 0x02, 0x03]);
        assert_eq!(read_chunk(&mut reader).unwrap(), vec![0x02, 0x03]);
        assert_eq!(read_chunk(&mut reader), Err(Error::UvarintEof));

        reader = Cursor::new(vec![0x03, 0x02, 0x03]);
        assert_eq!(read_chunk(&mut reader), Err(Error::ChunkEof));
    }

    #[test]
    fn test_read_header() {
        let header = Header {
            version: 1,
            roots: vec![Cid::new(
                libipld::cid::Version::V1,
                1,
                Multihash::from_bytes(&[0x01, 0x03, 0x01, 0x02, 0x03]).unwrap(),
            )
            .unwrap()],
        };
        let serialized = serde_ipld_dagcbor::to_vec(&header).unwrap();
        let mut buffer = vec![serialized.len() as u8];
        buffer.extend(serialized);
        let mut reader = Cursor::new(buffer);
        assert_eq!(read_header(&mut reader).unwrap(), header);
    }

    #[test]
    fn test_read_blocks() {
        let block = vec![0x04, 0x04, 0x04, 0x04];

        let bytes = vec![0x01, 0x01, 0x02, 0x01, 0x01];
        let bytes_reader = Cursor::new(&bytes);
        let hash = Multihash::read(bytes_reader).unwrap();
        let cid = Cid::new(libipld::cid::Version::V1, 1, hash).unwrap();

        let chunk_len = bytes.len() + block.len();

        let mut buffer = vec![chunk_len as u8];
        buffer.extend(cid.to_bytes());
        buffer.extend(&block);

        let mut reader = Cursor::new(buffer);
        assert_eq!(
            read_blocks(&mut reader).unwrap(),
            HashMap::from([(cid, block)])
        );
    }
}
