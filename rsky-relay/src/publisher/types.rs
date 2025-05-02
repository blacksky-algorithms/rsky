use std::net::{SocketAddr, TcpStream};

use rtrb::{Consumer, Producer};

use crate::types::Cursor;

pub use maybe_tls_stream::MaybeTlsStream;

pub type CommandSender = Producer<Command>;
pub type CommandReceiver = Consumer<Command>;
pub type SubscribeReposSender = Producer<SubscribeRepos>;
pub type SubscribeReposReceiver = Consumer<SubscribeRepos>;

#[derive(Debug)]
pub struct SubscribeRepos {
    pub addr: SocketAddr,
    pub stream: MaybeTlsStream<TcpStream>,
    pub cursor: Option<Cursor>,
}

#[expect(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum Command {
    Connect(SubscribeRepos),
    Shutdown,
}

mod maybe_tls_stream {
    use std::fmt::{self, Debug};
    use std::io::{self, BufRead, Read, Write};
    use std::net::{Shutdown, TcpStream};

    use tungstenite::stream::NoDelay;

    /// A stream that might be protected with TLS.
    #[non_exhaustive]
    #[expect(clippy::large_enum_variant)]
    pub enum MaybeTlsStream<S: Read + Write> {
        /// Unencrypted socket stream.
        Plain(S),
        /// Encrypted socket stream using `rustls`.
        Rustls(rustls::StreamOwned<rustls::ServerConnection, S>),
    }

    impl MaybeTlsStream<TcpStream> {
        pub fn peek(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            match self {
                Self::Plain(s) => s.peek(buf),
                Self::Rustls(s) => {
                    let read = s.fill_buf()?;
                    buf[..read.len()].copy_from_slice(read);
                    Ok(read.len())
                }
            }
        }

        pub fn shutdown(&mut self) -> io::Result<()> {
            match self {
                Self::Plain(s) => s.shutdown(Shutdown::Both),
                Self::Rustls(s) => {
                    s.conn.send_close_notify();
                    s.conn.complete_io(&mut s.sock)?;
                    s.sock.shutdown(Shutdown::Both)
                }
            }
        }
    }

    impl<S: Read + Write + Debug> fmt::Debug for MaybeTlsStream<S> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Plain(s) => f.debug_tuple("MaybeTlsStream::Plain").field(s).finish(),
                Self::Rustls(s) => {
                    struct RustlsStreamDebug<'a, S: Read + Write>(
                        &'a rustls::StreamOwned<rustls::ServerConnection, S>,
                    );

                    impl<S: Read + Write + Debug> fmt::Debug for RustlsStreamDebug<'_, S> {
                        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                            f.debug_struct("StreamOwned")
                                .field("conn", &self.0.conn)
                                .field("sock", &self.0.sock)
                                .finish()
                        }
                    }

                    f.debug_tuple("MaybeTlsStream::Rustls").field(&RustlsStreamDebug(s)).finish()
                }
            }
        }
    }

    impl<S: Read + Write> Read for MaybeTlsStream<S> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            match *self {
                Self::Plain(ref mut s) => s.read(buf),
                Self::Rustls(ref mut s) => s.read(buf),
            }
        }
    }

    impl<S: Read + Write> Write for MaybeTlsStream<S> {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            match *self {
                Self::Plain(ref mut s) => s.write(buf),
                Self::Rustls(ref mut s) => s.write(buf),
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            match *self {
                Self::Plain(ref mut s) => s.flush(),
                Self::Rustls(ref mut s) => s.flush(),
            }
        }
    }

    impl<S: Read + Write + NoDelay> NoDelay for MaybeTlsStream<S> {
        fn set_nodelay(&mut self, nodelay: bool) -> io::Result<()> {
            match *self {
                Self::Plain(ref mut s) => s.set_nodelay(nodelay),
                Self::Rustls(ref mut s) => s.set_nodelay(nodelay),
            }
        }
    }
}
