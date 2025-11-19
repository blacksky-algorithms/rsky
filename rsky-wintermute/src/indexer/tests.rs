#[cfg(test)]
mod indexer_tests {
    use crate::types::WriteAction;

    #[test]
    fn test_write_action_serialization() {
        let create = WriteAction::Create;
        let json = serde_json::to_string(&create).unwrap();
        assert!(json.contains("Create"));

        let update = WriteAction::Update;
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("Update"));

        let delete = WriteAction::Delete;
        let json = serde_json::to_string(&delete).unwrap();
        assert!(json.contains("Delete"));
    }

    #[test]
    fn test_uri_parsing() {
        let uri = "at://did:plc:test/app.bsky.feed.post/123";
        let parts: Vec<&str> = uri.strip_prefix("at://").unwrap().split('/').collect();

        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "did:plc:test");
        assert_eq!(parts[1], "app.bsky.feed.post");
        assert_eq!(parts[2], "123");
    }
}
