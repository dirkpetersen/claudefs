//! Simple S3 bucket policy

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

/// Effect of a policy statement - Allow or Deny.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyEffect {
    /// Allow the action
    Allow,
    /// Deny the action
    Deny,
}

/// S3 action that can be allowed or denied by a policy.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum S3Action {
    /// Get object (read)
    GetObject,
    /// Put object (write)
    PutObject,
    /// Delete object
    DeleteObject,
    /// List bucket contents
    ListBucket,
    /// Delete bucket
    DeleteBucket,
    /// Create bucket
    CreateBucket,
    /// All actions
    All,
}

impl S3Action {
    /// Parses an S3 action from an AWS-style action string.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "s3:GetObject" => Some(S3Action::GetObject),
            "s3:PutObject" => Some(S3Action::PutObject),
            "s3:DeleteObject" => Some(S3Action::DeleteObject),
            "s3:ListBucket" => Some(S3Action::ListBucket),
            "s3:DeleteBucket" => Some(S3Action::DeleteBucket),
            "s3:CreateBucket" => Some(S3Action::CreateBucket),
            "s3:*" => Some(S3Action::All),
            _ => None,
        }
    }

    /// Returns the AWS-style action string.
    pub fn to_str(&self) -> &'static str {
        match self {
            S3Action::GetObject => "s3:GetObject",
            S3Action::PutObject => "s3:PutObject",
            S3Action::DeleteObject => "s3:DeleteObject",
            S3Action::ListBucket => "s3:ListBucket",
            S3Action::DeleteBucket => "s3:DeleteBucket",
            S3Action::CreateBucket => "s3:CreateBucket",
            S3Action::All => "s3:*",
        }
    }
}

/// A principal that can be granted access
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Principal {
    /// Allow access from any principal
    Any,
    /// Allow access from a specific user ID
    User(u32),
    /// Allow access from any user in a specific group ID
    Group(u32),
    /// Allow access from a specific AWS account ID
    AccountId(String),
}

/// A resource pattern (e.g., "mybucket/*" or "mybucket/prefix/*")
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    /// The bucket name, or "*" for all buckets
    pub bucket: String,
    /// The key pattern (e.g., "*" or "prefix/*")
    pub key_pattern: String,
}

impl Resource {
    /// Creates a new resource with the given bucket and key pattern.
    pub fn new(bucket: &str, key_pattern: &str) -> Self {
        Self {
            bucket: bucket.to_string(),
            key_pattern: key_pattern.to_string(),
        }
    }

    /// Creates a resource that matches all keys in a bucket.
    pub fn bucket_only(bucket: &str) -> Self {
        Self {
            bucket: bucket.to_string(),
            key_pattern: "*".to_string(),
        }
    }

    /// Creates a resource that matches all buckets and all keys.
    pub fn all_buckets() -> Self {
        Self {
            bucket: "*".to_string(),
            key_pattern: "*".to_string(),
        }
    }

    /// Checks if this resource matches the given bucket and key.
    pub fn matches(&self, bucket: &str, key: &str) -> bool {
        if self.bucket != "*" && self.bucket != bucket {
            return false;
        }

        if self.key_pattern == "*" {
            return true;
        }

        if let Some(prefix) = self.key_pattern.strip_suffix("/*") {
            key.starts_with(prefix)
                && key.len() > prefix.len()
                && key.as_bytes().get(prefix.len()) == Some(&b'/')
        } else {
            false
        }
    }
}

/// A policy statement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyStatement {
    /// Whether this statement allows or denies access
    pub effect: PolicyEffect,
    /// The principals this statement applies to
    pub principals: Vec<Principal>,
    /// The actions this statement allows or denies
    pub actions: Vec<S3Action>,
    /// The resources this statement applies to
    pub resources: Vec<Resource>,
    /// Optional condition expression (currently unused)
    pub condition: Option<String>,
}

impl PolicyStatement {
    /// Creates a statement that allows public access to all resources.
    pub fn allow_all_public() -> Self {
        Self {
            effect: PolicyEffect::Allow,
            principals: vec![Principal::Any],
            actions: vec![S3Action::All],
            resources: vec![Resource::all_buckets()],
            condition: None,
        }
    }

    /// Creates a statement that denies all access.
    pub fn deny_all() -> Self {
        Self {
            effect: PolicyEffect::Deny,
            principals: vec![Principal::Any],
            actions: vec![S3Action::All],
            resources: vec![Resource::all_buckets()],
            condition: None,
        }
    }

