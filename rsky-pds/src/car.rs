use crate::repo::block_map::BlockMap;
use crate::repo::types::CidAndBytes;
use crate::vendored::iroh_car::{CarHeader, CarWriter};
use anyhow::Result;
use lexicon_cid::Cid;

pub async fn read_car_bytes(root: Option<&Cid>, blocks: BlockMap) -> Result<Vec<u8>> {
    let roots = match root {
        Some(root) => vec![*root],
        None => vec![],
    };
    let car_header = CarHeader::new_v1(roots);
    let buf: Vec<u8> = Default::default();
    let mut car_writer = CarWriter::new(car_header, buf);

    for CidAndBytes { cid, bytes } in blocks.entries()? {
        car_writer.write(cid, bytes).await?;
    }
    Ok(car_writer.finish().await?)
}
