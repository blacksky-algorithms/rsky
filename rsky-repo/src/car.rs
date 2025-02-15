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
    read_stream_car(bytes.as_slice()).await
}

pub async fn read_car_with_root(bytes: Vec<u8>) -> Result<CarWithRoot> {
    read_stream_car_with_root(bytes.as_slice()).await
}

pub async fn read_stream_car_with_root<R: AsyncRead + Send + Unpin>(
    bytes: R,
) -> Result<CarWithRoot> {
    let CarToBlocksOutput { roots, blocks } = read_stream_car(bytes).await?;
    if roots.len() != 1 {
        bail!("Expected one root, got {}", roots.len());
    }
    let root = roots[0];
    Ok(CarWithRoot { root, blocks })
}

pub async fn read_stream_car<R: AsyncRead + Send + Unpin>(bytes: R) -> Result<CarToBlocksOutput> {
    let car = CarReader::new(bytes).await?;
    car_to_blocks(car).await
}

#[cfg(test)]
mod tests {
    use crate::block_map::{BlockMap, Bytes};
    use crate::car::{read_car_with_root, read_stream_car_with_root, CarWithRoot};
    use lexicon_cid::multihash::Multihash;
    use lexicon_cid::{Cid, Version};
    use std::collections::BTreeMap;
    use std::io::Read;

    macro_rules! test_case {
        ($fname:expr) => {
            concat!(env!("CARGO_MANIFEST_DIR"), "/resources/test/", $fname) // assumes Linux ('/')!
        };
    }

