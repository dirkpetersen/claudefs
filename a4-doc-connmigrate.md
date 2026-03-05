Add missing Rust doc comments to the connmigrate.rs module for the claudefs-transport crate.

The file has #![warn(missing_docs)] enabled, so ALL public items need doc comments.

Rules:
- Add `/// <doc comment>` immediately before each public item that lacks one
- Do NOT modify any existing code, logic, tests, or existing doc comments
- Do NOT add comments to private/internal items (only pub items)
- Keep doc comments concise and accurate to what the code does
- Output the COMPLETE file with ALL added doc comments

Items specifically needing docs:

MigrationRecord struct fields:
- id: Unique migration operation identifier
- source: Source connection being migrated from
- target: Target connection being migrated to
- reason: Reason this migration was initiated
- state: Current state of this migration operation
- requests_migrated: Number of in-flight requests successfully migrated
- requests_failed: Number of requests that failed during migration
- started_at_ms: Timestamp when migration was started (milliseconds since UNIX epoch)
- completed_at_ms: Timestamp when migration completed (None if still in progress)

MigrationConfig struct fields:
- max_concurrent_migrations: Maximum number of migrations that can run simultaneously
- migration_timeout_ms: Maximum time allowed for a migration to complete (milliseconds)
- retry_failed_requests: Whether to retry requests that fail during migration
- max_retries: Maximum number of retry attempts for failed requests
- quiesce_timeout_ms: Time to wait for in-flight requests to complete before migrating (milliseconds)
- enabled: Whether connection migration is enabled

MigrationStats methods:
- new: Creates new zeroed migration statistics
- snapshot: Returns a snapshot of current statistics
- increment_total: Increments total migration count
- increment_successful: Increments successful migration count
- increment_failed: Increments failed migration count
- add_requests_migrated: Adds to the count of migrated requests
- add_requests_failed: Adds to the count of failed requests

MigrationStatsSnapshot struct fields:
- total_migrations: Total number of migration operations initiated
- successful_migrations: Number of migrations that completed successfully
- failed_migrations: Number of migrations that failed
- requests_migrated: Total requests successfully migrated across all operations
- requests_failed: Total requests that failed during migration
- active_migrations: Number of currently active migrations

MigrationManager methods:
- new: Creates a new migration manager with the given configuration
- start_migration: Initiates a migration from source to target connection
- record_request_migrated: Records that one request was successfully migrated
- record_request_failed: Records that one request failed during migration
- complete_migration: Marks a migration as successfully completed
- fail_migration: Marks a migration as failed
- get_migration: Returns the migration record for the given ID
- active_count: Returns the number of currently active (non-terminal) migrations
- is_migrating: Returns whether the given connection is currently being migrated

Please output the COMPLETE connmigrate.rs file with all the missing doc comments added.
Output ONLY the Rust source code with no markdown fences.
