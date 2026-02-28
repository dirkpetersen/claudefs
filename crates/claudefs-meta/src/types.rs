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
    /// Creates a new LogIndex from a raw u64 value
    pub fn new(i: u64) -> Self {
        LogIndex(i)
    }

    /// Returns the raw u64 value of this log index
    pub fn as_u64(&self) -> u64 {
        self.0
    }

    /// A zero log index
    pub const ZERO: LogIndex = LogIndex(0);
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
    #[error("inode {0} not found")]
    InodeNotFound(InodeId),

    #[error("directory inode {0} not found")]
    DirectoryNotFound(InodeId),

    #[error("entry '{name}' not found in directory {parent}")]
    EntryNotFound { parent: InodeId, name: String },

    #[error("entry '{name}' already exists in directory {parent}")]
    EntryExists { parent: InodeId, name: String },

    #[error("inode {0} is not a directory")]
    NotADirectory(InodeId),

    #[error("directory {0} is not empty")]
    DirectoryNotEmpty(InodeId),

    #[error("no space left on device")]
    NoSpace,

    #[error("permission denied")]
    PermissionDenied,

    #[error("not the Raft leader")]
    NotLeader { leader_hint: Option<NodeId> },

    #[error("raft error: {0}")]
    RaftError(String),

    #[error("kv store error: {0}")]
    KvError(String),

    #[error(transparent)]
    IoError(#[from] std::io::Error),
}

/// File type enumeration matching POSIX file types
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileType {
    RegularFile,
    Directory,
    Symlink,
    BlockDevice,
    CharDevice,
    Fifo,
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
    Local,
    Pending,
    Replicated,
    Conflict,
}

/// Inode attributes combining POSIX stat fields with ClaudeFS extensions
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InodeAttr {
    pub ino: InodeId,
    pub file_type: FileType,
    pub mode: u32,
    pub nlink: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub blocks: u64,
    pub atime: Timestamp,
    pub mtime: Timestamp,
    pub ctime: Timestamp,
    pub crtime: Timestamp,
    pub content_hash: Option<[u8; 32]>,
    pub repl_state: ReplicationState,
    pub vector_clock: VectorClock,
    pub generation: u64,
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
        }
    }
}

/// A directory entry linking a name to an inode
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirEntry {
    pub name: String,
    pub ino: InodeId,
    pub file_type: FileType,
}

/// Metadata operations recorded in the replication journal
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MetaOp {
    CreateInode {
        attr: InodeAttr,
    },
    DeleteInode {
        ino: InodeId,
    },
    SetAttr {
        ino: InodeId,
        attr: InodeAttr,
    },
    CreateEntry {
        parent: InodeId,
        name: String,
        entry: DirEntry,
    },
    DeleteEntry {
        parent: InodeId,
        name: String,
    },
    Rename {
        src_parent: InodeId,
        src_name: String,
        dst_parent: InodeId,
        dst_name: String,
    },
    SetXattr {
        ino: InodeId,
        key: String,
        value: Vec<u8>,
    },
    RemoveXattr {
        ino: InodeId,
        key: String,
    },
}

/// A single entry in the Raft log
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogEntry {
    pub index: LogIndex,
    pub term: Term,
    pub op: MetaOp,
}

/// Messages exchanged between Raft peers
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RaftMessage {
    RequestVote {
        term: Term,
        candidate_id: NodeId,
        last_log_index: LogIndex,
        last_log_term: Term,
    },
    RequestVoteResponse {
        term: Term,
        vote_granted: bool,
    },
    AppendEntries {
        term: Term,
        leader_id: NodeId,
        prev_log_index: LogIndex,
        prev_log_term: Term,
        entries: Vec<LogEntry>,
        leader_commit: LogIndex,
    },
    AppendEntriesResponse {
        term: Term,
        success: bool,
        match_index: LogIndex,
    },
}

/// Current state of a Raft node
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RaftState {
    Follower,
    Candidate,
    Leader,
}
