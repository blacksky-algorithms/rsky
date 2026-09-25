use rsky_wintermute::indexer::{IndexerManager, disable_notifications};
use rsky_wintermute::types::{IndexJob, WriteAction};
use serde_json::json;

fn job(uri: String, record: serde_json::Value) -> IndexJob {
    IndexJob {
        uri,
        cid: "bafynonotif".to_owned(),
        action: WriteAction::Create,
        record: Some(record),
        indexed_at: chrono::Utc::now().to_rfc3339(),
        rev: "3mwnonotif0001".to_owned(),
        provenance: None,
    }
}

#[tokio::test]
async fn test_disabled_notifications_write_no_rows() {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5432/bsky_test".to_owned());
    let pool = rsky_wintermute::config::create_pg_pool(
        &database_url,
        rsky_wintermute::config::pg_pool_config(4),
    )
    .unwrap();
    let did = "did:plc:nonotifauthor";
    let other = "did:plc:nonotifrecipient";
    let parent = format!("at://{other}/app.bsky.feed.post/parent1");
    let client = pool.get().await.unwrap();
    for sql in [
        "DELETE FROM notification WHERE author = $1",
        "DELETE FROM post WHERE creator = $1",
        "DELETE FROM \"like\" WHERE creator = $1",
        "DELETE FROM follow WHERE creator = $1",
        "DELETE FROM quote WHERE uri LIKE 'at://' || $1 || '/%'",
        "DELETE FROM record WHERE did = $1",
    ] {
        client.execute(sql, &[&did]).await.unwrap();
    }
    client
        .execute(
            "INSERT INTO post (uri, cid, creator, text, \"createdAt\", \"indexedAt\")
             VALUES ($1, 'bafyparent', $2, 'parent', '2025-07-17T00:00:00Z', '2025-07-17T00:00:00Z')
             ON CONFLICT DO NOTHING",
            &[&parent, &other],
        )
        .await
        .unwrap();

    disable_notifications();

    let post_ref = json!({"uri": parent, "cid": "bafyparent"});
    let jobs = [
        job(
            format!("at://{did}/app.bsky.feed.post/reply1"),
            json!({
                "text": "hi @recipient",
                "createdAt": "2025-07-17T00:01:00Z",
                "reply": {"root": post_ref, "parent": post_ref},
                "embed": {"$type": "app.bsky.embed.record", "record": post_ref},
                "facets": [{
                    "index": {"byteStart": 3, "byteEnd": 13},
                    "features": [{"$type": "app.bsky.richtext.facet#mention", "did": other}]
                }]
            }),
        ),
        job(
            format!("at://{did}/app.bsky.feed.like/like1"),
            json!({"subject": post_ref, "createdAt": "2025-07-17T00:02:00Z"}),
        ),
        job(
            format!("at://{did}/app.bsky.graph.follow/follow1"),
            json!({"subject": other, "createdAt": "2025-07-17T00:03:00Z"}),
        ),
    ];
    for j in &jobs {
        IndexerManager::process_job(&pool, j, false)
            .await
            .expect("index succeeds");
    }

    let indexed: i64 = client
        .query_one(
            "SELECT (SELECT COUNT(*) FROM post WHERE creator = $1)
                  + (SELECT COUNT(*) FROM \"like\" WHERE creator = $1)
                  + (SELECT COUNT(*) FROM follow WHERE creator = $1)
                  + (SELECT COUNT(*) FROM quote WHERE uri LIKE 'at://' || $1 || '/%')",
            &[&did],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(
        indexed, 4,
        "post, like, follow and quote rows are still written"
    );

    let notifications: i64 = client
        .query_one(
            "SELECT COUNT(*) FROM notification WHERE author = $1",
            &[&did],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(notifications, 0);
}
