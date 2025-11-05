use std::fs;
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

// Helper function to create test files
fn create_test_file(dir: &TempDir, name: &str, content: &[u8]) -> PathBuf {
    let file_path = dir.path().join(name);
    let mut file = fs::File::create(&file_path).unwrap();
    file.write_all(content).unwrap();
    file_path
}

#[test]
fn test_create_temp_directory() {
    let temp_dir = TempDir::new().unwrap();
    assert!(temp_dir.path().exists());
    assert!(temp_dir.path().is_dir());
}

#[test]
fn test_create_and_read_file() {
    let temp_dir = TempDir::new().unwrap();
    let content = b"Hello, World!";
    let file_path = create_test_file(&temp_dir, "test.txt", content);

    assert!(file_path.exists());
    let read_content = fs::read(&file_path).unwrap();
    assert_eq!(read_content, content);
}

#[test]
fn test_multiple_files_in_directory() {
    let temp_dir = TempDir::new().unwrap();

    create_test_file(&temp_dir, "file1.txt", b"Content 1");
    create_test_file(&temp_dir, "file2.txt", b"Content 2");
    create_test_file(&temp_dir, "file3.txt", b"Content 3");

    let entries: Vec<_> = fs::read_dir(temp_dir.path())
        .unwrap()
        .map(|e| e.unwrap())
        .collect();

    assert_eq!(entries.len(), 3);
}

#[test]
fn test_empty_file() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = create_test_file(&temp_dir, "empty.txt", b"");

    let metadata = fs::metadata(&file_path).unwrap();
    assert_eq!(metadata.len(), 0);
}

#[test]
fn test_large_file() {
    let temp_dir = TempDir::new().unwrap();
    // Create a 1MB file
    let content = vec![0xAB; 1024 * 1024];
    let file_path = create_test_file(&temp_dir, "large.bin", &content);

    let metadata = fs::metadata(&file_path).unwrap();
    assert_eq!(metadata.len(), 1024 * 1024);
}

#[test]
fn test_file_metadata() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = create_test_file(&temp_dir, "meta.txt", b"test data");

    let metadata = fs::metadata(&file_path).unwrap();
    assert!(metadata.is_file());
    assert!(!metadata.is_dir());
    assert_eq!(metadata.len(), 9); // "test data" is 9 bytes
}

#[test]
fn test_directory_creation() {
    let temp_dir = TempDir::new().unwrap();
    let sub_dir = temp_dir.path().join("subdir");

    fs::create_dir(&sub_dir).unwrap();
    assert!(sub_dir.exists());
    assert!(sub_dir.is_dir());
}

#[test]
fn test_nested_directory_structure() {
    let temp_dir = TempDir::new().unwrap();

    let sub_dir1 = temp_dir.path().join("dir1");
    let sub_dir2 = sub_dir1.join("dir2");

    fs::create_dir(&sub_dir1).unwrap();
    fs::create_dir(&sub_dir2).unwrap();

    let file_path = sub_dir2.join("nested.txt");
    let mut file = fs::File::create(&file_path).unwrap();
    file.write_all(b"nested content").unwrap();

    assert!(file_path.exists());
}

#[test]
fn test_binary_file_content() {
    let temp_dir = TempDir::new().unwrap();
    let binary_content: Vec<u8> = vec![0x00, 0xFF, 0xDE, 0xAD, 0xBE, 0xEF];
    let file_path = create_test_file(&temp_dir, "binary.dat", &binary_content);

    let read_content = fs::read(&file_path).unwrap();
    assert_eq!(read_content, binary_content);
}

#[test]
fn test_identical_files_different_locations() {
    let temp_dir = TempDir::new().unwrap();
    let content = b"Identical content for deduplication test";

    let file1 = create_test_file(&temp_dir, "file1.txt", content);
    let file2 = create_test_file(&temp_dir, "file2.txt", content);

    let content1 = fs::read(&file1).unwrap();
    let content2 = fs::read(&file2).unwrap();

    assert_eq!(content1, content2);
    assert_ne!(file1, file2); // Different paths
}

#[test]
fn test_file_with_special_chars() {
    let temp_dir = TempDir::new().unwrap();
    let content = b"Special chars: \n\r\t\x00\xFF";
    let file_path = create_test_file(&temp_dir, "special.txt", content);

    let read_content = fs::read(&file_path).unwrap();
    assert_eq!(read_content, content);
}

#[test]
#[cfg(unix)]
fn test_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new().unwrap();
    let file_path = create_test_file(&temp_dir, "perms.txt", b"test");

    let mut perms = fs::metadata(&file_path).unwrap().permissions();
    perms.set_mode(0o644);
    fs::set_permissions(&file_path, perms).unwrap();

    let metadata = fs::metadata(&file_path).unwrap();
    let mode = metadata.permissions().mode();
    // Check the lower 9 bits (rwxrwxrwx)
    assert_eq!(mode & 0o777, 0o644);
}

#[test]
fn test_two_directory_setup() {
    let temp_dir1 = TempDir::new().unwrap();
    let temp_dir2 = TempDir::new().unwrap();

    create_test_file(&temp_dir1, "shared.txt", b"shared content");
    create_test_file(&temp_dir2, "shared.txt", b"shared content");

    let file1 = temp_dir1.path().join("shared.txt");
    let file2 = temp_dir2.path().join("shared.txt");

    let content1 = fs::read(&file1).unwrap();
    let content2 = fs::read(&file2).unwrap();

    assert_eq!(content1, content2);
}

#[test]
fn test_file_modification_detection() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = create_test_file(&temp_dir, "modified.txt", b"original");

    let metadata1 = fs::metadata(&file_path).unwrap();
    let mtime1 = metadata1.modified().unwrap();

    // Sleep to ensure different timestamp
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Modify the file
    let mut file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&file_path)
        .unwrap();
    file.write_all(b"modified").unwrap();
    drop(file);

    let metadata2 = fs::metadata(&file_path).unwrap();
    let mtime2 = metadata2.modified().unwrap();

    assert!(mtime2 > mtime1, "Modified time should be later");
}
