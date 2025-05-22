#[cfg(test)]
mod tests {
    use crate::commands::account::{AccountCommands, execute};
    use mockito::{mock, server_url};
    use std::env;

    #[test]
    fn test_list_accounts() {
        let _env_guard = set_test_env();

        // Mock the API response for listRepos
        let repos_mock = mock("GET", "/xrpc/com.atproto.sync.listRepos?limit=100")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"repos":[{"did":"did:plc:test1"},{"did":"did:plc:test2"}]}"#)
            .create();

        // Mock the API response for getAccountInfo
        let account1_mock = mock(
            "GET",
            "/xrpc/com.atproto.admin.getAccountInfo?did=did:plc:test1",
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"handle":"user1.test","email":"user1@example.com","did":"did:plc:test1"}"#)
        .create();

        let account2_mock = mock(
            "GET",
            "/xrpc/com.atproto.admin.getAccountInfo?did=did:plc:test2",
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"handle":"user2.test","email":"user2@example.com","did":"did:plc:test2"}"#)
        .create();

        // Execute the list command
        let result = execute(&AccountCommands::List);

        // Verify mocks were called
        repos_mock.assert();
        account1_mock.assert();
        account2_mock.assert();

        // Check that the command executed successfully
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_account() {
        let _env_guard = set_test_env();

        // Mock the API response for createInviteCode
        let invite_mock = mock("POST", "/xrpc/com.atproto.server.createInviteCode")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"code":"test-invite-code"}"#)
            .create();

        // Mock the API response for createAccount
        let account_mock = mock("POST", "/xrpc/com.atproto.server.createAccount")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"did":"did:plc:newuser","handle":"newuser.test"}"#)
            .create();

        // Execute the create command
        let result = execute(&AccountCommands::Create {
            email: "newuser@example.com".to_string(),
            handle: "newuser.test".to_string(),
        });

        // Verify mocks were called
        invite_mock.assert();
        account_mock.assert();

        // Check that the command executed successfully
        assert!(result.is_ok());
    }

    #[test]
    fn test_reset_password() {
        let _env_guard = set_test_env();

        // Mock the API response for updateAccountPassword
        let password_mock = mock("POST", "/xrpc/com.atproto.admin.updateAccountPassword")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("{}")
            .create();

        // Execute the reset-password command
        let result = execute(&AccountCommands::ResetPassword {
            did: "did:plc:test".to_string(),
        });

        // Verify mocks were called
        password_mock.assert();

        // Check that the command executed successfully
        assert!(result.is_ok());
    }

    // Helper to set up test environment
    fn set_test_env() -> impl Drop {
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

        // Return a guard that will restore env vars when dropped
        EnvGuard {
            orig_hostname,
            orig_password,
        }
    }

    struct EnvGuard {
        orig_hostname: Option<String>,
        orig_password: Option<String>,
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // Restore original env vars
            match &self.orig_hostname {
                Some(val) => env::set_var("PDS_HOSTNAME", val),
                None => env::remove_var("PDS_HOSTNAME"),
            }

            match &self.orig_password {
                Some(val) => env::set_var("PDS_ADMIN_PASSWORD", val),
                None => env::remove_var("PDS_ADMIN_PASSWORD"),
            }
        }
    }
}