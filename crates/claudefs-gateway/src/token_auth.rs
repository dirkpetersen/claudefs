//! Bearer token authentication for S3

use std::collections::HashMap;
use std::sync::Mutex;

/// Token permissions bitmask
#[derive(Debug, Clone, Copy, Default)]
pub struct TokenPermissions {
    pub read: bool,
    pub write: bool,
    pub admin: bool,
}

impl TokenPermissions {
    pub fn read_only() -> Self {
        Self {
            read: true,
            write: false,
            admin: false,
        }
    }

    pub fn read_write() -> Self {
        Self {
            read: true,
            write: true,
            admin: false,
        }
    }

    pub fn admin() -> Self {
        Self {
            read: true,
            write: true,
            admin: true,
        }
    }
}

/// An authentication token
#[derive(Debug, Clone)]
pub struct AuthToken {
    /// The token string (random hex)
    pub token: String,
    /// Associated user ID
    pub uid: u32,
    /// Associated group ID
    pub gid: u32,
    /// Display name
    pub name: String,
    /// Token permissions
    pub permissions: TokenPermissions,
    /// Expiry (Unix seconds, 0 = never expires)
    pub expires_at: u64,
}

impl AuthToken {
    pub fn new(token: &str, uid: u32, gid: u32, name: &str) -> Self {
        Self {
            token: token.to_string(),
            uid,
            gid,
            name: name.to_string(),
            permissions: TokenPermissions::default(),
            expires_at: 0,
        }
    }

    pub fn with_expiry(mut self, expires_at: u64) -> Self {
        self.expires_at = expires_at;
        self
    }

    pub fn with_permissions(mut self, permissions: TokenPermissions) -> Self {
        self.permissions = permissions;
        self
    }

    pub fn is_expired(&self, now: u64) -> bool {
        self.expires_at > 0 && now > self.expires_at
    }

    pub fn can_read(&self) -> bool {
        self.permissions.read
    }

    pub fn can_write(&self) -> bool {
        self.permissions.write
    }

    pub fn can_admin(&self) -> bool {
        self.permissions.admin
    }
}

/// Token authentication registry
pub struct TokenAuth {
    tokens: Mutex<HashMap<String, AuthToken>>,
}

impl TokenAuth {
    pub fn new() -> Self {
        Self {
            tokens: Mutex::new(HashMap::new()),
        }
    }

    /// Register a token
    pub fn register(&self, token: AuthToken) {
        self.tokens
            .lock()
            .unwrap()
            .insert(token.token.clone(), token);
    }

    /// Validate a token string, return the token if valid and not expired
    pub fn validate(&self, token: &str, now: u64) -> Option<AuthToken> {
        let tokens = self.tokens.lock().unwrap();
        tokens.get(token).and_then(|t| {
            if t.is_expired(now) {
                None
            } else {
                Some(t.clone())
            }
        })
    }

    /// Revoke a token by string
    pub fn revoke(&self, token: &str) -> bool {
        self.tokens.lock().unwrap().remove(token).is_some()
    }

    /// List all tokens for a user (by uid)
    pub fn tokens_for_user(&self, uid: u32) -> Vec<AuthToken> {
        self.tokens
            .lock()
            .unwrap()
            .values()
            .filter(|t| t.uid == uid)
            .cloned()
            .collect()
    }

    /// Count valid (non-expired) tokens at a given time
    pub fn valid_count(&self, now: u64) -> usize {
        self.tokens
            .lock()
            .unwrap()
            .values()
            .filter(|t| !t.is_expired(now))
            .count()
    }

    /// Remove all expired tokens
    pub fn cleanup_expired(&self, now: u64) -> usize {
        let mut tokens = self.tokens.lock().unwrap();
        let before = tokens.len();
        tokens.retain(|_, t| !t.is_expired(now));
        before - tokens.len()
    }

    /// Generate a simple token string (hex of counter + uid)
    pub fn generate_token(uid: u32, counter: u64) -> String {
        format!("{:016x}{:08x}", counter, uid)
    }

