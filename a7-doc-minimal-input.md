# A7 Documentation Phase 3: Minimal Critical APIs Only

## Goal
Add documentation to the top 50 most-used public items to provide value without massive changes. Focus only on:
1. Main struct types (the "face" of each module)
2. Key methods (new, important operations)
3. Enum types and their variants
4. Leave fields with #[allow(missing_docs)] if needed

Target: Document ~50 key items, reduce warnings by ~100-150 (1440 → 1290-1340 range)

## Key Modules (Conservative)

### auth.rs
- `pub enum AuthMethod` - document all variants
- `pub struct AuthContext` - document struct and new method
- `pub fn authenticate()` - document

### config.rs
- `pub struct GatewayConfig` - document fields
- `pub fn load_config()` - document

### export_manager.rs
- `pub struct ExportManager` - document
- `pub struct ExportEntry` - document key fields

### health.rs
- `pub enum HealthStatus` - document variants
- `pub struct HealthCheck` - document

### mount.rs
- `pub struct MountPoint` - document

### nfs.rs
- `pub enum NfsVersion` - document
- `pub struct NfsRequest` - document
- `pub struct NfsResponse` - document

### pnfs.rs
- `pub struct PnfsLayout` - document
- `pub enum LayoutType` - document variants

### quota.rs
- `pub struct QuotaInfo` - document
- `pub struct QuotaManager` - document

### s3.rs
- `pub struct S3Request` - document
- `pub struct S3Response` - document
- `pub enum S3Operation` - document variants

### server.rs
- `pub struct GatewayServer` - document
- `pub fn start()` - document
- `pub fn shutdown()` - document

### session.rs
- `pub struct Session` - document
- `pub enum SessionState` - document

### stats.rs
- `pub struct Statistics` - document

## Approach
For each type/item:
- Add a 1-3 line doc comment explaining what it is and what it does
- Structs: Document the struct and key fields (not all fields if 10+)
- Enums: Document each variant briefly
- Methods: Focus on new() and main operations
- Don't change any implementation

## Expected Result
- ~50 documented items
- ~100-150 warnings reduced
- All 1007 tests passing
- Minimal diff (only doc comments added)

Just return the complete files with doc comments added. Keep it pragmatic - better to have 50 well-documented items than 1465 half-assed docs.
