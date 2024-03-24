use crate::repo::block_map::BlockMap;
use crate::repo::types::CidAndBytes;
use crate::vendored::iroh_car::{CarHeader, CarWriter};
use anyhow::Result;
use libipld::Cid;

pub fn read_car_bytes(root: &Cid, blocks: BlockMap) -> Result<Vec<u8>> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(inner_car_bytes_reader(root, blocks))
}

async fn inner_car_bytes_reader(root: &Cid, blocks: BlockMap) -> Result<Vec<u8>> {
    let car_header = CarHeader::new_v1(vec![*root]);
    let buf: Vec<u8> = Default::default();
    let mut car_writer = CarWriter::new(car_header, buf);

    for CidAndBytes { cid, bytes } in blocks.entries()? {
        car_writer.write(cid, bytes).await?;
    }
    Ok(car_writer.finish().await?)
}
