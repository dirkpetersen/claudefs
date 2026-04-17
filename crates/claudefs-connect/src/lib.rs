//! A simple connectivity test module for ClaudeFS.
//!
//! This module provides basic functionality to verify that the Rust
//! connectivity between components is working correctly.

/// Returns a greeting message.
///
/// # Returns
///
/// A [`String`] containing "Hello from Rust"
///
/// # Examples
///
/// ```
/// use claudefs_connect::hello;
/// let greeting = hello();
/// assert_eq!(greeting, "Hello from Rust");
/// ```
pub fn hello() -> String {
    "Hello from Rust".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hello_returns_correct_string() {
        let result = hello();
        assert_eq!(result, "Hello from Rust");
    }

    #[test]
    fn test_hello_returns_non_empty_string() {
        let result = hello();
        assert!(!result.is_empty());
    }

    #[test]
    fn test_hello_contains_hello() {
        let result = hello();
        assert!(result.contains("Hello"));
    }

    #[test]
    fn test_hello_contains_rust() {
        let result = hello();
        assert!(result.contains("Rust"));
    }

    #[test]
    fn test_hello_exact_length() {
        let result = hello();
        assert_eq!(result.len(), 15);
    }
}
