use crate::repo::block_map::BlockMap;
use crate::repo::types::CidAndBytes;
use crate::vendored::iroh_car::{CarHeader, CarReader, CarWriter};
use anyhow::{bail, Result};
use futures::StreamExt;
use lexicon_cid::Cid;
use tokio::io::AsyncRead;

pub struct CarWithRoot {
    pub root: Cid,
    pub blocks: BlockMap,
}

pub struct CarToBlocksOutput {
    pub roots: Vec<Cid>,
    pub blocks: BlockMap,
}

pub async fn blocks_to_car_file(root: Option<&Cid>, blocks: BlockMap) -> Result<Vec<u8>> {
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

pub async fn car_to_blocks<R: AsyncRead + Send + Unpin>(
    car: CarReader<R>,
) -> Result<CarToBlocksOutput> {
    let roots = car.get_roots().to_vec();
    let mut blocks = BlockMap::new();
    let mut stream = Box::pin(car.stream());
    while let Some(Ok((cid, bytes))) = stream.next().await {
        blocks.set(cid, bytes);
    }
    Ok(CarToBlocksOutput { roots, blocks })
}

pub async fn read_car(bytes: Vec<u8>) -> Result<CarToBlocksOutput> {
    let car = CarReader::new(bytes.as_slice()).await?;
    car_to_blocks(car).await
}

pub async fn read_car_with_root(bytes: Vec<u8>) -> Result<CarWithRoot> {
    let CarToBlocksOutput { roots, blocks } = read_car(bytes).await?;
    if roots.len() != 1 {
        bail!("Expected one root, got {}", roots.len());
    }
    let root = roots[0];
    Ok(CarWithRoot { root, blocks })
}
