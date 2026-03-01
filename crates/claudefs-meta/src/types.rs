use serde::{Deserialize, Serialize};
use std::fmt;

/// Represents a unique identifier for an inode in the metadata service
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InodeId(u64);

impl InodeId {
    /// The root inode ID (always 1)
    pub const ROOT_INODE: InodeId = InodeId(1);

    /// Creates a new InodeId from a raw u64 value
    pub fn new(id: u64) -> Self {
        InodeId(id)
    }

    /// Returns the raw u64 value of this inode ID
    pub fn as_u64(&self) -> u64 {
        self.0
    }

    /// Computes the shard ID for this inode using the given number of shards
    pub fn shard(self, num_shards: u16) -> ShardId {
        ShardId((self.0 % num_shards as u64) as u16)
    }
}

impl fmt::Display for InodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Represents a unique identifier for a metadata server node in the cluster
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(u64);

impl NodeId {
    /// Creates a new NodeId from a raw u64 value
    pub fn new(id: u64) -> Self {
        NodeId(id)
    }

    /// Returns the raw u64 value of this node ID
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Represents a shard identifier for metadata partitioning (256 default shards per decision D4)
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ShardId(u16);

impl ShardId {
    /// Creates a new ShardId from a raw u16 value
    pub fn new(id: u16) -> Self {
        ShardId(id)
    }

    /// Returns the raw u16 value of this shard ID
    pub fn as_u16(&self) -> u16 {
        self.0
    }
}

impl fmt::Display for ShardId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Represents a Raft term number for leader election
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Term(u64);

impl Term {
    /// Creates a new Term from a raw u64 value
    pub fn new(t: u64) -> Self {
        Term(t)
    }

    /// Returns the raw u64 value of this term
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Represents a Raft log index
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LogIndex(u64);

impl LogIndex {
    /// A zero log index
    pub const ZERO: LogIndex = LogIndex(0);

    /// Creates a new LogIndex from a raw u64 value
    pub fn new(i: u64) -> Self {
        LogIndex(i)
    }

    /// Returns the raw u64 value of this log index
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl fmt::Display for LogIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Represents a point in time with second and nanosecond precision
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timestamp {
    /// Seconds since Unix epoch
    pub secs: u64,
    /// Nanoseconds within the second
    pub nanos: u32,
}

impl Timestamp {
    /// Returns the current timestamp
    pub fn now() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before epoch");
        Self {
            secs: now.as_secs(),
            nanos: now.subsec_nanos(),
        }
    }
}

impl Ord for Timestamp {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.secs
            .cmp(&other.secs)
            .then_with(|| self.nanos.cmp(&other.nanos))
    }
}

impl PartialOrd for Timestamp {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Lamport timestamp for cross-site conflict resolution in distributed metadata replication
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorClock {
    /// Site identifier for distributed coordination
    pub site_id: u64,
    /// Sequence number for Lamport timestamp
    pub sequence: u64,
}

impl VectorClock {
    /// Creates a new vector clock with the given site ID and sequence number
    pub fn new(site_id: u64, sequence: u64) -> Self {
        Self { site_id, sequence }
    }
}

impl Ord for VectorClock {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.sequence
            .cmp(&other.sequence)
            .then_with(|| self.site_id.cmp(&other.site_id))
    }
}

impl PartialOrd for VectorClock {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Error types for metadata operations in the distributed metadata service
#[derive(Debug, thiserror::Error)]
pub enum MetaError {
    /// The requested inode does not exist.
    #[error("inode {0} not found")]
    InodeNotFound(InodeId),

    /// The requested directory inode does not exist.
    #[error("directory inode {0} not found")]
    DirectoryNotFound(InodeId),

    /// A directory entry with the given name was not found.
    #[error("entry '{name}' not found in directory {parent}")]
    EntryNotFound {
        /// Parent directory inode
        parent: InodeId,
        /// Entry name that was not found
        name: String,
    },

    /// A directory entry with the given name already exists.
    #[error("entry '{name}' already exists in directory {parent}")]
    EntryExists {
        /// Parent directory inode
        parent: InodeId,
        /// Existing entry name
        name: String,
    },

    /// The specified inode is not a directory when a directory was required.
    #[error("inode {0} is not a directory")]
    NotADirectory(InodeId),

    /// Attempted to delete a non-empty directory.
    #[error("directory {0} is not empty")]
    DirectoryNotEmpty(InodeId),

    /// No space left on device (metadata quota exceeded or storage full).
    #[error("no space left on device")]
    NoSpace,

