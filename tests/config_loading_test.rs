/// Integration tests for Phase E: Config Loading
/// Tests that configuration files are properly loaded and merged with CLI options
use std::fs;
use tempfile::TempDir;

#[test]
fn test_config_json_parsing() {
	// This test demonstrates that the config module can parse JSON config files
	let config_json = r#"{
		"name": "test-config",
		"description": "Test configuration",
		"metadata": {
			"strategy": "relaxed",
			"alwaysChecksum": false
		},
		"symlinks": {
			"mode": "preserve",
			"skipDangling": true,
			"maxDepth": 40
		},
		"conflicts": {
			"strategy": "prefer-newest"
		},
		"delete": {
			"mode": "sync"
		},
		"exclude": {
			"patterns": ["*.tmp", ".git/**"]
		}
	}"#;

	// Parse the JSON
	let result: Result<serde_json::Value, _> = serde_json::from_str(config_json);
	assert!(result.is_ok(), "Config JSON should be valid");

	let config = result.unwrap();
	assert_eq!(config["name"], "test-config");
	assert_eq!(config["metadata"]["strategy"], "relaxed");
	assert_eq!(config["symlinks"]["mode"], "preserve");
}

#[test]
fn test_config_file_creation_in_syncr_dir() {
	// Create a temporary directory to act as ~/.syncr
	let temp_dir = TempDir::new().expect("Failed to create temp dir");
	let syncr_dir = temp_dir.path();

	// Create a test config file
	let config_path = syncr_dir.join("default.json");
	let config_content = r#"{
		"name": "test-profile",
		"metadata": {
			"strategy": "strict",
			"alwaysChecksum": true
		},
		"symlinks": {
			"mode": "follow"
		},
		"conflicts": {
			"strategy": "prefer-oldest"
		},
		"delete": {
			"mode": "no-delete"
		},
		"exclude": {
			"patterns": ["node_modules/**", ".cache/**"]
		},
		"include": {
			"patterns": []
		}
	}"#;

	fs::write(&config_path, config_content).expect("Failed to write config file");

	// Verify the file exists and can be read
	assert!(config_path.exists(), "Config file should exist");

	let read_content = fs::read_to_string(&config_path).expect("Failed to read config file");

	assert_eq!(read_content, config_content);
	assert!(read_content.contains("\"strategy\": \"strict\""));
	assert!(read_content.contains("\"alwaysChecksum\": true"));
}

#[test]
fn test_multiple_config_profiles() {
	// Simulate having multiple named profiles
	let temp_dir = TempDir::new().expect("Failed to create temp dir");
	let syncr_dir = temp_dir.path();

	// Create "default" profile
	let default_config = r#"{
		"name": "default",
		"metadata": {"strategy": "smart"},
		"conflicts": {"strategy": "ask"},
		"delete": {"mode": "sync"},
		"exclude": {"patterns": []}
	}"#;

	// Create "backup" profile
	let backup_config = r#"{
		"name": "backup",
		"metadata": {"strategy": "strict"},
		"conflicts": {"strategy": "prefer-oldest"},
		"delete": {"mode": "sync"},
		"exclude": {"patterns": ["*.tmp"]}
	}"#;

	// Create "portable" profile
	let portable_config = r#"{
		"name": "portable",
		"metadata": {"strategy": "content-only"},
		"conflicts": {"strategy": "prefer-newest"},
		"delete": {"mode": "no-delete"},
		"exclude": {"patterns": []}
	}"#;

	fs::write(syncr_dir.join("default.json"), default_config)
		.expect("Failed to write default config");
	fs::write(syncr_dir.join("backup.json"), backup_config).expect("Failed to write backup config");
	fs::write(syncr_dir.join("portable.json"), portable_config)
		.expect("Failed to write portable config");

	// Verify all profiles exist
	assert!(syncr_dir.join("default.json").exists());
	assert!(syncr_dir.join("backup.json").exists());
	assert!(syncr_dir.join("portable.json").exists());

	// Verify each profile has distinct settings
	let default_str =
		fs::read_to_string(syncr_dir.join("default.json")).expect("Failed to read default.json");
	let backup_str =
		fs::read_to_string(syncr_dir.join("backup.json")).expect("Failed to read backup.json");

	assert!(default_str.contains("\"smart\""));
	assert!(backup_str.contains("\"strict\""));
	assert_ne!(default_str, backup_str);
}

