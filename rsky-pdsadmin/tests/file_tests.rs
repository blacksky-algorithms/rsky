#[cfg(test)]
mod file_tests {
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_file_operations() {
        // Create a temporary directory for testing
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create two test files
        let file1_path = temp_path.join("file1.txt");
        let file2_path = temp_path.join("file2.txt");

        // Write content to the files
        fs::write(&file1_path, "Test content 1").unwrap();
        fs::write(&file2_path, "Test content 2").unwrap();

        // Read back and verify
        let content1 = fs::read_to_string(&file1_path).unwrap();
        let content2 = fs::read_to_string(&file2_path).unwrap();

        assert_eq!(content1, "Test content 1");
        assert_eq!(content2, "Test content 2");
        assert_ne!(content1, content2);
    }
}