    /// Operation denied due to insufficient permissions.
    #[error("permission denied")]
    PermissionDenied,

    /// Operation requires the Raft leader but this node is not the leader.
    #[error("not the Raft leader")]
    NotLeader {
        /// Hint about the current leader
        leader_hint: Option<NodeId>,
    },

    /// An error occurred in the Raft consensus layer.
    #[error("raft error: {0}")]
    RaftError(String),

    /// An error occurred in the KV store layer.
    #[error("kv store error: {0}")]
    KvError(String),

    /// A lower-level I/O error occurred.
    #[error(transparent)]
    IoError(#[from] std::io::Error),
}

/// File type enumeration matching POSIX file types
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileType {
    /// Regular file (S_IFREG)
    RegularFile,
    /// Directory (S_IFDIR)
    Directory,
    /// Symbolic link (S_IFLNK)
    Symlink,
    /// Block device (S_IFBLK)
    BlockDevice,
    /// Character device (S_IFCHR)
    CharDevice,
    /// FIFO/named pipe (S_IFIFO)
    Fifo,
    /// Socket (S_IFSOCK)
    Socket,
}

impl FileType {
    /// Returns the POSIX S_IFMT bits for this file type
    pub fn mode_bits(&self) -> u32 {
        match self {
            FileType::RegularFile => 0o100000,
            FileType::Directory => 0o040000,
            FileType::Symlink => 0o120000,
            FileType::BlockDevice => 0o060000,
            FileType::CharDevice => 0o020000,
            FileType::Fifo => 0o010000,
            FileType::Socket => 0o140000,
        }
    }
}

/// Replication state for cross-site metadata synchronization
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplicationState {
    /// Metadata exists only locally
    Local,
    /// Replication in progress
    Pending,
    /// Metadata replicated to other sites
    Replicated,
    /// Write conflict detected during replication
    Conflict,
}

/// Inode attributes combining POSIX stat fields with ClaudeFS extensions
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InodeAttr {
    /// Inode number
    pub ino: InodeId,
    /// File type
    pub file_type: FileType,
    /// Permission bits (lower 12 bits)
    pub mode: u32,
    /// Hard link count
    pub nlink: u32,
    /// Owner user ID
    pub uid: u32,
    /// Owner group ID
    pub gid: u32,
    /// File size in bytes
    pub size: u64,
    /// 512-byte blocks allocated
    pub blocks: u64,
    /// Last access time
    pub atime: Timestamp,
    /// Last modification time
    pub mtime: Timestamp,
    /// Last status change time
    pub ctime: Timestamp,
    /// Creation time (Linux statx)
    pub crtime: Timestamp,
    /// BLAKE3 hash of content
    pub content_hash: Option<[u8; 32]>,
    /// Replication state
    pub repl_state: ReplicationState,
    /// Vector clock for conflict resolution
    pub vector_clock: VectorClock,
    /// Inode generation number (for NFS handle stability)
    pub generation: u64,
    /// Symlink target path (only valid for FileType::Symlink)
    pub symlink_target: Option<String>,
}

impl InodeAttr {
    /// Creates a new directory inode with sensible defaults
    pub fn new_directory(ino: InodeId, uid: u32, gid: u32, mode: u32, site_id: u64) -> Self {
        let now = Timestamp::now();
        Self {
            ino,
            file_type: FileType::Directory,
            mode,
            nlink: 2,
            uid,
            gid,
            size: 0,
            blocks: 0,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            content_hash: None,
            repl_state: ReplicationState::Local,
            vector_clock: VectorClock::new(site_id, 0),
            generation: 0,
            symlink_target: None,
        }
    }

    /// Creates a new file inode with sensible defaults
    pub fn new_file(ino: InodeId, uid: u32, gid: u32, mode: u32, site_id: u64) -> Self {
        let now = Timestamp::now();
        Self {
            ino,
            file_type: FileType::RegularFile,
            mode,
            nlink: 1,
            uid,
            gid,
            size: 0,
            blocks: 0,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            content_hash: None,
            repl_state: ReplicationState::Local,
            vector_clock: VectorClock::new(site_id, 0),
            generation: 0,
            symlink_target: None,
        }
    }

