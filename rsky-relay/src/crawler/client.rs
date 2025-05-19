use std::io;
use std::net::{TcpStream, ToSocketAddrs};

use http::Uri;
use http::request::Parts;
use socket2::{Domain, Protocol, Socket, Type};
use tungstenite::client::{IntoClientRequest, uri_mode};
use tungstenite::client_tls_with_config;
use tungstenite::error::{Error, Result, UrlError};
use tungstenite::handshake::client::Request;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::stream::{Mode, NoDelay};

use crate::crawler::types::{DecomposeError, HandshakeResult};

/// Connect to the given WebSocket in blocking mode.
///
/// The URL may be either ws:// or wss://.
/// To support wss:// URLs, feature `native-tls` or `rustls-tls` must be turned on.
///
/// This function "just works" for those who wants a simple blocking solution
/// similar to `std::net::TcpStream`. If you want a non-blocking or other
/// custom stream, call `client` instead.
///
/// This function uses `native_tls` or `rustls` to do TLS depending on the feature flags enabled. If
/// you want to use other TLS libraries, use `client` instead. There is no need to enable any of
/// the `*-tls` features if you don't call `connect` since it's the only function that uses them.
pub fn connect<Req: IntoClientRequest>(request: Req) -> HandshakeResult {
    connect_with_config(request, None, 3)
}

// Ref: https://github.com/snapview/tungstenite-rs/blob/master/src/client.rs
#[expect(
    clippy::expect_used,
    clippy::ignored_unit_patterns,
    clippy::redundant_clone,
    clippy::redundant_else
)]
pub fn connect_with_config<Req: IntoClientRequest>(
    request: Req, config: Option<WebSocketConfig>, max_redirects: u8,
) -> HandshakeResult {
    fn try_client_handshake(request: Request, config: Option<WebSocketConfig>) -> HandshakeResult {
        let uri = request.uri();
        let mode = uri_mode(uri)?;

        let host = request.uri().host().ok_or(Error::Url(UrlError::NoHostName))?;
        let host = if host.starts_with('[') { &host[1..host.len() - 1] } else { host };
        let port = uri.port_u16().unwrap_or(match mode {
            Mode::Plain => 80,
            Mode::Tls => 443,
        });
        let mut stream = connect_to_some((host, port), request.uri())?;
        NoDelay::set_nodelay(&mut stream, true)?;

        client_tls_with_config(request, stream, config, None).decompose()
    }

    fn create_request(parts: &Parts, uri: &Uri) -> Request {
        let mut builder =
            Request::builder().uri(uri.clone()).method(parts.method.clone()).version(parts.version);
        *builder.headers_mut().expect("Failed to create `Request`") = parts.headers.clone();
        builder.body(()).expect("Failed to create `Request`")
    }

    let (parts, _) = request.into_client_request()?.into_parts();
    let mut uri = parts.uri.clone();

    for attempt in 0..=max_redirects {
        let request = create_request(&parts, &uri);

        match try_client_handshake(request, config) {
            Err(Error::Http(res)) if res.status().is_redirection() && attempt < max_redirects => {
                if let Some(location) = res.headers().get("Location") {
                    uri = location.to_str()?.parse::<Uri>()?;
                    // debug!("Redirecting to {uri:?}");
                    continue;
                } else {
                    // warn!("No `Location` found in redirect");
                    return Err(Error::Http(res));
                }
            }
            other => return other,
        }
    }

    unreachable!("Bug in a redirect handling logic")
}

fn connect_to_some(addrs: impl ToSocketAddrs, uri: &Uri) -> Result<TcpStream> {
    fn is_blocking_error(error: &io::Error) -> bool {
        matches!(
            error.kind(),
            io::ErrorKind::Interrupted | io::ErrorKind::NotConnected | io::ErrorKind::WouldBlock
        ) || matches!(error.raw_os_error(), Some(libc::EINPROGRESS))
    }

    for addr in addrs.to_socket_addrs()? {
        // debug!("Trying to contact {uri} at {addr}...");
        let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
        socket.set_nonblocking(true)?;
        match socket.connect(&addr.into()) {
            Ok(()) => {}
            Err(e) if is_blocking_error(&e) => {}
            Err(_) => continue,
        }
        return Ok(socket.into());
    }
    Err(Error::Url(UrlError::UnableToConnect(uri.to_string())))
}
