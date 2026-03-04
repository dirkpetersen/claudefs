[0m
> build · accounts/fireworks/models/minimax-m2p5
[0m
[0m→ [0mRead crates/claudefs-storage/src/nvme_passthrough.rs
[0m
[0m← [0mEdit crates/claudefs-storage/src/nvme_passthrough.rs
Index: /home/cfs/claudefs/crates/claudefs-storage/src/nvme_passthrough.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-storage/src/nvme_passthrough.rs
+++ /home/cfs/claudefs/crates/claudefs-storage/src/nvme_passthrough.rs
@@ -1,9 +1,8 @@
 //! NVMe passthrough queue alignment for production workloads.
 
 use serde::{Deserialize, Serialize};
 use std::collections::HashMap;
-use thiserror::Error;
 use tracing::{debug, error, info, warn};
 
 #[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Serialize, Deserialize)]
 pub struct QueuePairId(pub u32);

[0m
Done. Removed the unused `use thiserror::Error;` import.
