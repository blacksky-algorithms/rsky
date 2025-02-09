use crate::block_map::BlockMap;
use crate::util::stream_to_buffer;
use crate::vendored::iroh_car::{CarHeader, CarReader, CarWriter};
use anyhow::{bail, Result};
use async_stream::stream;
use futures::{pin_mut, Stream, StreamExt};
use lexicon_cid::Cid;
use std::future::Future;
use tokio::io::AsyncRead;
use tokio::{
    io::{AsyncReadExt, DuplexStream},
    sync::oneshot,
};

pub struct CarWithRoot {
    pub root: Cid,
    pub blocks: BlockMap,
}

pub struct CarToBlocksOutput {
    pub roots: Vec<Cid>,
    pub blocks: BlockMap,
}

pub fn write_car_stream<F, Fut>(
    root: Option<&Cid>,
    user_fn: F,
) -> impl Stream<Item = Result<Vec<u8>>> + Send + 'static
where
    F: FnOnce(CarWriter<DuplexStream>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<CarWriter<DuplexStream>>> + Send + 'static,
{
    // Create a duplex pipe for streaming data
    let (writer, mut reader) = tokio::io::duplex(8 * 1024); // 8KB buffer

    // Create CAR header with roots
    let roots = root.map_or_else(Vec::new, |r| vec![r.clone()]);
    let header = CarHeader::new_v1(roots);
    let car_writer = CarWriter::new(header, writer);

    // Channel for error propagation
    let (error_sender, error_receiver) = oneshot::channel();

    // Spawn task to handle writing operations
    tokio::spawn(async move {
        let result = async {
            let mut car_writer = car_writer;
            car_writer = user_fn(car_writer).await?;
            car_writer.finish().await?;
            Ok(())
        }
        .await;

        if let Err(e) = result {
            let _ = error_sender.send(e);
        }
    });

    // Create stream that reads from the duplex reader
    stream! {
        let mut error_receiver = Some(error_receiver);
        let mut buf = [0; 8192]; // 8KB read buffer

        loop {
            tokio::select! {
                // Check for errors from writer task
                res = async {
                    // Safe to unwrap because this branch only runs if error_receiver.is_some()
                    let rx = error_receiver.take().unwrap();
                    rx.await
                }, if error_receiver.is_some() => {
                    match res {
                        Ok(err) => {
                            // Writer task sent an error, propagate it
                            yield Err(err);
                            break;
                        }
                        Err(_e) => {
                            // Writer task completed without error, stop checking
                            error_receiver = None;
                        }
                    }
                }

                // Read data from the pipe
                read_res = reader.read(&mut buf) => {
                    match read_res {
                        Ok(0) => break, // EOF
                        Ok(n) => {
                            yield Ok(Vec::from(&buf[..n]));
                        }
                        Err(e) => {
                            yield Err(e.into());
                            break;
                        }
                    }
                }
            }
        }
    }
}

pub async fn write_car<F, Fut>(
    root: Option<&Cid>,
    user_fn: F,
) -> impl Stream<Item = Result<Vec<u8>>> + Send + 'static
where
    F: FnOnce(CarWriter<DuplexStream>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<CarWriter<DuplexStream>>> + Send + 'static,
{
    write_car_stream(root, user_fn)
}

/// Converts a BlockMap to a CAR stream
pub fn blocks_to_car_stream(
    root: Option<&Cid>,
    blocks: BlockMap,
) -> impl Stream<Item = Result<Vec<u8>>> + Send + 'static {
    write_car_stream(root, move |mut writer| async move {
        for entry in blocks.into_iter() {
            writer.write(entry.cid, entry.bytes).await?;
        }
        Ok(writer)
    })
}

pub async fn blocks_to_car_file(root: Option<&Cid>, blocks: BlockMap) -> Result<Vec<u8>> {
    let car_stream = blocks_to_car_stream(root, blocks);
    pin_mut!(car_stream);
    stream_to_buffer(car_stream).await
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