    /// Creates a statement that allows a user to read from a bucket.
    pub fn allow_user_read(uid: u32, bucket: &str) -> Self {
        Self {
            effect: PolicyEffect::Allow,
            principals: vec![Principal::User(uid)],
            actions: vec![S3Action::GetObject, S3Action::ListBucket],
            resources: vec![Resource::bucket_only(bucket)],
            condition: None,
        }
    }

    /// Creates a statement that allows a user to write to a bucket.
    pub fn allow_user_write(uid: u32, bucket: &str) -> Self {
        Self {
            effect: PolicyEffect::Allow,
            principals: vec![Principal::User(uid)],
            actions: vec![S3Action::PutObject, S3Action::DeleteObject],
            resources: vec![Resource::new(bucket, "*")],
            condition: None,
        }
    }

    /// Checks if this statement applies to the given request.
    pub fn applies(&self, uid: u32, action: &S3Action, bucket: &str, key: &str) -> bool {
        let principal_matches = self.principals.iter().any(|p| match p {
            Principal::Any => true,
            Principal::User(u) => *u == uid,
            Principal::Group(_) => false,
            Principal::AccountId(_) => false,
        });

        if !principal_matches {
            return false;
        }

        let action_matches = self.actions.iter().any(|a| match a {
            S3Action::All => true,
            _ => a == action,
        });

        if !action_matches {
            return false;
        }

        self.resources.iter().any(|r| r.matches(bucket, key))
    }
}

/// A bucket policy containing multiple statements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketPolicy {
    /// The policy version string
    pub version: String,
    /// The list of policy statements
    pub statements: Vec<PolicyStatement>,
}

impl BucketPolicy {
    /// Creates a new empty bucket policy.
    pub fn new() -> Self {
        Self {
            version: "2012-10-17".to_string(),
            statements: Vec::new(),
        }
    }

    /// Adds a statement to this policy and returns self for chaining.
    pub fn add_statement(&mut self, stmt: PolicyStatement) -> &mut Self {
        self.statements.push(stmt);
        self
    }

    /// Checks if the given request is allowed by this policy.
    pub fn is_allowed(&self, uid: u32, action: &S3Action, bucket: &str, key: &str) -> bool {
        let mut has_deny = false;

        for stmt in &self.statements {
            if stmt.applies(uid, action, bucket, key) && stmt.effect == PolicyEffect::Deny {
                has_deny = true;
                break;
            }
        }

        if has_deny {
            return false;
        }

        for stmt in &self.statements {
            if stmt.applies(uid, action, bucket, key) && stmt.effect == PolicyEffect::Allow {
                return true;
            }
        }

        false
    }

    /// Serializes the policy to JSON format.
    pub fn to_json(&self) -> String {
        let mut json = String::from("{\"version\":\"");
        json.push_str(&self.version);
        json.push_str("\",\"statements\":[");

        for (i, stmt) in self.statements.iter().enumerate() {
            if i > 0 {
                json.push(',');
            }
            json.push('{');

            json.push_str("\"effect\":\"");
            match stmt.effect {
                PolicyEffect::Allow => json.push_str("Allow"),
                PolicyEffect::Deny => json.push_str("Deny"),
            }
            json.push_str("\",\"principals\":");

            json.push('[');
            for (j, p) in stmt.principals.iter().enumerate() {
                if j > 0 {
                    json.push(',');
                }
                match p {
                    Principal::Any => json.push_str("\"*\""),
                    Principal::User(u) => {
                        json.push_str(&format!("{{\"type\":\"User\",\"id\":{}}}", u))
                    }
                    Principal::Group(g) => {
                        json.push_str(&format!("{{\"type\":\"Group\",\"id\":{}}}", g))
                    }
                    Principal::AccountId(a) => {
                        json.push_str(&format!("{{\"type\":\"AccountId\",\"id\":\"{}\"}}", a))
                    }
                }
            }
            json.push(']');

            json.push_str(",\"actions\":[");
            for (j, a) in stmt.actions.iter().enumerate() {
                if j > 0 {
                    json.push(',');
                }
                json.push('"');
                json.push_str(a.to_str());
                json.push('"');
            }
            json.push(']');

            json.push_str(",\"resources\":[");
            for (j, r) in stmt.resources.iter().enumerate() {
                if j > 0 {
                    json.push(',');
                }
                json.push_str(&format!("\"{}/{}\"", r.bucket, r.key_pattern));
            }
            json.push(']');

            json.push('}');
        }

        json.push_str("]}");
        json
    }

