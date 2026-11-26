use futures::{Sink, SinkExt, Stream, StreamExt};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::{
    interval_at, sleep_until, timeout, timeout_at, Instant, Interval, MissedTickBehavior,
};
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use url::Url;

pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const KEEPALIVES_PER_IDLE_TIMEOUT: u32 = 3;

#[derive(Debug, thiserror::Error)]
pub enum Disconnect {
    #[error("WebSocket connection closed by server.")]
    Closed,
    #[error("WebSocket idle for {}s, reconnecting", .0.as_secs())]
    Idle(Duration),
    #[error("WebSocket error: {0}")]
    Socket(#[from] WsError),
}

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("handshake did not complete within {}s", .0.as_secs())]
    Timeout(Duration),
    #[error(transparent)]
    Socket(#[from] WsError),
}

pub fn idle_timeout(configured_secs: Option<&str>) -> Duration {
    configured_secs
        .and_then(|secs| secs.parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map_or(DEFAULT_IDLE_TIMEOUT, Duration::from_secs)
}

pub async fn connect(
    url: &Url,
    handshake_timeout: Duration,
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, ConnectError> {
    match timeout(handshake_timeout, connect_async(url)).await {
        Ok(Ok((socket, _response))) => Ok(socket),
        Ok(Err(error)) => Err(ConnectError::Socket(error)),
        Err(_) => Err(ConnectError::Timeout(handshake_timeout)),
    }
}

pub struct Subscription<R, W> {
    reader: R,
    writer: W,
    idle_timeout: Duration,
    keepalive: Interval,
    deadline: Instant,
}

impl<R, W> Subscription<R, W>
where
    R: Stream<Item = Result<Message, WsError>> + Unpin,
    W: Sink<Message, Error = WsError> + Unpin,
{
    pub fn new(reader: R, writer: W, idle_timeout: Duration) -> Self {
        let keepalive_period = idle_timeout / KEEPALIVES_PER_IDLE_TIMEOUT;
        let now = Instant::now();
        let mut keepalive = interval_at(now + keepalive_period, keepalive_period);
        keepalive.set_missed_tick_behavior(MissedTickBehavior::Delay);
        Self {
            reader,
            writer,
            idle_timeout,
            keepalive,
            deadline: now + idle_timeout,
        }
    }

    /// Any inbound frame, pongs included, proves the peer is alive and resets the idle deadline.
    pub async fn next_binary(&mut self) -> Result<Vec<u8>, Disconnect> {
        loop {
            tokio::select! {
                biased;
                frame = self.reader.next() => {
                    let message = frame.ok_or(Disconnect::Closed)??;
                    self.deadline = Instant::now() + self.idle_timeout;
                    match message {
                        Message::Binary(payload) => return Ok(payload),
                        Message::Close(_) => return Err(Disconnect::Closed),
                        _ => {}
                    }
                }
                () = sleep_until(self.deadline) => return Err(Disconnect::Idle(self.idle_timeout)),
                _ = self.keepalive.tick() => {
                    timeout_at(self.deadline, self.writer.send(Message::Ping(Vec::new())))
                        .await
                        .map_err(|_| Disconnect::Idle(self.idle_timeout))??;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::channel::mpsc;
    use tokio::net::TcpListener;

    const IDLE: Duration = Duration::from_secs(60);

    type Inbound = mpsc::UnboundedSender<Result<Message, WsError>>;
    type Reader = mpsc::UnboundedReceiver<Result<Message, WsError>>;
    type Writer<S> = futures::sink::SinkMapErr<S, fn(mpsc::SendError) -> WsError>;

    fn closed(_: mpsc::SendError) -> WsError {
        WsError::ConnectionClosed
    }

    fn subscription() -> (
        Inbound,
        mpsc::UnboundedReceiver<Message>,
        Subscription<Reader, Writer<mpsc::UnboundedSender<Message>>>,
    ) {
        let (inbound, reader) = mpsc::unbounded();
        let (writer, outbound) = mpsc::unbounded();
        let writer = writer.sink_map_err(closed as fn(mpsc::SendError) -> WsError);
        (inbound, outbound, Subscription::new(reader, writer, IDLE))
    }

    fn deliver(inbound: &Inbound, message: Message) {
        inbound.unbounded_send(Ok(message)).unwrap();
    }

    #[test]
    fn idle_timeout_defaults_when_unset_or_invalid() {
        assert_eq!(idle_timeout(None), DEFAULT_IDLE_TIMEOUT);
        assert_eq!(idle_timeout(Some("abc")), DEFAULT_IDLE_TIMEOUT);
        assert_eq!(idle_timeout(Some("0")), DEFAULT_IDLE_TIMEOUT);
        assert_eq!(idle_timeout(Some("15")), Duration::from_secs(15));
    }

    #[tokio::test(start_paused = true)]
    async fn returns_binary_payloads_and_skips_other_frames() {
        let (inbound, _outbound, mut subscription) = subscription();
        deliver(&inbound, Message::Text("info".into()));
        deliver(&inbound, Message::Pong(Vec::new()));
        deliver(&inbound, Message::Binary(vec![1, 2, 3]));
        assert_eq!(subscription.next_binary().await.unwrap(), vec![1, 2, 3]);
    }

    #[tokio::test(start_paused = true)]
    async fn close_frame_and_end_of_stream_disconnect_as_closed() {
        let (inbound, _outbound, mut subscription) = subscription();
        deliver(&inbound, Message::Close(None));
        assert!(matches!(
            subscription.next_binary().await,
            Err(Disconnect::Closed)
        ));
        drop(inbound);
        assert!(matches!(
            subscription.next_binary().await,
            Err(Disconnect::Closed)
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn socket_errors_disconnect() {
        let (inbound, _outbound, mut subscription) = subscription();
        inbound.unbounded_send(Err(WsError::Utf8)).unwrap();
        let error = subscription.next_binary().await.unwrap_err();
        assert!(matches!(error, Disconnect::Socket(WsError::Utf8)));
        assert_eq!(
            error.to_string(),
            format!("WebSocket error: {}", WsError::Utf8)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn silent_peer_is_pinged_then_disconnected_at_the_idle_deadline() {
        let (_inbound, mut outbound, mut subscription) = subscription();
        let started = Instant::now();
        let error = subscription.next_binary().await.unwrap_err();
        assert!(matches!(error, Disconnect::Idle(IDLE)));
        assert_eq!(error.to_string(), "WebSocket idle for 60s, reconnecting");
        assert_eq!(started.elapsed(), IDLE);
        let mut pings = 0;
        while let Ok(Some(message)) = outbound.try_next() {
            assert_eq!(message, Message::Ping(Vec::new()));
            pings += 1;
        }
        assert_eq!(pings, 2);
    }

    #[tokio::test(start_paused = true)]
    async fn frame_queued_past_the_deadline_is_delivered_without_a_ping_burst() {
        let (inbound, mut outbound, mut subscription) = subscription();
        deliver(&inbound, Message::Binary(vec![4]));
        tokio::time::advance(Duration::from_secs(100)).await;
        assert_eq!(subscription.next_binary().await.unwrap(), vec![4]);
        let resumed = Instant::now();
        assert!(matches!(
            subscription.next_binary().await,
            Err(Disconnect::Idle(IDLE))
        ));
        assert_eq!(resumed.elapsed(), IDLE);
        let mut pings = 0;
        while let Ok(Some(_)) = outbound.try_next() {
            pings += 1;
        }
        assert_eq!(pings, 3);
    }

    #[tokio::test(start_paused = true)]
    async fn pongs_keep_a_quiet_subscription_alive() {
        let (inbound, _outbound, mut subscription) = subscription();
        let started = Instant::now();
        let peer = tokio::spawn(async move {
            for _ in 0..5 {
                tokio::time::sleep(Duration::from_secs(45)).await;
                deliver(&inbound, Message::Pong(Vec::new()));
            }
            deliver(&inbound, Message::Binary(vec![9]));
            inbound
        });
        assert_eq!(subscription.next_binary().await.unwrap(), vec![9]);
        assert_eq!(started.elapsed(), Duration::from_secs(225));
        drop(peer.await.unwrap());
    }

    #[tokio::test(start_paused = true)]
    async fn keepalive_send_failure_disconnects() {
        let (_inbound, outbound, mut subscription) = subscription();
        drop(outbound);
        assert!(matches!(
            subscription.next_binary().await,
            Err(Disconnect::Socket(WsError::ConnectionClosed))
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn blocked_keepalive_send_disconnects_at_the_idle_deadline() {
        let (_inbound, reader) = mpsc::unbounded::<Result<Message, WsError>>();
        let (writer, _outbound) = mpsc::channel::<Message>(0);
        let writer = writer.sink_map_err(closed as fn(mpsc::SendError) -> WsError);
        let mut subscription = Subscription::new(reader, writer, IDLE);
        let started = Instant::now();
        assert!(matches!(
            subscription.next_binary().await,
            Err(Disconnect::Idle(IDLE))
        ));
        assert_eq!(started.elapsed(), IDLE);
    }

    async fn listener() -> (TcpListener, Url) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("ws://{}", listener.local_addr().unwrap())).unwrap();
        (listener, url)
    }

    #[tokio::test]
    async fn connect_completes_the_handshake() {
        let (listener, url) = listener().await;
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            socket.send(Message::Binary(vec![7])).await.unwrap();
            socket
        });
        let socket = connect(&url, IDLE).await.unwrap();
        let (writer, reader) = socket.split();
        let mut subscription = Subscription::new(reader, writer, IDLE);
        assert_eq!(subscription.next_binary().await.unwrap(), vec![7]);
        drop(server.await.unwrap());
    }

    #[tokio::test]
    async fn connect_times_out_when_the_handshake_stalls() {
        let (_listener, url) = listener().await;
        let handshake_timeout = Duration::from_millis(200);
        let error = connect(&url, handshake_timeout).await.unwrap_err();
        assert!(matches!(error, ConnectError::Timeout(elapsed) if elapsed == handshake_timeout));
        assert_eq!(error.to_string(), "handshake did not complete within 0s");
    }

    #[tokio::test]
    async fn connect_reports_refused_connections() {
        let (listener, url) = listener().await;
        drop(listener);
        let error = connect(&url, IDLE).await.unwrap_err();
        assert!(matches!(error, ConnectError::Socket(WsError::Io(_))));
    }
}
