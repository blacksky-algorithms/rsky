//! Distributed tracing: bridges the existing `tracing` spans (117
//! `#[tracing::instrument]` call sites across the handler layer) to
//! OpenTelemetry, exported via OTLP.
//!
//! This is additive to the Prometheus metrics in [`crate::metrics`], not a
//! replacement: metrics stay on the `metrics` facade / `/metrics` endpoint,
//! this only covers spans. Modeled on the reference TypeScript PDS's
//! `telemetry.ts`, which wires the OTEL SDK the same way (there, via
//! `@atproto-labs/opentelemetry-node`).
//!
//! Opt-in and zero-cost when unconfigured: tracing only initializes, and
//! the OTLP exporter only gets built, when `OTEL_EXPORTER_OTLP_ENDPOINT` is
//! set. Deployments that don't run a collector pay nothing -- no exporter,
//! no background export thread, no failed-connection log spam.

use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use rsky_common::env::env_str;
use std::sync::OnceLock;

static TRACER_PROVIDER: OnceLock<SdkTracerProvider> = OnceLock::new();

/// Builds the OTEL tracer provider and returns a `tracing` layer that
/// forwards every span through it, or `None` if
/// `OTEL_EXPORTER_OTLP_ENDPOINT` isn't set.
///
/// The returned provider must be kept alive (and ideally flushed via
/// `shutdown()`) for the process lifetime; [`init`] handles that by setting
/// it as the OTEL global provider, which owns the batch exporter thread.
pub fn layer<S>() -> Option<impl tracing_subscriber::Layer<S>>
where
    S: tracing::Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    let endpoint = env_str("OTEL_EXPORTER_OTLP_ENDPOINT")?;

    let exporter = match opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(&endpoint)
        .build()
    {
        Ok(exporter) => exporter,
        Err(error) => {
            tracing::error!("Failed to build OTLP span exporter for {endpoint}: {error}");
            return None;
        }
    };

    let service_name = env_str("OTEL_SERVICE_NAME").unwrap_or_else(|| "rsky-pds".to_string());
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            Resource::builder()
                .with_attributes([KeyValue::new("service.name", service_name)])
                .build(),
        )
        .build();

    let tracer = provider.tracer("rsky-pds");
    // Keep our own handle for shutdown() -- this version of the `global`
    // module has no `shutdown_tracer_provider()`, and `set_tracer_provider`
    // takes the callee's provider by value, so the clone here is the only
    // way to flush on exit.
    let _ = TRACER_PROVIDER.set(provider.clone());
    opentelemetry::global::set_tracer_provider(provider);
    Some(tracing_opentelemetry::layer().with_tracer(tracer))
}

/// Flushes and shuts down the tracer provider installed by [`layer`], if
/// any. Best-effort: a failure here just means some in-flight spans may
/// not reach the collector, never a reason to fail shutdown.
pub fn shutdown() {
    if let Some(provider) = TRACER_PROVIDER.get() {
        if let Err(error) = provider.shutdown() {
            tracing::warn!("Error shutting down OTEL tracer provider: {error}");
        }
    }
}
