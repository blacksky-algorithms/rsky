use prometheus::{histogram_opts, opts, Histogram, IntCounter, IntGauge, Registry, TextEncoder};
use std::sync::LazyLock;

static REGISTRY: LazyLock<Registry> = LazyLock::new(Registry::new);

pub static GRAPH_QUERIES_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    let c = IntCounter::new(
        "graph_queries_total",
        "Total follows-following queries served",
    )
    .unwrap();
    REGISTRY.register(Box::new(c.clone())).unwrap();
    c
});

pub static GRAPH_QUERY_DURATION: LazyLock<Histogram> = LazyLock::new(|| {
    let h = Histogram::with_opts(histogram_opts!(
        "graph_query_duration_seconds",
        "Query duration in seconds",
        vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0]
    ))
    .unwrap();
    REGISTRY.register(Box::new(h.clone())).unwrap();
    h
});

pub static GRAPH_BLOOM_REJECTIONS: LazyLock<IntCounter> = LazyLock::new(|| {
    let c = IntCounter::new(
        "graph_bloom_rejections_total",
        "Queries short-circuited by bloom filter (definite no overlap)",
    )
    .unwrap();
    REGISTRY.register(Box::new(c.clone())).unwrap();
    c
});

pub static GRAPH_USERS_TOTAL: LazyLock<IntGauge> = LazyLock::new(|| {
    let g = IntGauge::with_opts(opts!("graph_users_total", "Total UIDs in graph")).unwrap();
    REGISTRY.register(Box::new(g.clone())).unwrap();
    g
});

pub static GRAPH_FOLLOWS_TOTAL: LazyLock<IntGauge> = LazyLock::new(|| {
    let g =
        IntGauge::with_opts(opts!("graph_follows_total", "Total follow edges in graph")).unwrap();
    REGISTRY.register(Box::new(g.clone())).unwrap();
    g
});

pub static GRAPH_FIREHOSE_EVENTS: LazyLock<IntCounter> = LazyLock::new(|| {
    let c = IntCounter::new(
        "graph_firehose_events_total",
        "Follow events processed from firehose",
    )
    .unwrap();
    REGISTRY.register(Box::new(c.clone())).unwrap();
    c
});

pub static GRAPH_FIREHOSE_FRAMES: LazyLock<IntCounter> = LazyLock::new(|| {
    let c = IntCounter::new(
        "graph_firehose_frames_total",
        "Binary frames received from firehose (any type)",
    )
    .unwrap();
    REGISTRY.register(Box::new(c.clone())).unwrap();
    c
});

pub static GRAPH_FIREHOSE_COMMITS: LazyLock<IntCounter> = LazyLock::new(|| {
    let c = IntCounter::new(
        "graph_firehose_commits_total",
        "#commit frames successfully decoded",
    )
    .unwrap();
    REGISTRY.register(Box::new(c.clone())).unwrap();
    c
});

pub static GRAPH_FIREHOSE_DECODE_ERRORS: LazyLock<IntCounter> = LazyLock::new(|| {
    let c = IntCounter::new(
        "graph_firehose_decode_errors_total",
        "Frames where rsky_firehose::firehose::read returned Err or non-commit",
    )
    .unwrap();
    REGISTRY.register(Box::new(c.clone())).unwrap();
    c
});

pub fn encode_metrics() -> String {
    // Force lazy initialization
    let _ = &*GRAPH_QUERIES_TOTAL;
    let _ = &*GRAPH_QUERY_DURATION;
    let _ = &*GRAPH_BLOOM_REJECTIONS;
    let _ = &*GRAPH_USERS_TOTAL;
    let _ = &*GRAPH_FOLLOWS_TOTAL;
    let _ = &*GRAPH_FIREHOSE_EVENTS;
    let _ = &*GRAPH_FIREHOSE_FRAMES;
    let _ = &*GRAPH_FIREHOSE_COMMITS;
    let _ = &*GRAPH_FIREHOSE_DECODE_ERRORS;

    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buffer = String::new();
    encoder.encode_utf8(&metric_families, &mut buffer).unwrap();
    buffer
}
