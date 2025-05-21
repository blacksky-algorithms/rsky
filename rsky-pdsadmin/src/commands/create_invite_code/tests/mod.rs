#[cfg(test)]
mod tests {
    use crate::commands::create_invite_code::execute;
    use mockito::{mock, server_url};
    use std::env;

    #[test]
    fn test_create_invite_code() {
        // Save original env vars
        let orig_hostname = env::var("PDS_HOSTNAME").ok();
        let orig_password = env::var("PDS_ADMIN_PASSWORD").ok();

        // Set test env vars
        env::set_var(
            "PDS_HOSTNAME",
            server_url()
                .strip_prefix("http://")
                .unwrap_or("localhost:1234"),
        );
        env::set_var("PDS_ADMIN_PASSWORD", "test-password");

        // Mock the API response
        let invite_mock = mock("POST", "/xrpc/com.atproto.server.createInviteCode")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"code":"test-invite-code"}"#)
            .create();

        // Execute the command
        let result = execute();

        // Verify mock was called
        invite_mock.assert();

        // Check that the command executed successfully
        assert!(result.is_ok());

        // Restore original env vars
        match orig_hostname {
            Some(val) => env::set_var("PDS_HOSTNAME", val),
            None => env::remove_var("PDS_HOSTNAME"),
        }

        match orig_password {
            Some(val) => env::set_var("PDS_ADMIN_PASSWORD", val),
            None => env::remove_var("PDS_ADMIN_PASSWORD"),
        }
    }
}