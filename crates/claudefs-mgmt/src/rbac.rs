use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RbacError {
    #[error("User not found: {0}")]
    UserNotFound(String),
    #[error("Role not found: {0}")]
    RoleNotFound(String),
    #[error("Permission denied: user {user} lacks {permission} on {resource}")]
    PermissionDenied {
        user: String,
        permission: String,
        resource: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    ViewCluster,
    ViewNodes,
    DrainNodes,
    ManageTiering,
    ManageSnapshots,
    ViewQuotas,
    ManageQuotas,
    ViewReplication,
    QueryAnalytics,
    ManageWebhooks,
    Admin,
}

impl Permission {
    pub fn implies(&self, other: &Permission) -> bool {
        if self == &Permission::Admin {
            return true;
        }
        self == other
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub name: String,
    pub description: String,
    pub permissions: HashSet<Permission>,
}

impl Role {
    pub fn new(name: String, description: String) -> Self {
        Self {
            name,
            description,
            permissions: HashSet::new(),
        }
    }

    pub fn add_permission(&mut self, perm: Permission) {
        self.permissions.insert(perm);
    }

    pub fn has_permission(&self, perm: &Permission) -> bool {
        self.permissions.contains(perm)
    }

    pub fn permission_count(&self) -> usize {
        self.permissions.len()
    }
}

pub fn admin_role() -> Role {
    let mut role = Role::new(
        "admin".to_string(),
        "Full administrative access".to_string(),
    );
    role.add_permission(Permission::ViewCluster);
    role.add_permission(Permission::ViewNodes);
    role.add_permission(Permission::DrainNodes);
    role.add_permission(Permission::ManageTiering);
    role.add_permission(Permission::ManageSnapshots);
    role.add_permission(Permission::ViewQuotas);
    role.add_permission(Permission::ManageQuotas);
    role.add_permission(Permission::ViewReplication);
    role.add_permission(Permission::QueryAnalytics);
    role.add_permission(Permission::ManageWebhooks);
    role.add_permission(Permission::Admin);
    role
}

pub fn operator_role() -> Role {
    let mut role = Role::new("operator".to_string(), "Cluster operator".to_string());
    role.add_permission(Permission::ViewCluster);
    role.add_permission(Permission::ViewNodes);
    role.add_permission(Permission::DrainNodes);
    role.add_permission(Permission::ManageTiering);
    role.add_permission(Permission::ManageSnapshots);
    role.add_permission(Permission::ViewQuotas);
    role.add_permission(Permission::ViewReplication);
    role
}

pub fn viewer_role() -> Role {
    let mut role = Role::new("viewer".to_string(), "Read-only access".to_string());
    role.add_permission(Permission::ViewCluster);
    role.add_permission(Permission::ViewNodes);
    role.add_permission(Permission::ViewQuotas);
    role.add_permission(Permission::ViewReplication);
    role.add_permission(Permission::QueryAnalytics);
    role
}

pub fn tenant_admin_role() -> Role {
    let mut role = Role::new(
        "tenant_admin".to_string(),
        "Tenant administrator".to_string(),
    );
    role.add_permission(Permission::ViewQuotas);
    role.add_permission(Permission::ManageQuotas);
    role.add_permission(Permission::ViewReplication);
    role.add_permission(Permission::ManageSnapshots);
    role
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    pub roles: Vec<String>,
    pub active: bool,
    pub created_at: u64,
}

impl User {
    pub fn new(id: String, username: String) -> Self {
        Self {
            id,
            username,
            email: None,
            roles: vec![],
            active: true,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }
}

pub struct RbacRegistry {
    roles: HashMap<String, Role>,
    users: HashMap<String, User>,
}

impl RbacRegistry {
    pub fn new() -> Self {
        Self {
            roles: HashMap::new(),
            users: HashMap::new(),
        }
    }

    pub fn add_role(&mut self, role: Role) {
        self.roles.insert(role.name.clone(), role);
    }

    pub fn get_role(&self, name: &str) -> Option<&Role> {
        self.roles.get(name)
    }

    pub fn remove_role(&mut self, name: &str) -> Option<Role> {
        self.roles.remove(name)
    }

    pub fn add_user(&mut self, user: User) {
        self.users.insert(user.id.clone(), user);
    }

    pub fn get_user(&self, user_id: &str) -> Option<&User> {
        self.users.get(user_id)
    }

    pub fn get_user_by_name(&self, username: &str) -> Option<&User> {
        self.users.values().find(|u| u.username == username)
    }

    pub fn remove_user(&mut self, user_id: &str) -> Option<User> {
        self.users.remove(user_id)
    }

    pub fn assign_role(&mut self, user_id: &str, role_name: &str) -> Result<(), RbacError> {
        if !self.roles.contains_key(role_name) {
            return Err(RbacError::RoleNotFound(role_name.to_string()));
        }
        if let Some(user) = self.users.get_mut(user_id) {
            if !user.roles.contains(&role_name.to_string()) {
                user.roles.push(role_name.to_string());
            }
            Ok(())
        } else {
            Err(RbacError::UserNotFound(user_id.to_string()))
        }
    }

    pub fn revoke_role(&mut self, user_id: &str, role_name: &str) -> Result<(), RbacError> {
        if let Some(user) = self.users.get_mut(user_id) {
            user.roles.retain(|r| r != role_name);
            Ok(())
        } else {
            Err(RbacError::UserNotFound(user_id.to_string()))
        }
    }

    pub fn check_permission(
        &self,
        user_id: &str,
        permission: &Permission,
    ) -> Result<(), RbacError> {
        let user = self
            .users
            .get(user_id)
            .ok_or_else(|| RbacError::UserNotFound(user_id.to_string()))?;

        if !user.active {
            return Err(RbacError::PermissionDenied {
                user: user_id.to_string(),
                permission: format!("{:?}", permission),
                resource: "*".to_string(),
            });
        }

        for role_name in &user.roles {
            if let Some(role) = self.roles.get(role_name) {
                if role.has_permission(permission) || role.has_permission(&Permission::Admin) {
                    return Ok(());
                }
            }
        }

        Err(RbacError::PermissionDenied {
            user: user_id.to_string(),
            permission: format!("{:?}", permission),
            resource: "*".to_string(),
        })
    }

    pub fn user_permissions(&self, user_id: &str) -> HashSet<Permission> {
        let user = match self.users.get(user_id) {
            Some(u) => u,
            None => return HashSet::new(),
        };

        let mut perms = HashSet::new();
        for role_name in &user.roles {
            if let Some(role) = self.roles.get(role_name) {
                perms.extend(role.permissions.clone());
            }
        }
        perms
    }

    pub fn with_builtin_roles(mut self) -> Self {
        self.add_role(admin_role());
        self.add_role(operator_role());
        self.add_role(viewer_role());
        self.add_role(tenant_admin_role());
        self
    }

    pub fn role_count(&self) -> usize {
        self.roles.len()
    }

    pub fn user_count(&self) -> usize {
        self.users.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_admin_implies_all() {
        assert!(Permission::Admin.implies(&Permission::ViewCluster));
        assert!(Permission::Admin.implies(&Permission::ViewNodes));
        assert!(Permission::Admin.implies(&Permission::DrainNodes));
        assert!(Permission::Admin.implies(&Permission::ManageTiering));
        assert!(Permission::Admin.implies(&Permission::Admin));
    }

    #[test]
    fn test_permission_view_cluster_implies() {
        assert!(Permission::ViewCluster.implies(&Permission::ViewCluster));
        assert!(!Permission::ViewCluster.implies(&Permission::Admin));
        assert!(!Permission::ViewCluster.implies(&Permission::DrainNodes));
    }

    #[test]
    fn test_permission_drain_nodes_implies_only_itself() {
        assert!(Permission::DrainNodes.implies(&Permission::DrainNodes));
        assert!(!Permission::DrainNodes.implies(&Permission::ViewCluster));
    }

    #[test]
    fn test_role_new() {
        let role = Role::new("test_role".to_string(), "Test role".to_string());
        assert_eq!(role.name, "test_role");
        assert_eq!(role.description, "Test role");
        assert!(role.permissions.is_empty());
    }

    #[test]
    fn test_role_add_permission() {
        let mut role = Role::new("test_role".to_string(), "Test role".to_string());
        role.add_permission(Permission::ViewCluster);
        assert!(role.has_permission(&Permission::ViewCluster));
    }

    #[test]
    fn test_role_has_permission() {
        let mut role = Role::new("test_role".to_string(), "Test role".to_string());
        role.add_permission(Permission::ViewCluster);
        assert!(role.has_permission(&Permission::ViewCluster));
        assert!(!role.has_permission(&Permission::DrainNodes));
    }

    #[test]
    fn test_role_permission_count() {
        let mut role = Role::new("test_role".to_string(), "Test role".to_string());
        assert_eq!(role.permission_count(), 0);
        role.add_permission(Permission::ViewCluster);
        role.add_permission(Permission::ViewNodes);
        assert_eq!(role.permission_count(), 2);
    }

    #[test]
    fn test_admin_role_has_admin_permission() {
        let role = admin_role();
        assert!(role.has_permission(&Permission::Admin));
    }

    #[test]
    fn test_operator_role_has_drain_nodes_not_manage_quotas() {
        let role = operator_role();
        assert!(role.has_permission(&Permission::DrainNodes));
        assert!(!role.has_permission(&Permission::ManageQuotas));
    }

    #[test]
    fn test_viewer_role_has_view_cluster_not_drain_nodes() {
        let role = viewer_role();
        assert!(role.has_permission(&Permission::ViewCluster));
        assert!(!role.has_permission(&Permission::DrainNodes));
    }

    #[test]
    fn test_tenant_admin_role_has_manage_quotas() {
        let role = tenant_admin_role();
        assert!(role.has_permission(&Permission::ManageQuotas));
    }

    #[test]
    fn test_user_new() {
        let user = User::new("user1".to_string(), "alice".to_string());
        assert_eq!(user.id, "user1");
        assert_eq!(user.username, "alice");
        assert!(user.active);
        assert!(user.roles.is_empty());
    }

    #[test]
    fn test_rbac_registry_add_and_get_role() {
        let mut registry = RbacRegistry::new();
        let role = admin_role();
        registry.add_role(role);

        let retrieved = registry.get_role("admin");
        assert!(retrieved.is_some());
    }

    #[test]
    fn test_rbac_registry_add_and_get_user() {
        let mut registry = RbacRegistry::new();
        let user = User::new("user1".to_string(), "alice".to_string());
        registry.add_user(user);

        let retrieved = registry.get_user("user1");
        assert!(retrieved.is_some());
    }

    #[test]
    fn test_rbac_registry_get_user_by_name() {
        let mut registry = RbacRegistry::new();
        let user = User::new("user1".to_string(), "alice".to_string());
        registry.add_user(user);

        let retrieved = registry.get_user_by_name("alice");
        assert!(retrieved.is_some());
    }

    #[test]
    fn test_rbac_registry_assign_role() {
        let mut registry = RbacRegistry::new();
        registry.add_role(admin_role());
        let user = User::new("user1".to_string(), "alice".to_string());
        registry.add_user(user);

        registry.assign_role("user1", "admin").unwrap();

        let user = registry.get_user("user1").unwrap();
        assert!(user.roles.contains(&"admin".to_string()));
    }

    #[test]
    fn test_rbac_registry_assign_role_not_found() {
        let mut registry = RbacRegistry::new();
        let user = User::new("user1".to_string(), "alice".to_string());
        registry.add_user(user);

        let result = registry.assign_role("user1", "nonexistent");
        assert!(matches!(result, Err(RbacError::RoleNotFound(_))));
    }

    #[test]
    fn test_rbac_registry_check_permission_admin() {
        let mut registry = RbacRegistry::new();
        registry.add_role(admin_role());
        let user = User::new("user1".to_string(), "alice".to_string());
        registry.add_user(user);
        registry.assign_role("user1", "admin").unwrap();

        let result = registry.check_permission("user1", &Permission::DrainNodes);
        assert!(result.is_ok());
    }

    #[test]
    fn test_rbac_registry_check_permission_denied() {
        let mut registry = RbacRegistry::new();
        registry.add_role(viewer_role());
        let user = User::new("user1".to_string(), "alice".to_string());
        registry.add_user(user);
        registry.assign_role("user1", "viewer").unwrap();

        let result = registry.check_permission("user1", &Permission::DrainNodes);
        assert!(matches!(result, Err(RbacError::PermissionDenied { .. })));
    }

    #[test]
    fn test_rbac_registry_revoke_role() {
        let mut registry = RbacRegistry::new();
        registry.add_role(admin_role());
        let user = User::new("user1".to_string(), "alice".to_string());
        registry.add_user(user);
        registry.assign_role("user1", "admin").unwrap();

        registry.revoke_role("user1", "admin").unwrap();

        let user = registry.get_user("user1").unwrap();
        assert!(!user.roles.contains(&"admin".to_string()));
    }

    #[test]
    fn test_rbac_registry_user_permissions() {
        let mut registry = RbacRegistry::new();
        registry.add_role(viewer_role());
        registry.add_role(operator_role());
        let user = User::new("user1".to_string(), "alice".to_string());
        registry.add_user(user);
        registry.assign_role("user1", "viewer").unwrap();
        registry.assign_role("user1", "operator").unwrap();

        let perms = registry.user_permissions("user1");
        assert!(perms.contains(&Permission::ViewCluster));
        assert!(perms.contains(&Permission::DrainNodes));
    }

    #[test]
    fn test_rbac_registry_with_builtin_roles() {
        let registry = RbacRegistry::new().with_builtin_roles();
        assert_eq!(registry.role_count(), 4);
        assert!(registry.get_role("admin").is_some());
        assert!(registry.get_role("operator").is_some());
        assert!(registry.get_role("viewer").is_some());
        assert!(registry.get_role("tenant_admin").is_some());
    }

    #[test]
    fn test_rbac_registry_user_count() {
        let mut registry = RbacRegistry::new();
        registry.add_user(User::new("user1".to_string(), "alice".to_string()));
        registry.add_user(User::new("user2".to_string(), "bob".to_string()));
        assert_eq!(registry.user_count(), 2);
    }

    #[test]
    fn test_rbac_registry_role_count() {
        let mut registry = RbacRegistry::new();
        registry.add_role(admin_role());
        registry.add_role(operator_role());
        assert_eq!(registry.role_count(), 2);
    }
}
