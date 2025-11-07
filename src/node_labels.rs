//! Intelligent node label generation for concise display
//!
//! This module provides algorithms for generating short, distinguishable labels
//! for sync nodes that display in CLI progress and TUI tables. The algorithm
//! analyzes node addresses and determines what components are most important
//! for distinguishing each node.

use std::collections::HashMap;
use std::path::Path;

/// Parsed components of a node address
#[derive(Debug, Clone)]
pub struct NodeComponents {
	/// True if this is a remote node (contains hostname)
	pub is_remote: bool,
	/// Optional username (user@host format)
	pub user: Option<String>,
	/// Hostname or "local" for local paths
	pub host: String,
	/// Directory path (normalized and expanded)
	pub path: String,
}

/// Context about node differentiation strategy
#[derive(Debug, Clone)]
struct LabelContext {
	all_same_host: bool,
	#[allow(dead_code)]
	all_different_hosts: bool,
	#[allow(dead_code)]
	all_local: bool,
	common_path_prefix: String,
	common_domain: Option<String>,
	basename_conflicts: HashMap<String, usize>,
	max_label_len: usize,
}

/// Parse a node address into components
///
/// Handles formats:
/// - `/path/to/dir` - local path
/// - `./relative/path` - local relative path
/// - `hostname:/path` - remote path
/// - `user@hostname:/path` - remote with user
fn parse_node(address: &str) -> NodeComponents {
	let address = address.trim();

	// Check if remote (contains : and doesn't start with /, ., or ~)
	if address.contains(':')
		&& !address.starts_with('/')
		&& !address.starts_with('.')
		&& !address.starts_with('~')
	{
		// Remote node
		let (host_part, path) = address.split_once(':').unwrap_or((address, ""));

		let (user, host) = if let Some((u, h)) = host_part.split_once('@') {
			(Some(u.to_string()), h.to_string())
		} else {
			(None, host_part.to_string())
		};

		NodeComponents { is_remote: true, user, host, path: path.to_string() }
	} else {
		// Local node
		let expanded = expand_path(address);
		NodeComponents { is_remote: false, user: None, host: "local".to_string(), path: expanded }
	}
}

/// Expand paths starting with ~ or make them absolute
fn expand_path(path: &str) -> String {
	if path.starts_with('~') {
		if let Ok(home) = std::env::var("HOME") {
			return path.replacen('~', &home, 1);
		}
	}

	let p = Path::new(path);
	if p.is_absolute() {
		path.to_string()
	} else if let Ok(cwd) = std::env::current_dir() {
		cwd.join(path).to_string_lossy().to_string()
	} else {
		path.to_string()
	}
}

/// Find the common prefix shared by all paths
fn find_common_path_prefix(paths: &[String]) -> String {
	if paths.is_empty() {
		return String::new();
	}

	let mut common = paths[0].clone();
	for path in &paths[1..] {
		// Find common prefix character by character
		common.truncate(common.chars().zip(path.chars()).take_while(|(c1, c2)| c1 == c2).count());
		if common.is_empty() {
			break;
		}
	}

	// Backtrack to last directory separator
	if let Some(last_sep) = common.rfind('/') {
		common.truncate(last_sep + 1);
	} else {
		common.clear();
	}

	common
}

/// Find common domain in hostnames (e.g., "example.com")
fn find_common_domain(hosts: &[String]) -> Option<String> {
	if hosts.is_empty() {
		return None;
	}

	// Only consider real hostnames, not "local"
	let real_hosts: Vec<_> = hosts.iter().filter(|h| h != &"local").collect();
	if real_hosts.is_empty() || real_hosts.len() < 2 {
		return None;
	}

	// Split first host by dots
	let parts: Vec<&str> = real_hosts[0].split('.').collect();
	if parts.len() < 2 {
		return None;
	}

	// Try to find common domain suffix
	for start_idx in 0..parts.len() {
		let candidate = parts[start_idx..].join(".");
		if real_hosts.iter().all(|h| h.ends_with(&candidate) || h.as_str() == candidate) {
			return Some(candidate);
		}
	}

	None
}

/// Extract the basename of a path
fn path_basename(path: &str) -> String {
	Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or("").to_string()
}

/// Extract parent directory name from a path
fn path_parent_name(path: &str) -> Option<String> {
	Path::new(path)
		.parent()
		.and_then(|p| p.file_name())
		.and_then(|n| n.to_str())
		.map(|s| s.to_string())
}

/// Intelligently abbreviate a hostname
fn abbreviate_host(host: &str, context: &LabelContext) -> String {
	if host == "local" {
		return String::new();
	}

	if context.all_same_host {
		return String::new();
	}

	// If there's a common domain, strip it
	if let Some(ref domain) = context.common_domain {
		if host.ends_with(&format!(".{}", domain)) {
			let abbrev = host.strip_suffix(&format!(".{}", domain)).unwrap_or(host);
			return abbrev.to_string();
		}
	}

	// If it's an IP address, try to abbreviate
	if let Some(last_octet) = host.rsplit('.').next() {
		if last_octet.chars().all(|c| c.is_numeric()) {
			// Likely an IP, use last octet with prefix
			return format!(".{}", last_octet);
		}
	}

	host.to_string()
}