    /// Returns true if the policy has no statements.
    pub fn is_empty(&self) -> bool {
        self.statements.is_empty()
    }

    /// Returns the number of statements in this policy.
    pub fn statement_count(&self) -> usize {
        self.statements.len()
    }
}

impl Default for BucketPolicy {
    fn default() -> Self {
        Self::new()
    }
}

/// Global bucket policy registry
pub struct BucketPolicyRegistry {
    policies: RwLock<HashMap<String, BucketPolicy>>,
}

impl BucketPolicyRegistry {
    /// Creates a new empty bucket policy registry.
    pub fn new() -> Self {
        Self {
            policies: RwLock::new(HashMap::new()),
        }
    }

    /// Sets the policy for a bucket.
    pub fn set_policy(&self, bucket: &str, policy: BucketPolicy) {
        if let Ok(mut policies) = self.policies.write() {
            policies.insert(bucket.to_string(), policy);
        }
    }

    /// Gets the policy for a bucket, if one exists.
    pub fn get_policy(&self, bucket: &str) -> Option<BucketPolicy> {
        self.policies.read().ok()?.get(bucket).cloned()
    }

    /// Removes and returns true if a policy existed for the bucket.
    pub fn remove_policy(&self, bucket: &str) -> bool {
        if let Ok(mut policies) = self.policies.write() {
            policies.remove(bucket).is_some()
        } else {
            false
        }
    }

    /// Checks if the given request is allowed for the bucket.
    pub fn is_allowed(&self, uid: u32, action: &S3Action, bucket: &str, key: &str) -> bool {
        if let Ok(policies) = self.policies.read() {
            if let Some(policy) = policies.get(bucket) {
                return policy.is_allowed(uid, action, bucket, key);
            }
        }
        true
    }

    /// Returns the number of buckets with policies.
    pub fn bucket_count(&self) -> usize {
        self.policies.read().map(|p| p.len()).unwrap_or(0)
    }
}

