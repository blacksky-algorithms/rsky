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

#[derive(Debug)]
pub enum Command {
    Connect(SubscribeRepos),
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

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::Cursor as IoCursor;
        use std::net::{TcpListener, TcpStream};
        use std::sync::Mutex;

        // A test-only Read+Write+NoDelay backing store for exercising Plain methods.
        #[derive(Debug, Default)]
        struct Pipe {
            buf: Vec<u8>,
            nodelay: Mutex<Option<bool>>,
        }
        impl Read for Pipe {
            fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
                let n = dst.len().min(self.buf.len());
                dst[..n].copy_from_slice(&self.buf[..n]);
                self.buf.drain(..n);
                Ok(n)
            }
        }
        impl Write for Pipe {
            fn write(&mut self, src: &[u8]) -> io::Result<usize> {
                self.buf.extend_from_slice(src);
                Ok(src.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        impl NoDelay for Pipe {
            fn set_nodelay(&mut self, v: bool) -> io::Result<()> {
                *self.nodelay.lock().unwrap() = Some(v);
                Ok(())
            }
        }

        #[test]
        fn plain_read_write_round_trips() {
            let mut s = MaybeTlsStream::Plain(IoCursor::new(Vec::<u8>::new()));
            assert_eq!(s.write(b"hello").unwrap(), 5);
            s.flush().unwrap();
            // Reset cursor pos for read.
            if let MaybeTlsStream::Plain(c) = &mut s {
                c.set_position(0);
            }
            let mut out = [0u8; 5];
            assert_eq!(s.read(&mut out).unwrap(), 5);
            assert_eq!(&out, b"hello");
        }

        #[test]
        fn plain_set_nodelay_reaches_inner() {
            let mut s = MaybeTlsStream::Plain(Pipe::default());
            s.set_nodelay(true).unwrap();
            if let MaybeTlsStream::Plain(p) = &s {
                assert_eq!(*p.nodelay.lock().unwrap(), Some(true));
            } else {
                panic!("not Plain");
            }
        }

        #[test]
        fn debug_renders_plain_variant() {
            let s: MaybeTlsStream<IoCursor<Vec<u8>>> =
                MaybeTlsStream::Plain(IoCursor::new(vec![1u8]));
            let dbg = format!("{s:?}");
            assert!(dbg.starts_with("MaybeTlsStream::Plain"));
        }

        #[test]
        fn plain_tcp_peek_returns_buffered_bytes() {
            // Local TCP pair: write from server side, peek from client side.
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let server_thread = std::thread::spawn(move || {
                let (mut s, _) = listener.accept().unwrap();
                s.write_all(b"abc").unwrap();
                s.flush().unwrap();
            });
            let client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            // Wait until the bytes arrive (deterministic via small loop).
            let mut s = MaybeTlsStream::Plain(client);
            let mut buf = [0u8; 8];
            let mut got = 0usize;
            for _ in 0..50 {
                if let Ok(n) = s.peek(&mut buf) {
                    if n >= 3 {
                        got = n;
                        break;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            server_thread.join().unwrap();
            assert!(got >= 3, "peek did not yield bytes; got {got}");
            assert_eq!(&buf[..3], b"abc");
        }

        #[test]
        fn plain_tcp_shutdown_succeeds() {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let server_thread = std::thread::spawn(move || {
                drop(listener.accept());
            });
            let client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let mut s = MaybeTlsStream::Plain(client);
            s.shutdown().unwrap();
            server_thread.join().unwrap();
        }
    }
}