/// Intelligently abbreviate a path
fn abbreviate_path(path: &str, context: &LabelContext, is_local: bool) -> String {
	if path.is_empty() {
		return String::new();
	}

	let mut result = path.to_string();

	// Strip common prefix
	if !context.common_path_prefix.is_empty() && result.starts_with(&context.common_path_prefix) {
		result = result.strip_prefix(&context.common_path_prefix).unwrap_or(&result).to_string();
	}

	// Preserve leading markers for local paths
	if is_local && path.starts_with(['.', '~']) {
		// Keep the leading ./ or ~/
		result = path.chars().take_while(|c| ['.', '/', '~'].contains(c)).collect::<String>()
			+ result.strip_prefix(['.', '/', '~']).unwrap_or(&result);
	}

	result
}

/// Smartly truncate a string preserving start and end
fn truncate_smart(s: &str, max_len: usize) -> String {
	if s.len() <= max_len {
		return s.to_string();
	}

	if max_len <= 3 {
		return s.chars().take(max_len).collect();
	}

	let ellipsis = "…";
	let available = max_len.saturating_sub(ellipsis.len());
	let start_len = available.div_ceil(2);
	let end_len = available / 2;

	let start = s.chars().take(start_len).collect::<String>();
	let end = s.chars().rev().take(end_len).collect::<String>();
	let end = end.chars().rev().collect::<String>();

	format!("{}{}{}", start, ellipsis, end)
}