impl Default for BucketPolicyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s3action_from_str() {
        assert_eq!(S3Action::parse("s3:GetObject"), Some(S3Action::GetObject));
        assert_eq!(S3Action::parse("s3:PutObject"), Some(S3Action::PutObject));
        assert_eq!(S3Action::parse("s3:ListBucket"), Some(S3Action::ListBucket));
        assert_eq!(S3Action::parse("s3:*"), Some(S3Action::All));
    }

    #[test]
    fn test_s3action_from_str_invalid() {
        assert_eq!(S3Action::parse("s3:Invalid"), None);
    }

    #[test]
    fn test_s3action_to_str() {
        assert_eq!(S3Action::GetObject.to_str(), "s3:GetObject");
        assert_eq!(S3Action::All.to_str(), "s3:*");
    }

    #[test]
    fn test_resource_new() {
        let r = Resource::new("bucket", "prefix/*");
        assert_eq!(r.bucket, "bucket");
        assert_eq!(r.key_pattern, "prefix/*");
    }

    #[test]
    fn test_resource_bucket_only() {
        let r = Resource::bucket_only("mybucket");
        assert_eq!(r.bucket, "mybucket");
        assert_eq!(r.key_pattern, "*");
    }

    #[test]
    fn test_resource_all_buckets() {
        let r = Resource::all_buckets();
        assert_eq!(r.bucket, "*");
        assert_eq!(r.key_pattern, "*");
    }

    #[test]
    fn test_resource_matches_wildcard() {
        let r = Resource::bucket_only("bucket");
        assert!(r.matches("bucket", "any/key/here"));
        assert!(r.matches("bucket", ""));
    }

    #[test]
    fn test_resource_matches_prefix() {
        let r = Resource::new("bucket", "prefix/*");
        assert!(r.matches("bucket", "prefix/file.txt"));
        assert!(r.matches("bucket", "prefix/dir/file.txt"));
        assert!(!r.matches("bucket", "other/file.txt"));
        assert!(!r.matches("bucket", "prefix"));
    }

    #[test]
    fn test_resource_matches_all_buckets() {
        let r = Resource::all_buckets();
        assert!(r.matches("bucket1", "key"));
        assert!(r.matches("bucket2", "key"));
    }

    #[test]
    fn test_allow_all_public() {
        let stmt = PolicyStatement::allow_all_public();
        assert_eq!(stmt.effect, PolicyEffect::Allow);
        assert_eq!(stmt.principals, vec![Principal::Any]);
        assert_eq!(stmt.actions, vec![S3Action::All]);
    }

    #[test]
    fn test_deny_all() {
        let stmt = PolicyStatement::deny_all();
        assert_eq!(stmt.effect, PolicyEffect::Deny);
    }

    #[test]
    fn test_allow_user_read() {
        let stmt = PolicyStatement::allow_user_read(1000, "mybucket");
        assert!(stmt.applies(1000, &S3Action::GetObject, "mybucket", "key"));
        assert!(stmt.applies(1000, &S3Action::ListBucket, "mybucket", ""));
    }

    #[test]
    fn test_allow_user_write() {
        let stmt = PolicyStatement::allow_user_write(1000, "mybucket");
        assert!(stmt.applies(1000, &S3Action::PutObject, "mybucket", "key"));
        assert!(stmt.applies(1000, &S3Action::DeleteObject, "mybucket", "key"));
    }

    #[test]
    fn test_policy_statement_applies_mismatch() {
        let stmt = PolicyStatement::allow_user_read(1000, "mybucket");
        assert!(!stmt.applies(999, &S3Action::GetObject, "mybucket", "key"));
        assert!(!stmt.applies(1000, &S3Action::PutObject, "mybucket", "key"));
        assert!(!stmt.applies(1000, &S3Action::GetObject, "other", "key"));
    }

    #[test]
    fn test_bucket_policy_new() {
        let policy = BucketPolicy::new();
        assert_eq!(policy.version, "2012-10-17");
        assert!(policy.is_empty());
    }

    #[test]
    fn test_bucket_policy_add_statement() {
        let mut policy = BucketPolicy::new();
        policy.add_statement(PolicyStatement::allow_all_public());
        assert_eq!(policy.statement_count(), 1);
    }

    #[test]
    fn test_bucket_policy_is_allowed() {
        let mut policy = BucketPolicy::new();
        policy.add_statement(PolicyStatement::allow_user_read(1000, "bucket"));

        assert!(policy.is_allowed(1000, &S3Action::GetObject, "bucket", "key"));
        assert!(!policy.is_allowed(999, &S3Action::GetObject, "bucket", "key"));
    }

    #[test]
    fn test_bucket_policy_default_deny() {
        let policy = BucketPolicy::new();
        assert!(!policy.is_allowed(1000, &S3Action::GetObject, "bucket", "key"));
    }

    #[test]
    fn test_bucket_policy_deny_overrides_allow() {
        let mut policy = BucketPolicy::new();
        policy.add_statement(PolicyStatement::allow_user_read(1000, "bucket"));
        policy.add_statement(PolicyStatement::deny_all());

        assert!(!policy.is_allowed(1000, &S3Action::GetObject, "bucket", "key"));
    }

    #[test]
    fn test_bucket_policy_to_json() {
        let mut policy = BucketPolicy::new();
        policy.add_statement(PolicyStatement::allow_user_read(1000, "mybucket"));

        let json = policy.to_json();
        assert!(json.contains("2012-10-17"));
        assert!(json.contains("Allow"));
        assert!(json.contains("mybucket/*"));
    }

    #[test]
    fn test_bucket_policy_registry_set_get() {
        let registry = BucketPolicyRegistry::new();
        let mut policy = BucketPolicy::new();
        policy.add_statement(PolicyStatement::allow_all_public());

        registry.set_policy("mybucket", policy);

        let retrieved = registry.get_policy("mybucket");
        assert!(retrieved.is_some());
        assert!(retrieved
            .unwrap()
            .is_allowed(0, &S3Action::GetObject, "mybucket", "key"));
    }

    #[test]
    fn test_bucket_policy_registry_remove() {
        let registry = BucketPolicyRegistry::new();
        let mut policy = BucketPolicy::new();
        policy.add_statement(PolicyStatement::allow_all_public());

        registry.set_policy("mybucket", policy);
        let removed = registry.remove_policy("mybucket");

        assert!(removed);
        assert!(registry.get_policy("mybucket").is_none());
    }

    #[test]
    fn test_bucket_policy_registry_open_access() {
        let registry = BucketPolicyRegistry::new();
        assert!(registry.is_allowed(1000, &S3Action::GetObject, "bucket", "key"));
    }

    #[test]
    fn test_bucket_policy_registry_bucket_count() {
        let registry = BucketPolicyRegistry::new();
        registry.set_policy("bucket1", BucketPolicy::new());
        registry.set_policy("bucket2", BucketPolicy::new());

        assert_eq!(registry.bucket_count(), 2);
    }
}