    /// Check if a token string exists (regardless of expiry)
    pub fn exists(&self, token: &str) -> bool {
        self.tokens.lock().unwrap().contains_key(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_permissions_read_only() {
        let perms = TokenPermissions::read_only();
        assert!(perms.read);
        assert!(!perms.write);
        assert!(!perms.admin);
    }

    #[test]
    fn test_token_permissions_read_write() {
        let perms = TokenPermissions::read_write();
        assert!(perms.read);
        assert!(perms.write);
        assert!(!perms.admin);
    }

    #[test]
    fn test_token_permissions_admin() {
        let perms = TokenPermissions::admin();
        assert!(perms.read);
        assert!(perms.write);
        assert!(perms.admin);
    }

    #[test]
    fn test_auth_token_new() {
        let token = AuthToken::new("abc123", 1000, 100, "testuser");
        assert_eq!(token.token, "abc123");
        assert_eq!(token.uid, 1000);
        assert_eq!(token.gid, 100);
        assert_eq!(token.name, "testuser");
        assert_eq!(token.expires_at, 0);
    }

    #[test]
    fn test_auth_token_with_expiry() {
        let token = AuthToken::new("abc123", 1000, 100, "user").with_expiry(1000000);
        assert_eq!(token.expires_at, 1000000);
    }

    #[test]
    fn test_auth_token_with_permissions() {
        let token = AuthToken::new("abc123", 1000, 100, "user")
            .with_permissions(TokenPermissions::read_only());
        assert!(token.can_read());
        assert!(!token.can_write());
    }

    #[test]
    fn test_auth_token_is_expired() {
        let token = AuthToken::new("abc", 1, 1, "u").with_expiry(100);
        assert!(!token.is_expired(50));
        assert!(!token.is_expired(100));
        assert!(token.is_expired(101));
    }

    #[test]
    fn test_auth_token_never_expires() {
        let token = AuthToken::new("abc", 1, 1, "u");
        assert!(!token.is_expired(u64::MAX));
    }

    #[test]
    fn test_auth_token_can_read() {
        let token =
            AuthToken::new("abc", 1, 1, "u").with_permissions(TokenPermissions::read_only());
        assert!(token.can_read());
    }

    #[test]
    fn test_auth_token_can_write() {
        let token =
            AuthToken::new("abc", 1, 1, "u").with_permissions(TokenPermissions::read_write());
        assert!(token.can_write());
    }

    #[test]
    fn test_auth_token_can_admin() {
        let token = AuthToken::new("abc", 1, 1, "u").with_permissions(TokenPermissions::admin());
        assert!(token.can_admin());
    }

    #[test]
    fn test_token_auth_register() {
        let auth = TokenAuth::new();
        auth.register(AuthToken::new("token1", 1000, 100, "user1"));

        assert!(auth.exists("token1"));
    }

    #[test]
    fn test_token_auth_validate() {
        let auth = TokenAuth::new();
        auth.register(AuthToken::new("token1", 1000, 100, "user1"));

        let token = auth.validate("token1", 0);
        assert!(token.is_some());
        assert_eq!(token.unwrap().uid, 1000);
    }

    #[test]
    fn test_token_auth_validate_expired() {
        let auth = TokenAuth::new();
        auth.register(AuthToken::new("token1", 1000, 100, "user1").with_expiry(100));

        let token = auth.validate("token1", 101);
        assert!(token.is_none());
    }

    #[test]
    fn test_token_auth_validate_unknown() {
        let auth = TokenAuth::new();
        let token = auth.validate("nonexistent", 0);
        assert!(token.is_none());
    }

    #[test]
    fn test_token_auth_revoke() {
        let auth = TokenAuth::new();
        auth.register(AuthToken::new("token1", 1000, 100, "user1"));

        assert!(auth.revoke("token1"));
        assert!(!auth.exists("token1"));
    }

    #[test]
    fn test_token_auth_revoke_unknown() {
        let auth = TokenAuth::new();
        assert!(!auth.revoke("nonexistent"));
    }

    #[test]
    fn test_token_auth_tokens_for_user() {
        let auth = TokenAuth::new();
        auth.register(AuthToken::new("token1", 1000, 100, "user1"));
        auth.register(AuthToken::new("token2", 1000, 100, "user1"));
        auth.register(AuthToken::new("token3", 2000, 100, "user2"));

        let tokens = auth.tokens_for_user(1000);
        assert_eq!(tokens.len(), 2);
    }

    #[test]
    fn test_token_auth_valid_count() {
        let auth = TokenAuth::new();
        auth.register(AuthToken::new("token1", 1000, 100, "u").with_expiry(1000));
        auth.register(AuthToken::new("token2", 1000, 100, "u").with_expiry(2000));
        auth.register(AuthToken::new("token3", 1000, 100, "u").with_expiry(500));

        assert_eq!(auth.valid_count(600), 2);
        assert_eq!(auth.valid_count(1500), 1);
        assert_eq!(auth.valid_count(2500), 0);
    }

    #[test]
    fn test_token_auth_cleanup_expired() {
        let auth = TokenAuth::new();
        auth.register(AuthToken::new("token1", 1000, 100, "u").with_expiry(100));
        auth.register(AuthToken::new("token2", 1000, 100, "u").with_expiry(200));
        auth.register(AuthToken::new("token3", 1000, 100, "u").with_expiry(0));

        let removed = auth.cleanup_expired(150);
        assert_eq!(removed, 1);
        assert_eq!(auth.valid_count(199), 2);
    }

    #[test]
    fn test_token_auth_generate_token() {
        let token = TokenAuth::generate_token(1000, 12345);
        assert_eq!(token, "0000000000003039000003e8");
    }

    #[test]
    fn test_token_auth_exists() {
        let auth = TokenAuth::new();
        auth.register(AuthToken::new("token1", 1000, 100, "u"));

        assert!(auth.exists("token1"));
        assert!(!auth.exists("token2"));
    }
}