    fn fetch_valid_repo() -> (Cid, BlockMap) {
        let root = Cid::new(
            Version::V1,
            113,
            Multihash::from_bytes(&[
                18, 32, 105, 192, 230, 151, 125, 153, 153, 186, 221, 178, 55, 124, 35, 243, 245,
                55, 82, 223, 81, 45, 33, 1, 239, 220, 188, 99, 231, 110, 208, 80, 206, 188,
            ])
            .unwrap(),
        )
        .unwrap();

        let mut map: BTreeMap<String, Bytes> = BTreeMap::new();
        let arr: &[u8] = &[
            165, 100, 116, 101, 120, 116, 103, 82, 101, 112, 108, 121, 32, 50, 101, 36, 116, 121,
            112, 101, 114, 97, 112, 112, 46, 98, 115, 107, 121, 46, 102, 101, 101, 100, 46, 112,
            111, 115, 116, 101, 108, 97, 110, 103, 115, 129, 98, 101, 110, 101, 114, 101, 112, 108,
            121, 162, 100, 114, 111, 111, 116, 162, 99, 99, 105, 100, 120, 59, 98, 97, 102, 121,
            114, 101, 105, 104, 100, 52, 116, 98, 102, 55, 122, 122, 120, 51, 105, 114, 121, 110,
            105, 98, 114, 99, 121, 99, 55, 50, 112, 104, 53, 51, 121, 116, 113, 52, 107, 110, 117,
            50, 98, 106, 106, 106, 104, 112, 111, 119, 118, 55, 53, 106, 112, 109, 102, 99, 109,
            99, 117, 114, 105, 120, 70, 97, 116, 58, 47, 47, 100, 105, 100, 58, 112, 108, 99, 58,
            114, 55, 102, 100, 104, 113, 109, 119, 51, 104, 50, 99, 105, 102, 101, 97, 107, 119,
            53, 104, 109, 118, 121, 54, 47, 97, 112, 112, 46, 98, 115, 107, 121, 46, 102, 101, 101,
            100, 46, 112, 111, 115, 116, 47, 51, 108, 104, 109, 121, 100, 50, 55, 103, 115, 107,
            50, 51, 102, 112, 97, 114, 101, 110, 116, 162, 99, 99, 105, 100, 120, 59, 98, 97, 102,
            121, 114, 101, 105, 103, 105, 52, 103, 100, 111, 119, 108, 104, 119, 121, 108, 53, 108,
            120, 98, 98, 50, 111, 122, 113, 53, 105, 122, 52, 120, 55, 119, 114, 55, 112, 120, 54,
            109, 118, 98, 118, 111, 107, 117, 113, 112, 103, 115, 102, 101, 103, 100, 99, 101, 112,
            121, 99, 117, 114, 105, 120, 70, 97, 116, 58, 47, 47, 100, 105, 100, 58, 112, 108, 99,
            58, 114, 55, 102, 100, 104, 113, 109, 119, 51, 104, 50, 99, 105, 102, 101, 97, 107,
            119, 53, 104, 109, 118, 121, 54, 47, 97, 112, 112, 46, 98, 115, 107, 121, 46, 102, 101,
            101, 100, 46, 112, 111, 115, 116, 47, 51, 108, 104, 109, 121, 100, 100, 55, 99, 112,
            115, 50, 51, 105, 99, 114, 101, 97, 116, 101, 100, 65, 116, 120, 24, 50, 48, 50, 53,
            45, 48, 50, 45, 48, 56, 84, 48, 49, 58, 52, 49, 58, 50, 54, 46, 57, 57, 50, 90,
        ];
        let bytes: Bytes = Bytes(arr.to_vec());
        map.insert(
            "bafyreiaau22fcyoampbgk7wbuhv3hux6zrgqav7c4cjn76wb7cyokdple4".to_string(),
            bytes,
        );
        let arr: &[u8] = &[
            166, 99, 100, 105, 100, 120, 32, 100, 105, 100, 58, 112, 108, 99, 58, 114, 55, 102,
            100, 104, 113, 109, 119, 51, 104, 50, 99, 105, 102, 101, 97, 107, 119, 53, 104, 109,
            118, 121, 54, 99, 114, 101, 118, 109, 51, 108, 104, 109, 121, 100, 104, 119, 105, 122,
            112, 50, 100, 99, 115, 105, 103, 88, 64, 59, 192, 157, 148, 153, 52, 170, 211, 68, 176,
            221, 10, 155, 27, 6, 207, 58, 223, 239, 147, 73, 124, 82, 38, 192, 179, 191, 13, 23,
            139, 210, 255, 64, 73, 23, 192, 168, 134, 179, 33, 197, 65, 81, 106, 132, 183, 189, 93,
            131, 77, 71, 24, 233, 197, 192, 41, 205, 183, 212, 230, 221, 241, 78, 206, 100, 100,
            97, 116, 97, 216, 42, 88, 37, 0, 1, 113, 18, 32, 224, 109, 158, 163, 87, 52, 134, 27,
            28, 168, 202, 252, 247, 134, 56, 160, 134, 176, 86, 132, 92, 180, 177, 22, 199, 159,
            30, 26, 194, 40, 206, 102, 100, 112, 114, 101, 118, 246, 103, 118, 101, 114, 115, 105,
            111, 110, 3,
        ];
        let bytes: Bytes = Bytes(arr.to_vec());
        map.insert(
            "bafyreidjydtjo7mztg5n3mrxpqr7h5jxklpvcljbahx5zpdd45xnaugoxq".to_string(),
            bytes,
        );
        let arr: &[u8] = &[
            164, 100, 116, 101, 120, 116, 102, 80, 111, 115, 116, 32, 50, 101, 36, 116, 121, 112,
            101, 114, 97, 112, 112, 46, 98, 115, 107, 121, 46, 102, 101, 101, 100, 46, 112, 111,
            115, 116, 101, 108, 97, 110, 103, 115, 129, 98, 101, 110, 105, 99, 114, 101, 97, 116,
            101, 100, 65, 116, 120, 24, 50, 48, 50, 53, 45, 48, 50, 45, 48, 56, 84, 48, 49, 58, 52,
            49, 58, 49, 56, 46, 51, 52, 50, 90,
        ];
        let bytes: Bytes = Bytes(arr.to_vec());
        map.insert(
            "bafyreie6ohdwckxus23cuvd737xsmzqmc34omqxgpiqivpzk56fn4f343i".to_string(),
            bytes,
        );
        let arr: &[u8] = &[
            163, 101, 36, 116, 121, 112, 101, 117, 97, 112, 112, 46, 98, 115, 107, 121, 46, 103,
            114, 97, 112, 104, 46, 102, 111, 108, 108, 111, 119, 103, 115, 117, 98, 106, 101, 99,
            116, 120, 32, 100, 105, 100, 58, 112, 108, 99, 58, 122, 55, 50, 105, 55, 104, 100, 121,
            110, 109, 107, 54, 114, 50, 50, 122, 50, 55, 104, 54, 116, 118, 117, 114, 105, 99, 114,
            101, 97, 116, 101, 100, 65, 116, 120, 24, 50, 48, 50, 53, 45, 48, 50, 45, 48, 56, 84,
            48, 49, 58, 49, 57, 58, 52, 50, 46, 52, 54, 55, 90,
        ];
        let bytes: Bytes = Bytes(arr.to_vec());
        map.insert(
            "bafyreiecinm3zdkks2aviabdqtle4k5xswcqk65v3u2ne3m3bqxmg3oo74".to_string(),
            bytes,
        );
        let arr: &[u8] = &[
            162, 97, 101, 130, 164, 97, 107, 88, 32, 97, 112, 112, 46, 98, 115, 107, 121, 46, 102,
            101, 101, 100, 46, 112, 111, 115, 116, 47, 51, 108, 104, 109, 121, 100, 104, 100, 106,
            54, 115, 50, 51, 97, 112, 0, 97, 116, 246, 97, 118, 216, 42, 88, 37, 0, 1, 113, 18, 32,
            0, 166, 180, 81, 97, 192, 99, 194, 101, 126, 193, 161, 235, 179, 210, 254, 204, 77, 0,
            87, 226, 224, 146, 223, 250, 193, 248, 176, 229, 13, 235, 39, 164, 97, 107, 88, 26,
            103, 114, 97, 112, 104, 46, 102, 111, 108, 108, 111, 119, 47, 51, 108, 104, 109, 120,
            52, 108, 97, 108, 120, 115, 50, 51, 97, 112, 9, 97, 116, 246, 97, 118, 216, 42, 88, 37,
            0, 1, 113, 18, 32, 130, 67, 89, 188, 141, 74, 150, 129, 84, 0, 35, 132, 214, 78, 43,
            183, 149, 133, 5, 123, 181, 221, 52, 210, 109, 155, 12, 46, 195, 109, 206, 255, 97,
            108, 246,
        ];
        let bytes: Bytes = Bytes(arr.to_vec());
        map.insert(
            "bafyreieemvw7ugze3jhx4x7iwf4uyx73glmyoc4swvznd5ztnognqgpv24".to_string(),
            bytes,
        );
        let arr: &[u8] = &[
            162, 97, 101, 130, 164, 97, 107, 88, 32, 97, 112, 112, 46, 98, 115, 107, 121, 46, 102,
            101, 101, 100, 46, 112, 111, 115, 116, 47, 51, 108, 104, 109, 121, 100, 50, 55, 103,
            115, 107, 50, 51, 97, 112, 0, 97, 116, 246, 97, 118, 216, 42, 88, 37, 0, 1, 113, 18,
            32, 227, 228, 194, 95, 231, 55, 218, 35, 134, 160, 49, 22, 5, 253, 60, 253, 222, 39,
            14, 41, 180, 208, 82, 148, 157, 238, 181, 127, 212, 189, 133, 19, 164, 97, 107, 71, 55,
            51, 106, 119, 99, 50, 51, 97, 112, 24, 25, 97, 116, 246, 97, 118, 216, 42, 88, 37, 0,
            1, 113, 18, 32, 158, 113, 199, 97, 42, 244, 150, 182, 42, 84, 127, 223, 239, 38, 102,
            12, 22, 248, 230, 66, 230, 122, 32, 138, 191, 42, 239, 138, 222, 23, 124, 218, 97, 108,
            246,
        ];
        let bytes: Bytes = Bytes(arr.to_vec());
        map.insert(
            "bafyreifbryjdakyw2w5urgj7yeiua3cfh5ky64c5ipbxhqy6mewbtmikqy".to_string(),
            bytes,
        );
        let arr: &[u8] = &[
            162, 97, 101, 129, 164, 97, 107, 88, 27, 97, 112, 112, 46, 98, 115, 107, 121, 46, 97,
            99, 116, 111, 114, 46, 112, 114, 111, 102, 105, 108, 101, 47, 115, 101, 108, 102, 97,
            112, 0, 97, 116, 216, 42, 88, 37, 0, 1, 113, 18, 32, 161, 142, 18, 48, 43, 22, 213,
            187, 72, 153, 63, 193, 17, 64, 108, 69, 63, 85, 143, 112, 93, 67, 195, 115, 195, 30,
            97, 44, 25, 177, 10, 134, 97, 118, 216, 42, 88, 37, 0, 1, 113, 18, 32, 246, 40, 16, 84,
            144, 58, 210, 79, 88, 0, 75, 132, 89, 3, 154, 217, 173, 115, 107, 229, 108, 8, 10, 222,
            195, 21, 45, 248, 8, 246, 76, 231, 97, 108, 246,
        ];
        let bytes: Bytes = Bytes(arr.to_vec());
        map.insert(
            "bafyreifjtlanrukysp6kr2zotumlnjtmorqozyw67u7uh6cc5ehft5spcy".to_string(),
            bytes,
        );
        let arr: &[u8] = &[
            162, 97, 101, 128, 97, 108, 216, 42, 88, 37, 0, 1, 113, 18, 32, 132, 101, 109, 250, 27,
            36, 218, 79, 126, 95, 232, 177, 121, 76, 95, 251, 50, 217, 135, 11, 146, 181, 114, 209,
            247, 51, 107, 140, 216, 25, 245, 215,
        ];
        let bytes: Bytes = Bytes(arr.to_vec());
        map.insert(
            "bafyreifoftislgcupyprxq2shgapmswafdw7pc7qdmjxpks6vscjkioxte".to_string(),
            bytes,
        );
        let arr: &[u8] = &[
            165, 100, 116, 101, 120, 116, 103, 82, 101, 112, 108, 121, 32, 49, 101, 36, 116, 121,
            112, 101, 114, 97, 112, 112, 46, 98, 115, 107, 121, 46, 102, 101, 101, 100, 46, 112,
            111, 115, 116, 101, 108, 97, 110, 103, 115, 129, 98, 101, 110, 101, 114, 101, 112, 108,
            121, 162, 100, 114, 111, 111, 116, 162, 99, 99, 105, 100, 120, 59, 98, 97, 102, 121,
            114, 101, 105, 104, 100, 52, 116, 98, 102, 55, 122, 122, 120, 51, 105, 114, 121, 110,
            105, 98, 114, 99, 121, 99, 55, 50, 112, 104, 53, 51, 121, 116, 113, 52, 107, 110, 117,
            50, 98, 106, 106, 106, 104, 112, 111, 119, 118, 55, 53, 106, 112, 109, 102, 99, 109,
            99, 117, 114, 105, 120, 70, 97, 116, 58, 47, 47, 100, 105, 100, 58, 112, 108, 99, 58,
            114, 55, 102, 100, 104, 113, 109, 119, 51, 104, 50, 99, 105, 102, 101, 97, 107, 119,
            53, 104, 109, 118, 121, 54, 47, 97, 112, 112, 46, 98, 115, 107, 121, 46, 102, 101, 101,
            100, 46, 112, 111, 115, 116, 47, 51, 108, 104, 109, 121, 100, 50, 55, 103, 115, 107,
            50, 51, 102, 112, 97, 114, 101, 110, 116, 162, 99, 99, 105, 100, 120, 59, 98, 97, 102,
            121, 114, 101, 105, 104, 100, 52, 116, 98, 102, 55, 122, 122, 120, 51, 105, 114, 121,
            110, 105, 98, 114, 99, 121, 99, 55, 50, 112, 104, 53, 51, 121, 116, 113, 52, 107, 110,
            117, 50, 98, 106, 106, 106, 104, 112, 111, 119, 118, 55, 53, 106, 112, 109, 102, 99,
            109, 99, 117, 114, 105, 120, 70, 97, 116, 58, 47, 47, 100, 105, 100, 58, 112, 108, 99,
            58, 114, 55, 102, 100, 104, 113, 109, 119, 51, 104, 50, 99, 105, 102, 101, 97, 107,
            119, 53, 104, 109, 118, 121, 54, 47, 97, 112, 112, 46, 98, 115, 107, 121, 46, 102, 101,
            101, 100, 46, 112, 111, 115, 116, 47, 51, 108, 104, 109, 121, 100, 50, 55, 103, 115,
            107, 50, 51, 105, 99, 114, 101, 97, 116, 101, 100, 65, 116, 120, 24, 50, 48, 50, 53,
            45, 48, 50, 45, 48, 56, 84, 48, 49, 58, 52, 49, 58, 50, 50, 46, 54, 53, 57, 90,
        ];
        let bytes: Bytes = Bytes(arr.to_vec());
        map.insert(
            "bafyreigi4gdowlhwyl5lxbb2ozq5iz4x7wr7px6mvbvokuqpgsfegdcepy".to_string(),
            bytes,
        );
        let arr: &[u8] = &[
            162, 97, 101, 129, 164, 97, 107, 88, 32, 97, 112, 112, 46, 98, 115, 107, 121, 46, 102,
            101, 101, 100, 46, 112, 111, 115, 116, 47, 51, 108, 104, 109, 121, 100, 100, 55, 99,
            112, 115, 50, 51, 97, 112, 0, 97, 116, 216, 42, 88, 37, 0, 1, 113, 18, 32, 174, 44,
            209, 37, 152, 84, 126, 31, 27, 195, 82, 57, 128, 246, 74, 192, 40, 237, 247, 139, 240,
            27, 19, 119, 170, 94, 172, 132, 149, 33, 215, 153, 97, 118, 216, 42, 88, 37, 0, 1, 113,
            18, 32, 200, 225, 134, 235, 44, 246, 194, 250, 187, 132, 58, 118, 97, 212, 103, 151,
            253, 163, 247, 223, 204, 168, 106, 229, 82, 15, 52, 138, 67, 12, 68, 126, 97, 108, 216,
            42, 88, 37, 0, 1, 113, 18, 32, 169, 154, 192, 216, 209, 88, 147, 252, 168, 235, 46,
            157, 24, 182, 166, 108, 116, 96, 236, 226, 222, 253, 63, 67, 248, 66, 233, 14, 89, 246,
            79, 22,
        ];
        let bytes: Bytes = Bytes(arr.to_vec());
        map.insert(
            "bafyreihanwpkgvzuqynrzkgk7t3ymofaq2yfnbc4wsyrnr47dynmekgomy".to_string(),
            bytes,
        );
        let arr: &[u8] = &[
            164, 100, 116, 101, 120, 116, 102, 80, 111, 115, 116, 32, 49, 101, 36, 116, 121, 112,
            101, 114, 97, 112, 112, 46, 98, 115, 107, 121, 46, 102, 101, 101, 100, 46, 112, 111,
            115, 116, 101, 108, 97, 110, 103, 115, 129, 98, 101, 110, 105, 99, 114, 101, 97, 116,
            101, 100, 65, 116, 120, 24, 50, 48, 50, 53, 45, 48, 50, 45, 48, 56, 84, 48, 49, 58, 52,
            49, 58, 49, 51, 46, 50, 50, 54, 90,
        ];
        let bytes: Bytes = Bytes(arr.to_vec());
        map.insert(
            "bafyreihd4tbf7zzx3irynibrcyc72ph53ytq4knu2bjjjhpowv75jpmfcm".to_string(),
            bytes,
        );
        let arr: &[u8] = &[
            164, 101, 36, 116, 121, 112, 101, 118, 97, 112, 112, 46, 98, 115, 107, 121, 46, 97, 99,
            116, 111, 114, 46, 112, 114, 111, 102, 105, 108, 101, 102, 97, 118, 97, 116, 97, 114,
            164, 99, 114, 101, 102, 216, 42, 88, 37, 0, 1, 85, 18, 32, 147, 77, 143, 132, 8, 79,
            249, 181, 62, 150, 39, 198, 203, 198, 47, 138, 134, 3, 67, 9, 134, 166, 215, 85, 243,
            8, 202, 243, 94, 229, 7, 120, 100, 115, 105, 122, 101, 25, 126, 132, 101, 36, 116, 121,
            112, 101, 100, 98, 108, 111, 98, 104, 109, 105, 109, 101, 84, 121, 112, 101, 105, 105,
            109, 97, 103, 101, 47, 112, 110, 103, 105, 99, 114, 101, 97, 116, 101, 100, 65, 116,
            120, 24, 50, 48, 50, 53, 45, 48, 50, 45, 48, 56, 84, 48, 49, 58, 49, 57, 58, 52, 51,
            46, 48, 53, 53, 90, 107, 100, 105, 115, 112, 108, 97, 121, 78, 97, 109, 101, 96,
        ];
        let bytes: Bytes = Bytes(arr.to_vec());
        map.insert(
            "bafyreihwfaifjeb22jhvqaclqrmqhgwzvvzwxzlmbafn5qyvfx4ar5sm44".to_string(),
            bytes,
        );

        (root, BlockMap { map })
    }

    #[tokio::test]
    async fn test_read_car_with_root_valid() {
        let mut file = std::fs::File::open(test_case!("valid_repo.car")).unwrap();
        let mut bytes: Vec<u8> = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        let car_with_root = read_car_with_root(bytes).await.expect("Failed to read car");
        let (root, blocks) = fetch_valid_repo();
        let expected = CarWithRoot { root, blocks };

        assert_eq!(car_with_root.root, expected.root);
        assert_eq!(car_with_root.blocks, expected.blocks);
    }

    #[tokio::test]
    async fn test_read_stream_car_with_root() {
        let file = tokio::fs::File::open(test_case!("valid_repo.car"))
            .await
            .unwrap();
        let car_with_root = read_stream_car_with_root(file)
            .await
            .expect("Failed to read car");
        let (root, blocks) = fetch_valid_repo();
        let expected = CarWithRoot { root, blocks };

        assert_eq!(car_with_root.root, expected.root);
        assert_eq!(car_with_root.blocks, expected.blocks);
    }
}