    /// Creates a new symlink inode
    pub fn new_symlink(
        ino: InodeId,
        uid: u32,
        gid: u32,
        mode: u32,
        site_id: u64,
        target: String,
    ) -> Self {
        let now = Timestamp::now();
        Self {
            ino,
            file_type: FileType::Symlink,
            mode,
            nlink: 1,
            uid,
            gid,
            size: target.len() as u64,
            blocks: 0,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            content_hash: None,
            repl_state: ReplicationState::Local,
            vector_clock: VectorClock::new(site_id, 0),
            generation: 0,
            symlink_target: Some(target),
        }
    }
}

/// A directory entry linking a name to an inode
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirEntry {
    /// Entry name
    pub name: String,
    /// Inode number
    pub ino: InodeId,
    /// File type
    pub file_type: FileType,
}

/// Metadata operations recorded in the replication journal
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MetaOp {
    /// Create a new inode
    CreateInode {
        /// Inode attributes
        attr: InodeAttr,
    },
    /// Delete an inode
    DeleteInode {
        /// Inode ID to delete
        ino: InodeId,
    },
    /// Set inode attributes
    SetAttr {
        /// Target inode
        ino: InodeId,
        /// New attributes
        attr: InodeAttr,
    },
    /// Create a directory entry
    CreateEntry {
        /// Parent directory inode
        parent: InodeId,
        /// Entry name
        name: String,
        /// Directory entry
        entry: DirEntry,
    },
    /// Delete a directory entry
    DeleteEntry {
        /// Parent directory inode
        parent: InodeId,
        /// Entry name to delete
        name: String,
    },
    /// Rename a directory entry
    Rename {
        /// Source parent directory
        src_parent: InodeId,
        /// Source name
        src_name: String,
        /// Destination parent directory
        dst_parent: InodeId,
        /// Destination name
        dst_name: String,
    },
    /// Set extended attribute
    SetXattr {
        /// Target inode
        ino: InodeId,
        /// Attribute key
        key: String,
        /// Attribute value
        value: Vec<u8>,
    },
    /// Remove extended attribute
    RemoveXattr {
        /// Target inode
        ino: InodeId,
        /// Attribute key
        key: String,
    },
    /// Create a hard link
    Link {
        /// Parent directory for the new link
        parent: InodeId,
        /// Name of the new link
        name: String,
        /// Target inode
        ino: InodeId,
    },
}

/// A single entry in the Raft log
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogEntry {
    /// Log index
    pub index: LogIndex,
    /// Term when entry was created
    pub term: Term,
    /// Operation to apply
    pub op: MetaOp,
}

/// Messages exchanged between Raft peers
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RaftMessage {
    /// Request for vote from candidate
    RequestVote {
        /// Candidate's term
        term: Term,
        /// Candidate node ID
        candidate_id: NodeId,
        /// Index of candidate's last log entry
        last_log_index: LogIndex,
        /// Term of candidate's last log entry
        last_log_term: Term,
    },
    /// Response to RequestVote
    RequestVoteResponse {
        /// Responder's term
        term: Term,
        /// Whether vote was granted
        vote_granted: bool,
    },
    /// Append entries from leader to follower
    AppendEntries {
        /// Leader's term
        term: Term,
        /// Leader node ID
        leader_id: NodeId,
        /// Index of log entry preceding new entries
        prev_log_index: LogIndex,
        /// Term of prev_log_index entry
        prev_log_term: Term,
        /// Log entries to append
        entries: Vec<LogEntry>,
        /// Leader's commit index
        leader_commit: LogIndex,
    },
    /// Response to AppendEntries
    AppendEntriesResponse {
        /// Follower's term
        term: Term,
        /// Whether append succeeded
        success: bool,
        /// Match index for leader
        match_index: LogIndex,
    },
    /// Pre-vote request (Raft thesis §9.6) — sent before real election to avoid disruption
    PreVote {
        /// Candidate's term (would-be next term, NOT incremented yet)
        term: Term,
        /// Candidate node ID
        candidate_id: NodeId,
        /// Index of candidate's last log entry
        last_log_index: LogIndex,
        /// Term of candidate's last log entry
        last_log_term: Term,
    },
    /// Response to PreVote
    PreVoteResponse {
        /// Responder's term
        term: Term,
        /// Whether pre-vote was granted
        vote_granted: bool,
    },
    /// Leadership transfer request (Raft thesis §6.4)
    TimeoutNow {
        /// Leader's term
        term: Term,
        /// Current leader ID
        leader_id: NodeId,
    },
}

/// Current state of a Raft node
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RaftState {
    /// Following a leader
    Follower,
    /// Campaigning for leadership
    Candidate,
    /// Leading the cluster
    Leader,
    /// Pre-candidate: gathering pre-votes before starting real election
    PreCandidate,
}
