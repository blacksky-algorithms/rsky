use anyhow::{Result};
use libipld::Cid;
use diesel::prelude::*;
use serde::Deserialize as DeserializeTrait;

#[derive(Debug, Deserialize, Serialize)]
pub struct ObjAndBytes<T> {
    pub obj: T,
    pub bytes: Vec<u8>
}

pub fn get_bytes (
    conn: &mut PgConnection,
    cid: &Cid
) -> Result<Vec<u8>> {
    // TO DO: Implement caching
    use crate::schema::pds::repo_block::dsl as RepoBlockSchema;

    let result = RepoBlockSchema::repo_block
        .filter(RepoBlockSchema::cid.eq(cid.to_string()))
        .select(RepoBlockSchema::content)
        .first(conn)
        .map_err(|error| {
            let context = format!("missing block '{}'", cid.to_string());
            anyhow::Error::new(error).context(context)
        })?;
    Ok(result)
}

pub fn has (
    conn: &mut PgConnection,
    cid: Cid
) -> Result<bool> {
    let got = get_bytes(conn, &cid)?;
    Ok(!got.is_empty())
}

pub fn attempt_read<'de, T: DeserializeTrait<'de>>(
    conn: &mut PgConnection,
    cid: &Cid
) -> Result<ObjAndBytes<T>> {
    let bytes = get_bytes(conn, cid)?;
    let obj = serde_ipld_dagcbor::from_slice(bytes.as_slice())?;
    Ok(ObjAndBytes {
        obj,
        bytes
    })
}

pub fn read_obj_and_bytes<'de, T: DeserializeTrait<'de>>(
    conn: &mut PgConnection,
    cid: &Cid
) -> Result<ObjAndBytes<T>> {
    let read = attempt_read(conn, cid)?;
    Ok(read)
}

pub fn read_obj<'de, T: DeserializeTrait<'de>>(
    conn: &mut PgConnection,
    cid: &Cid
) -> Result<T> {
    let obj = read_obj_and_bytes(conn, cid)?;
    Ok(obj.obj)
}