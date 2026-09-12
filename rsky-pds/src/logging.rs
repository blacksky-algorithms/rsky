//! Log output shaped like the reference PDS's, so the same shipping rules
//! and dashboards read both: one JSON object per line with pino's numeric
//! levels, `time` in milliseconds, `pid`, `hostname`, `name`, and `msg`,
//! with every structured field alongside and dotted field names nested.

use serde_json::{Map, Value};
use std::fmt;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Json,
    Text,
}

impl LogFormat {
    pub fn from_env() -> Self {
        match std::env::var("PDS_LOG_FORMAT").as_deref() {
            Ok("text") => LogFormat::Text,
            _ => LogFormat::Json,
        }
    }
}

/// pino's numeric levels.
pub fn pino_level(level: &Level) -> u8 {
    match *level {
        Level::TRACE => 10,
        Level::DEBUG => 20,
        Level::INFO => 30,
        Level::WARN => 40,
        Level::ERROR => 50,
    }
}

fn hostname() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer outlives the call and its length is passed
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
    if rc != 0 {
        return "unknown".to_string();
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).to_string()
}

/// Collects an event's fields into a JSON object, nesting `a.b` under `a`.
#[derive(Default)]
struct JsonVisitor {
    fields: Map<String, Value>,
    message: Option<String>,
}

impl JsonVisitor {
    fn insert(&mut self, name: &str, value: Value) {
        if name == "message" {
            self.message = Some(match value {
                Value::String(text) => text,
                other => other.to_string(),
            });
            return;
        }
        insert_path(&mut self.fields, name, value);
    }
}

fn insert_path(object: &mut Map<String, Value>, path: &str, value: Value) {
    match path.split_once('.') {
        None => {
            object.insert(path.to_string(), value);
        }
        Some((head, rest)) => {
            let child = object
                .entry(head.to_string())
                .or_insert_with(|| Value::Object(Map::new()));
            if !child.is_object() {
                *child = Value::Object(Map::new());
            }
            insert_path(child.as_object_mut().expect("object"), rest, value);
        }
    }
}

impl Visit for JsonVisitor {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.insert(field.name(), Value::from(value));
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.insert(field.name(), Value::from(value));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.insert(field.name(), Value::from(value));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.insert(field.name(), Value::from(value));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.insert(field.name(), Value::from(value));
    }
    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.insert(field.name(), Value::from(value.to_string()));
    }
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.insert(field.name(), Value::from(format!("{value:?}")));
    }
}

pub struct PinoFormat {
    pid: u32,
    hostname: String,
    name: &'static str,
}

impl PinoFormat {
    pub fn new(name: &'static str) -> Self {
        PinoFormat {
            pid: std::process::id(),
            hostname: hostname(),
            name,
        }
    }

    /// The JSON line for an event, without the trailing newline.
    fn line(&self, event: &Event<'_>, time_ms: u128) -> String {
        let mut visitor = JsonVisitor::default();
        event.record(&mut visitor);
        let mut object = Map::new();
        object.insert("level".into(), pino_level(event.metadata().level()).into());
        object.insert("time".into(), Value::from(time_ms as u64));
        object.insert("pid".into(), self.pid.into());
        object.insert("hostname".into(), self.hostname.clone().into());
        object.insert("name".into(), self.name.into());
        object.insert("target".into(), event.metadata().target().into());
        for (key, value) in visitor.fields {
            object.insert(key, value);
        }
        object.insert("msg".into(), visitor.message.unwrap_or_default().into());
        Value::Object(object).to_string()
    }
}

impl<S, N> FormatEvent<S, N> for PinoFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let time_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        writeln!(writer, "{}", self.line(event, time_ms))
    }
}

/// Installs the process-wide subscriber: `RUST_LOG` selects the level
/// (default `info`), `PDS_LOG_FORMAT` selects `json` (default) or `text`.
pub fn init(format: LogFormat) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    let installed = match format {
        LogFormat::Json => builder.event_format(PinoFormat::new("pds")).try_init(),
        LogFormat::Text => builder.try_init(),
    };
    if let Err(err) = installed {
        eprintln!("logging already initialised: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Sink {
        type Writer = Sink;
        fn make_writer(&'a self) -> Sink {
            self.clone()
        }
    }

    #[test]
    fn lines_are_pino_shaped_with_nested_fields() {
        let sink = Sink::default();
        let subscriber = tracing_subscriber::fmt()
            .event_format(PinoFormat::new("pds"))
            .with_writer(sink.clone())
            .with_max_level(Level::TRACE)
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(
                req.id = 7u64,
                req.method = "GET",
                res.statusCode = 200i64,
                responseTime = 1.5f64,
                cached = true,
                err = %std::io::Error::other("boom"),
                detail = ?vec![1, 2],
                "request completed"
            );
            tracing::warn!("plain");
            let failure: Box<dyn std::error::Error + 'static> = "broken".into();
            tracing::error!(nested.deep.key = "x", failure = failure.as_ref(), "deep");
            tracing::info!(message = 42u64);
            tracing::debug!("debug");
            tracing::trace!("trace");
        });
        let output = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<Value> = output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 6, "{output}");
        assert_eq!(lines[2]["failure"], "broken");
        assert_eq!(lines[3]["msg"], "42");
        assert_eq!(lines[3]["level"], 30);
        let request = &lines[0];
        assert_eq!(request["level"], 30);
        assert_eq!(request["name"], "pds");
        assert_eq!(request["msg"], "request completed");
        assert_eq!(request["req"]["id"], 7);
        assert_eq!(request["req"]["method"], "GET");
        assert_eq!(request["res"]["statusCode"], 200);
        assert_eq!(request["responseTime"], 1.5);
        assert_eq!(request["cached"], true);
        assert_eq!(request["err"], "boom");
        assert_eq!(request["detail"], "[1, 2]");
        assert_eq!(request["pid"], std::process::id());
        assert!(request["hostname"].as_str().is_some_and(|h| !h.is_empty()));
        assert!(request["time"]
            .as_u64()
            .is_some_and(|t| t > 1_600_000_000_000));
        assert_eq!(lines[1]["level"], 40);
        assert_eq!(lines[2]["level"], 50);
        assert_eq!(lines[2]["nested"]["deep"]["key"], "x");
        assert_eq!(lines[4]["level"], 20);
        assert_eq!(lines[5]["level"], 10);
        std::io::Write::flush(&mut sink.clone()).unwrap();
    }

    #[test]
    fn a_scalar_field_gives_way_to_a_nested_one() {
        let mut object = Map::new();
        insert_path(&mut object, "req", Value::from(1));
        insert_path(&mut object, "req.id", Value::from(2));
        assert_eq!(Value::Object(object)["req"]["id"], 2);
    }

    #[test]
    fn format_and_hostname_come_from_the_environment() {
        std::env::set_var("PDS_LOG_FORMAT", "text");
        assert_eq!(LogFormat::from_env(), LogFormat::Text);
        std::env::set_var("PDS_LOG_FORMAT", "json");
        assert_eq!(LogFormat::from_env(), LogFormat::Json);
        std::env::remove_var("PDS_LOG_FORMAT");
        assert_eq!(LogFormat::from_env(), LogFormat::Json);
        assert!(!hostname().is_empty());
        // installing twice is reported, not fatal
        init(LogFormat::Text);
        init(LogFormat::Json);
    }
}
