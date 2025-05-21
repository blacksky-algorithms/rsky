#[cfg(test)]
mod tests {
    use crate::commands::update::{compare_files, COMPOSE_URL};
    use std::fs::{self, File};
    use std::io::Write;
    use std::path::Path;
    use tempfile::TempDir;

    // Helper function to create a mock compose.yaml file
    fn create_mock_compose_file(dir: &Path, content: &str) -> std::io::Result<()> {
        let file_path = dir.join("compose.yaml");
        let mut file = File::create(file_path)?;
        file.write_all(content.as_bytes())?;
        Ok(())
    }

    #[test]
    fn test_compare_files() {
        // Create a temporary directory for testing
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create two identical files
        let content = "name: test-compose\nversion: '3'\nservices:\n  test:\n    image: test:latest";
        let file1 = temp_path.join("file1.yaml");
        let file2 = temp_path.join("file2.yaml");

        fs::write(&file1, content).unwrap();
        fs::write(&file2, content).unwrap();

        // Files should be identical
        let result = compare_files(file1.to_str().unwrap(), file2.to_str().unwrap()).unwrap();
        assert!(result);

        // Now create a different file
        let different_content = "name: test-compose\nversion: '3'\nservices:\n  test:\n    image: test:1.1";
        fs::write(&file2, different_content).unwrap();

        // Files should not be identical
        let result = compare_files(file1.to_str().unwrap(), file2.to_str().unwrap()).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_compose_url_is_valid() {
        // Ensure the COMPOSE_URL constant is set to a reasonable value
        assert!(COMPOSE_URL.starts_with("https://"));
        assert!(COMPOSE_URL.contains("blacksky-algorithms"));
        assert!(COMPOSE_URL.contains("compose.yaml"));
    }
}