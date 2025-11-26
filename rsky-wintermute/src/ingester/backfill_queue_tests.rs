#[cfg(test)]
mod tests {
    use crate::ingester::backfill_queue::populate_backfill_queue;
    use crate::storage::Storage;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn setup_test_storage() -> (Arc<Storage>, TempDir) {
        let temp_dir = TempDir::with_prefix("backfill_queue_test_").unwrap();
        let db_path = temp_dir.path().join("test_db");
        let storage = Arc::new(Storage::new(Some(db_path)).unwrap());
        (storage, temp_dir)
    }

    #[tokio::test]
    async fn test_populate_backfill_queue_single_page() {
        let (storage, _dir) = setup_test_storage();

        let mut server = mockito::Server::new_async().await;

        // Mock response with 3 repos, no cursor
        let response_json = serde_json::json!({
            "repos": [
                {"did": "did:plc:test1"},
                {"did": "did:plc:test2"},
                {"did": "did:plc:test3"}
            ],
            "cursor": null
        });

        let _mock = server
            .mock("GET", "/xrpc/com.atproto.sync.listRepos")
            .match_query(mockito::Matcher::UrlEncoded("limit".into(), "1000".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(response_json.to_string())
            .create_async()
            .await;

        let result = populate_backfill_queue(storage.clone(), server.url()).await;
        drop(server);

        assert!(
            result.is_ok(),
            "populate_backfill_queue should succeed: {result:?}"
        );

        // Verify 3 repos were enqueued
        let queue_len = storage.repo_backfill_len().unwrap();
        assert_eq!(queue_len, 3, "expected 3 repos in backfill queue");

        // Verify we can dequeue them (order may not be preserved by fjall)
        let mut found_dids = Vec::new();
        for _ in 0..3 {
            let (key, job) = storage.dequeue_backfill().unwrap().unwrap();
            found_dids.push(job.did);
            storage.remove_backfill(&key).unwrap();
        }
        found_dids.sort();
        assert_eq!(
            found_dids,
            vec!["did:plc:test1", "did:plc:test2", "did:plc:test3"]
        );
    }

    #[tokio::test]
    async fn test_populate_backfill_queue_multiple_pages() {
        let (storage, _dir) = setup_test_storage();

        let mut server = mockito::Server::new_async().await;

        // First page response
        let page1_json = serde_json::json!({
            "repos": [
                {"did": "did:plc:page1_repo1"},
                {"did": "did:plc:page1_repo2"}
            ],
            "cursor": "cursor_page2"
        });

        let _mock1 = server
            .mock("GET", "/xrpc/com.atproto.sync.listRepos")
            .match_query(mockito::Matcher::UrlEncoded("limit".into(), "1000".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(page1_json.to_string())
            .expect(1)
            .create_async()
            .await;

        // Second page response (no cursor = end)
        let page2_json = serde_json::json!({
            "repos": [
                {"did": "did:plc:page2_repo1"}
            ],
            "cursor": null
        });

        let _mock2 = server
            .mock("GET", "/xrpc/com.atproto.sync.listRepos")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("limit".into(), "1000".into()),
                mockito::Matcher::UrlEncoded("cursor".into(), "cursor_page2".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(page2_json.to_string())
            .create_async()
            .await;

        let result = populate_backfill_queue(storage.clone(), server.url()).await;
        drop(server);

        assert!(
            result.is_ok(),
            "populate_backfill_queue should succeed: {result:?}"
        );

        // Verify all 3 repos were enqueued
        let queue_len = storage.repo_backfill_len().unwrap();
        assert_eq!(queue_len, 3, "expected 3 repos total from 2 pages");
    }

    #[tokio::test]
    async fn test_populate_backfill_queue_http_error() {
        let (storage, _dir) = setup_test_storage();

        let mut server = mockito::Server::new_async().await;

        // Mock 500 error
        let _mock = server
            .mock("GET", "/xrpc/com.atproto.sync.listRepos")
            .with_status(500)
            .create_async()
            .await;

        let result = populate_backfill_queue(storage.clone(), server.url()).await;
        drop(server);

        assert!(result.is_err(), "should fail with HTTP 500");
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("http error") || err_msg.contains("500"));
    }

    #[tokio::test]
    async fn test_populate_backfill_queue_empty_response() {
        let (storage, _dir) = setup_test_storage();

        let mut server = mockito::Server::new_async().await;

        // Mock response with no repos
        let response_json = serde_json::json!({
            "repos": [],
            "cursor": null
        });

        let _mock = server
            .mock("GET", "/xrpc/com.atproto.sync.listRepos")
            .match_query(mockito::Matcher::UrlEncoded("limit".into(), "1000".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(response_json.to_string())
            .create_async()
            .await;

        let result = populate_backfill_queue(storage.clone(), server.url()).await;
        drop(server);

        assert!(
            result.is_ok(),
            "should succeed with empty repos: {result:?}"
        );

        // Verify queue is empty
        let queue_len = storage.repo_backfill_len().unwrap();
        assert_eq!(queue_len, 0, "queue should be empty");
    }

    #[tokio::test]
    async fn test_populate_backfill_queue_cursor_persistence() {
        let (storage, _dir) = setup_test_storage();

        let mut server = mockito::Server::new_async().await;

        // First page with cursor
        let page1_json = serde_json::json!({
            "repos": [{"did": "did:plc:test1"}],
            "cursor": "123456"
        });

        let _mock1 = server
            .mock("GET", "/xrpc/com.atproto.sync.listRepos")
            .match_query(mockito::Matcher::UrlEncoded("limit".into(), "1000".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(page1_json.to_string())
            .expect(1)
            .create_async()
            .await;

        // Second page (end)
        let page2_json = serde_json::json!({
            "repos": [],
            "cursor": null
        });

        let _mock2 = server
            .mock("GET", "/xrpc/com.atproto.sync.listRepos")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("limit".into(), "1000".into()),
                mockito::Matcher::UrlEncoded("cursor".into(), "123456".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(page2_json.to_string())
            .create_async()
            .await;

        let server_url = server.url();
        let result = populate_backfill_queue(storage.clone(), server_url.clone()).await;
        drop(server);

        assert!(
            result.is_ok(),
            "populate_backfill_queue should succeed: {result:?}"
        );

        // Verify cursor was stored
        let cursor_key = format!("backfill_enum:{server_url}");
        let stored_cursor = storage.get_cursor(&cursor_key).unwrap();
        assert!(stored_cursor.is_some(), "cursor should be stored");
    }
}