/// Generate concise, distinguishable labels for sync nodes
///
/// # Arguments
/// * `addresses` - Slice of node addresses to generate labels for
///
/// # Returns
/// Vector of labels, one per address, suitable for display in CLI/TUI
///
/// # Examples
/// ```ignore
/// let labels = generate_node_labels(&[
///     "./dir1",
///     "./dir2",
///     "server:/backup"
/// ]);
/// assert_eq!(labels, vec!["./dir1", "./dir2", "server"]);
/// ```
pub fn generate_node_labels(addresses: &[&str]) -> Vec<String> {
	if addresses.is_empty() {
		return Vec::new();
	}

	// Parse all nodes
	let nodes: Vec<NodeComponents> = addresses.iter().map(|a| parse_node(a)).collect();

	// Analyze node composition
	let hosts: Vec<String> = nodes.iter().map(|n| n.host.clone()).collect();
	let paths: Vec<String> = nodes.iter().map(|n| n.path.clone()).collect();

	let unique_hosts = hosts.iter().collect::<std::collections::HashSet<_>>().len();
	let all_same_host = unique_hosts == 1;
	let all_different_hosts = unique_hosts == hosts.len();
	let all_local = nodes.iter().all(|n| !n.is_remote);

	// Find common components
	let common_path_prefix = find_common_path_prefix(&paths);
	let common_domain = find_common_domain(&hosts);

	// Count basename conflicts
	let mut basename_conflicts: HashMap<String, usize> = HashMap::new();
	for path in &paths {
		let basename = path_basename(path);
		*basename_conflicts.entry(basename).or_insert(0) += 1;
	}

	let context = LabelContext {
		all_same_host,
		all_different_hosts,
		all_local,
		common_path_prefix,
		common_domain,
		basename_conflicts,
		max_label_len: 14,
	};

	// Generate labels
	let mut labels = Vec::new();

	for node in &nodes {
		let mut parts = Vec::new();

		// Add host part if distinguishing
		if !all_local && !context.all_same_host {
			let host_abbrev = abbreviate_host(&node.host, &context);
			if !host_abbrev.is_empty() {
				parts.push(host_abbrev);
			}
		}

		// Add user if differentiating
		if let Some(ref user) = node.user {
			// Only include user if there are multiple users on same host
			let users_on_host = nodes
				.iter()
				.filter(|n| n.host == node.host)
				.filter_map(|n| n.user.as_ref())
				.collect::<std::collections::HashSet<_>>()
				.len();
			if users_on_host > 1 {
				parts.push(user.clone());
			}
		}

		// Add path part
		let basename = path_basename(&node.path);
		let has_basename_conflict =
			matches!(context.basename_conflicts.get(&basename), Some(&count) if count > 1);

		if all_local && !has_basename_conflict && !basename.is_empty() {
			// Simple case: just use basename
			parts.push(basename);
		} else if has_basename_conflict {
			// Use parent/basename for conflict resolution
			if let Some(parent) = path_parent_name(&node.path) {
				if !parent.is_empty() {
					parts.push(format!("{}/{}", parent, basename));
				} else {
					parts.push(basename);
				}
			} else {
				parts.push(basename);
			}
		} else {
			// Use abbreviated path
			let abbrev = abbreviate_path(&node.path, &context, node.is_remote);
			if !abbrev.is_empty() {
				parts.push(abbrev);
			}
		}

		// Assemble and truncate
		let mut label = parts.join(":");
		if label.is_empty() {
			// Fallback: use path or host
			label = if node.path.is_empty() { node.host.clone() } else { node.path.clone() };
		}

		label = truncate_smart(&label, context.max_label_len);
		labels.push(label);
	}

	// Handle collisions: if any labels are identical, add indices
	let mut label_counts: HashMap<String, usize> = HashMap::new();
	for label in &labels {
		*label_counts.entry(label.clone()).or_insert(0) += 1;
	}

	let has_collisions = label_counts.values().any(|&count| count > 1);
	if has_collisions {
		let mut label_indices: HashMap<String, usize> = HashMap::new();
		for label in &mut labels {
			if let Some(&count) = label_counts.get(label) {
				if count > 1 {
					let idx = label_indices.entry(label.clone()).or_insert(0);
					*idx += 1;
					label.push_str(&format!("[{}]", idx));
				}
			}
		}
	}

	labels
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_parse_local_paths() {
		let node = parse_node("./dir");
		assert!(!node.is_remote);
		assert_eq!(node.host, "local");
		assert!(node.user.is_none());
	}

	#[test]
	fn test_parse_remote_path() {
		let node = parse_node("hostname:/path");
		assert!(node.is_remote);
		assert_eq!(node.host, "hostname");
		assert_eq!(node.path, "/path");
		assert!(node.user.is_none());
	}

	#[test]
	fn test_parse_remote_with_user() {
		let node = parse_node("user@hostname:/path");
		assert!(node.is_remote);
		assert_eq!(node.host, "hostname");
		assert_eq!(node.user, Some("user".to_string()));
		assert_eq!(node.path, "/path");
	}

	#[test]
	fn test_simple_local_dirs() {
		let labels = generate_node_labels(&["./dir1", "./dir2"]);
		assert_eq!(labels.len(), 2);
		// Should differentiate by basename
		assert!(labels[0].contains("dir1") || labels[0].contains("1"));
		assert!(labels[1].contains("dir2") || labels[1].contains("2"));
	}

	#[test]
	fn test_mixed_local_and_remote() {
		let labels = generate_node_labels(&["./local", "server:/remote"]);
		assert_eq!(labels.len(), 2);
		// First should be local, second should include server
		assert!(labels[0].contains("local"));
		assert!(labels[1].contains("server") || labels[1].contains("remote"));
	}

	#[test]
	fn test_same_host_different_paths() {
		let labels = generate_node_labels(&["server:/data/proj1", "server:/data/proj2"]);
		assert_eq!(labels.len(), 2);
		// Host should be omitted, paths differentiate
		assert!(labels[0].contains("proj1"));
		assert!(labels[1].contains("proj2"));
	}

	#[test]
	fn test_different_hosts_same_path() {
		let labels = generate_node_labels(&["host1:/data", "host2:/data"]);
		assert_eq!(labels.len(), 2);
		// Paths should be omitted or minimal, hosts differentiate
		assert!(labels[0].contains("host1"));
		assert!(labels[1].contains("host2"));
	}

	#[test]
	fn test_fqdn_with_common_domain() {
		let labels = generate_node_labels(&["srv1.example.com:/data", "srv2.example.com:/data"]);
		assert_eq!(labels.len(), 2);
		// Should strip .example.com and just show srv1, srv2
		assert!(labels[0].contains("srv1"));
		assert!(labels[1].contains("srv2"));
		assert!(!labels[0].contains("example"));
	}

	#[test]
	fn test_ip_addresses() {
		let labels = generate_node_labels(&["192.168.1.10:/data", "192.168.1.20:/data"]);
		assert_eq!(labels.len(), 2);
		// Should use last octet for distinction
		assert!(labels[0].contains("10") || labels[0].contains("data"));
		assert!(labels[1].contains("20") || labels[1].contains("data"));
	}

	#[test]
	fn test_single_node() {
		let labels = generate_node_labels(&["/data"]);
		assert_eq!(labels.len(), 1);
		assert!(!labels[0].is_empty());
	}

	#[test]
	fn test_identical_paths_get_indices() {
		let labels = generate_node_labels(&["./dir", "./dir", "./dir"]);
		assert_eq!(labels.len(), 3);
		// All labels should be different with indices
		assert_eq!(labels[0].len(), labels[1].len());
		assert_ne!(labels[0], labels[1]);
	}

	#[test]
	fn test_truncation() {
		let long_label = "very-long-hostname-with-many-chars";
		let truncated = truncate_smart(long_label, 10);
		assert_eq!(truncated.len(), 10);
		assert!(truncated.contains("…"));
	}
}

// vim: ts=4
