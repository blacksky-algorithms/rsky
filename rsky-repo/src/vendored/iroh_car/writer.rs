#![allow(unused)]

use integer_encoding::VarIntAsyncWriter;
use lexicon_cid::Cid;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use super::{error::Error, header::CarHeader};

#[derive(Debug)]
pub struct CarWriter<W> {
    header: CarHeader,
    writer: W,
    cid_buffer: Vec<u8>,
    is_header_written: bool,
    /// A running count of bytes written so far (header + each block)
    bytes_written: usize,
}

impl<W> CarWriter<W>
where
    W: AsyncWrite + Send + Unpin,
{
    pub fn new(header: CarHeader, writer: W) -> Self {
        CarWriter {
            header,
            writer,
            cid_buffer: Vec::new(),
            is_header_written: false,
            bytes_written: 0
        }
    }

    /// Writes header and stream of data to writer in Car format.
    pub async fn write<T>(&mut self, cid: Cid, data: T) -> Result<(), Error>
    where
        T: AsRef<[u8]>,
    {
        if !self.is_header_written {
            // Write header bytes
            let header_bytes = self.header.encode()?;
            self.writer.write_varint_async(header_bytes.len()).await?;
            self.writer.write_all(&header_bytes).await?;
            self.bytes_written += varint_len(header_bytes.len()) + header_bytes.len();
            self.is_header_written = true;
        }

        // Write the given block.
        self.cid_buffer.clear();
        cid.write_bytes(&mut self.cid_buffer).expect("vec write");

        let block_data = data.as_ref();
        let block_len = self.cid_buffer.len() + block_data.len();
        
        // We'll write: varint(block_len) + cid bytes + block_data
        let varint_block_len = varint_len(block_len);
        
        self.writer.write_varint_async(block_len).await?;
        self.writer.write_all(&self.cid_buffer).await?;
        self.writer.write_all(block_data).await?;
        self.bytes_written += varint_block_len + block_len;

        Ok(())
    }

    /// Finishes writing, including flushing and returns the writer.
    pub async fn finish(mut self) -> Result<W, Error> {
        self.flush().await?;
        Ok(self.writer)
    }

    /// A helper function that returns how many bytes have been written so far.
    pub fn current_offset(&self) -> usize {
        self.bytes_written
    }

    /// Flushes the underlying writer.
    pub async fn flush(&mut self) -> Result<(), Error> {
        self.writer.flush().await?;
        Ok(())
    }

    /// Consumes the [`CarWriter`] and returns the underlying writer.
    pub fn into_inner(self) -> W {
        self.writer
    }
}

/// Returns how many bytes it takes to encode `val` as a varint.  
fn varint_len(val: usize) -> usize {
    // e.g. integer_encoding::VarInt::required_space(val)
    integer_encoding::VarInt::required_space(val)
}