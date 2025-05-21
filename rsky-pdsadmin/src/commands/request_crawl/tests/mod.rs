#[cfg(test)]
mod tests {
    use crate::commands::request_crawl::execute;
    use mockito::{mock, server_url};
    use std::env;

    #[test]
    fn test_request_crawl_with_provided_hosts() {
        // Save original env vars
        let orig_hostname = env::var("PDS_HOSTNAME").ok();

        // Set test env vars
        env::set_var("PDS_HOSTNAME", "test.hostname.com");

        // Mock the API response
        let crawl_mock = mock("POST", "/xrpc/com.atproto.sync.requestCrawl")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("{}")
            .match_body(r#"{"hostname":"test.hostname.com"}"#)
            .create();

        // Execute the command with a host
        let result = execute(
            &server_url()
                .strip_prefix("http://")
                .unwrap_or("localhost:1234"),
        );

        // Verify mock was called
        crawl_mock.assert();

        // Check that the command executed successfully
        assert!(result.is_ok());

        // Restore original env vars
        match orig_hostname {
            Some(val) => env::set_var("PDS_HOSTNAME", val),
            None => env::remove_var("PDS_HOSTNAME"),
        }
    }

    #[test]
    fn test_request_crawl_with_env_hosts() {
        // Save original env vars
        let orig_hostname = env::var("PDS_HOSTNAME").ok();
        let orig_crawlers = env::var("PDS_CRAWLERS").ok();

        // Set test env vars
        env::set_var("PDS_HOSTNAME", "test.hostname.com");
        let server = server_url()
            .strip_prefix("http://")
            .unwrap_or("localhost:1234");
        env::set_var("PDS_CRAWLERS", server);

        // Mock the API response
        let crawl_mock = mock("POST", "/xrpc/com.atproto.sync.requestCrawl")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("{}")
            .match_body(r#"{"hostname":"test.hostname.com"}"#)
            .create();

        // Execute the command without a host (should use PDS_CRAWLERS)
        let result = execute("");

        // Verify mock was called
        crawl_mock.assert();

        // Check that the command executed successfully
        assert!(result.is_ok());

        // Restore original env vars
        match orig_hostname {
            Some(val) => env::set_var("PDS_HOSTNAME", val),
            None => env::remove_var("PDS_HOSTNAME"),
        }

        match orig_crawlers {
            Some(val) => env::set_var("PDS_CRAWLERS", val),
            None => env::remove_var("PDS_CRAWLERS"),
        }
    }
}