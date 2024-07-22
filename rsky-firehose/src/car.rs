use libipld::cid::Cid;
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{Cursor, Read};

#[derive(Debug)]
pub enum Error {
    UvarintEof,
    UvarintBad,
    ChunkEof,
    HeaderCbor,
    BlockCid,
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

#[derive(Debug, Deserialize)]
pub struct Header {
    pub version: u8,
    pub roots: Vec<Cid>,
}

pub fn read_header<T: Read>(reader: &mut T) -> Result<Header, Error> {
    let mut reader = Cursor::new(read_chunk(reader)?);
    let header =
        serde_ipld_dagcbor::from_reader::<Header, _>(&mut reader).map_err(|_| Error::HeaderCbor)?;
    Ok(header)
}

pub fn read_blocks<T: Read>(mut reader: &mut T) -> Result<HashMap<Cid, Vec<u8>>, Error> {
    let mut blocks = HashMap::new();

    while let Ok(chunk) = read_chunk(&mut reader) {
        let mut block_reader = Cursor::new(chunk);

        let cid = Cid::read_bytes(&mut block_reader).map_err(|_| Error::BlockCid)?;
        let mut block = Vec::new();
        block_reader
            .read_to_end(&mut block)
            .map_err(|_| Error::BlockData)?;
        blocks.insert(cid, block);
    }

    Ok(blocks)
}