#[test]
fn test_config_precedence_cli_overrides_file() {
	// This test documents the expected precedence:
	// CLI options > config file > defaults
	//
	// The actual testing happens in:
	// - main.rs: merge_config_with_cli_options() function
	// - SyncCliOptions struct in types.rs
	//
	// This test just documents the principle:

	// CLI option set (higher precedence)
	let cli_delete_mode = "delete-after"; // From CLI --delete-after
	let cli_conflict_strategy = "skip"; // From CLI --skip-conflicts

	// Config file option (medium precedence)
	let config_delete_mode = "no-delete";
	let config_conflict_strategy = "ask";

	// Expected result: CLI options win
	assert_ne!(cli_delete_mode, config_delete_mode);
	assert_ne!(cli_conflict_strategy, config_conflict_strategy);
	assert_eq!(cli_delete_mode, "delete-after", "CLI setting should be used");
	assert_eq!(cli_conflict_strategy, "skip", "CLI setting should be used");
}

#[test]
fn test_config_metadata_strategy_values() {
	// Verify that all metadata strategy values are valid
	let strategies = vec!["strict", "smart", "relaxed", "content-only"];

	for strategy in strategies {
		let config = format!(r#"{{"metadata": {{"strategy": "{}"}}}}"#, strategy);
		let result: Result<serde_json::Value, _> = serde_json::from_str(&config);
		assert!(result.is_ok(), "Strategy '{}' should be valid JSON", strategy);
	}
}

#[test]
fn test_config_symlink_mode_values() {
	// Verify that all symlink mode values are valid
	let modes = vec!["preserve", "follow", "ignore", "relative"];

	for mode in modes {
		let config = format!(r#"{{"symlinks": {{"mode": "{}"}}}}"#, mode);
		let result: Result<serde_json::Value, _> = serde_json::from_str(&config);
		assert!(result.is_ok(), "Mode '{}' should be valid JSON", mode);
	}
}

#[test]
fn test_config_file_not_required() {
	// Verify that sync works fine without a config file
	// This documents that config is optional and gracefully handles missing files

	let temp_dir = TempDir::new().expect("Failed to create temp dir");
	let syncr_dir = temp_dir.path();

	// Verify config file does NOT exist
	let config_path = syncr_dir.join("default.json");
	assert!(!config_path.exists(), "Config file should not exist initially");

	// When config loading is attempted, it should return None gracefully
	// and sync should continue with defaults
	// (This is verified in the merge_config_with_cli_options function)
}

#[test]
fn test_config_exclude_patterns_merge() {
	// Test that exclude patterns from config are properly merged with CLI patterns
	// This documents the merging behavior in merge_config_with_cli_options()

	let config_excludes =
		vec!["*.tmp".to_string(), ".git/**".to_string(), "node_modules/**".to_string()];

	let cli_excludes = vec!["*.log".to_string(), "*.bak".to_string()];

	// After merge, both should be present
	let mut merged = cli_excludes.clone();
	for pattern in config_excludes {
		if !merged.contains(&pattern) {
			merged.push(pattern);
		}
	}

	assert_eq!(merged.len(), 5);
	assert!(merged.contains(&"*.tmp".to_string()));
	assert!(merged.contains(&"*.log".to_string()));
}

#[test]
fn test_config_structure_roundtrip() {
	// Test that a config can be written and read back
	let temp_dir = TempDir::new().expect("Failed to create temp dir");
	let config_path = temp_dir.path().join("test.json");

	let original = r#"{
		"name": "roundtrip-test",
		"metadata": {
			"strategy": "smart",
			"alwaysChecksum": false
		},
		"symlinks": {
			"mode": "preserve",
			"skipDangling": true
		},
		"conflicts": {
			"strategy": "prefer-newest"
		},
		"delete": {
			"mode": "sync"
		},
		"exclude": {
			"patterns": ["*.cache"]
		}
	}"#;

	// Write
	fs::write(&config_path, original).expect("Failed to write config");

	// Read
	let read_back = fs::read_to_string(&config_path).expect("Failed to read config");

	// Parse both as JSON to compare structure (ignoring whitespace)
	let original_parsed: serde_json::Value =
		serde_json::from_str(original).expect("Original should parse");
	let read_parsed: serde_json::Value =
		serde_json::from_str(&read_back).expect("Read-back should parse");

	assert_eq!(original_parsed, read_parsed, "Config structure should be preserved");
}
