//! RPC message types for ClaudeFS inter-node communication.
//!
//! This module defines all request and response message types for the RPC protocol.
//! Messages are serialized using bincode for efficient wire encoding.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{Result, TransportError};

/// Serialize any serde-compatible message to bytes using bincode.
pub fn serialize_message<T: Serialize>(msg: &T) -> Result<Vec<u8>> {
    bincode::serialize(msg).map_err(|e| TransportError::SerializationError(e.to_string()))
}

/// Deserialize bytes to a message using bincode.
pub fn deserialize_message<T: DeserializeOwned>(data: &[u8]) -> Result<T> {
    bincode::deserialize(data).map_err(|e| TransportError::SerializationError(e.to_string()))
}

// ============================================================================
// Metadata operations (0x01xx)
// ============================================================================

/// Lookup request - find a file by name in a directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookupRequest {
    /// Inode number of the parent directory.
    pub parent_inode: u64,
    /// Name of the file to look up.
    pub name: String,
}

/// Lookup response - returns file attributes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookupResponse {
    /// Inode number of the found file.
    pub inode: u64,
    /// File mode (permissions and type).
    pub mode: u32,
    /// File size in bytes.
    pub size: u64,
    /// Owner user ID.
    pub uid: u32,
    /// Owner group ID.
    pub gid: u32,
    /// Modification time (seconds since epoch).
    pub mtime_secs: i64,
    /// Modification time (nanoseconds).
    pub mtime_nsecs: u32,
}

/// Create request - create a new regular file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRequest {
    /// Inode number of the parent directory.
    pub parent_inode: u64,
    /// Name of the file to create.
    pub name: String,
    /// File mode (permissions).
    pub mode: u32,
    /// Owner user ID.
    pub uid: u32,
    /// Owner group ID.
    pub gid: u32,
}

/// Create response - returns the created file's inode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateResponse {
    /// Inode number of the newly created file.
    pub inode: u64,
}

/// Mkdir request - create a new directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MkdirRequest {
    /// Inode number of the parent directory.
    pub parent_inode: u64,
    /// Name of the directory to create.
    pub name: String,
    /// Directory mode (permissions).
    pub mode: u32,
    /// Owner user ID.
    pub uid: u32,
    /// Owner group ID.
    pub gid: u32,
}

/// Mkdir response - returns the created directory's inode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MkdirResponse {
    /// Inode number of the newly created directory.
    pub inode: u64,
}

/// Unlink request - remove a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlinkRequest {
    /// Inode number of the parent directory.
    pub parent_inode: u64,
    /// Name of the file to remove.
    pub name: String,
}

/// Unlink response - empty response for successful unlink.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlinkResponse {}

/// Rmdir request - remove an empty directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RmdirRequest {
    /// Inode number of the parent directory.
    pub parent_inode: u64,
    /// Name of the directory to remove.
    pub name: String,
}

/// Rmdir response - empty response for successful rmdir.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RmdirResponse {}

/// Rename request - rename a file or directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameRequest {
    /// Inode number of the old parent directory.
    pub old_parent: u64,
    /// Original name.
    pub old_name: String,
    /// Inode number of the new parent directory.
    pub new_parent: u64,
    /// New name.
    pub new_name: String,
}

/// Rename response - empty response for successful rename.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameResponse {}

/// Getattr request - get file attributes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetattrRequest {
    /// Inode number of the file.
    pub inode: u64,
}

/// Getattr response - returns all file attributes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetattrResponse {
    /// Inode number of the file.
    pub inode: u32,
    /// File mode (permissions and type).
    pub mode: u32,
    /// Number of hard links.
    pub nlink: u32,
    /// Owner user ID.
    pub uid: u32,
    /// Owner group ID.
    pub gid: u32,
    /// File size in bytes.
    pub size: u64,
    /// Number of 512-byte blocks allocated.
    pub blocks: u64,
    /// Access time (seconds since epoch).
    pub atime_secs: i64,
    /// Access time (nanoseconds).
    pub atime_nsecs: u32,
    /// Modification time (seconds since epoch).
    pub mtime_secs: i64,
    /// Modification time (nanoseconds).
    pub mtime_nsecs: u32,
    /// Change time (seconds since epoch).
    pub ctime_secs: i64,
    /// Change time (nanoseconds).
    pub ctime_nsecs: u32,
}

/// Setattr request - set file attributes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetattrRequest {
    /// Inode number of the file.
    pub inode: u64,
    /// New mode (permissions), if Some.
    pub mode: Option<u32>,
    /// New owner user ID, if Some.
    pub uid: Option<u32>,
    /// New owner group ID, if Some.
    pub gid: Option<u32>,
    /// New file size, if Some.
    pub size: Option<u64>,
    /// New access time (seconds), if Some.
    pub atime_secs: Option<i64>,
    /// New access time (nanoseconds), if Some.
    pub atime_nsecs: Option<u32>,
    /// New modification time (seconds), if Some.
    pub mtime_secs: Option<i64>,
    /// New modification time (nanoseconds), if Some.
    pub mtime_nsecs: Option<u32>,
}

/// Setattr response - empty response for successful setattr.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetattrResponse {}

/// Readdir request - read directory entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReaddirRequest {
    /// Inode number of the directory.
    pub inode: u64,
    /// Offset to start reading from.
    pub offset: u64,
}

/// A single directory entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReaddirEntry {
    /// Inode number of the entry.
    pub inode: u64,
    /// Name of the entry.
    pub name: String,
    /// File type (0=unknown, 1=regular, 2=directory, 3=symlink, etc.).
    pub file_type: u8,
}

/// Readdir response - returns directory entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReaddirResponse {
    /// List of directory entries.
    pub entries: Vec<ReaddirEntry>,
}

/// Symlink request - create a symbolic link.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymlinkRequest {
    /// Inode number of the parent directory.
    pub parent_inode: u64,
    /// Name of the symbolic link.
    pub name: String,
    /// Target path the symlink points to.
    pub target: String,
}

/// Symlink response - returns the created symlink's inode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymlinkResponse {
    /// Inode number of the newly created symbolic link.
    pub inode: u64,
}

/// Readlink request - read the target of a symbolic link.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadlinkRequest {
    /// Inode number of the symbolic link.
    pub inode: u64,
}

/// Readlink response - returns the symlink target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadlinkResponse {
    /// Target path the symlink points to.
    pub target: String,
}

/// Link request - create a hard link.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkRequest {
    /// Inode number of the existing file.
    pub inode: u64,
    /// Inode number of the new parent directory.
    pub new_parent: u64,
    /// Name for the new hard link.
    pub new_name: String,
}

/// Link response - empty response for successful link.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkResponse {}

/// Statfs request - get filesystem statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatfsRequest {}

/// Statfs response - returns filesystem statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatfsResponse {
    /// Total number of blocks.
    pub total_blocks: u64,
    /// Number of free blocks.
    pub free_blocks: u64,
    /// Number of available blocks (for non-privileged users).
    pub available_blocks: u64,
    /// Total number of inodes.
    pub total_inodes: u64,
    /// Number of free inodes.
    pub free_inodes: u64,
    /// Block size in bytes.
    pub block_size: u32,
    /// Maximum filename length.
    pub max_name_length: u32,
}

// ============================================================================
// Data operations (0x02xx)
// ============================================================================

/// Read request - read file data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadRequest {
    /// Inode number of the file.
    pub inode: u64,
    /// Offset to start reading from.
    pub offset: u64,
    /// Number of bytes to read.
    pub size: u32,
}

/// Read response - returns the requested data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadResponse {
    /// Data bytes read from the file.
    pub data: Vec<u8>,
}

/// Write request - write file data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteRequest {
    /// Inode number of the file.
    pub inode: u64,
    /// Offset to start writing at.
    pub offset: u64,
    /// Data bytes to write.
    pub data: Vec<u8>,
}

/// Write response - returns the number of bytes written.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteResponse {
    /// Number of bytes successfully written.
    pub bytes_written: u32,
}

/// Fsync request - synchronize file data to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsyncRequest {
    /// Inode number of the file.
    pub inode: u64,
    /// If true, only sync user data (not metadata).
    pub datasync: bool,
}

/// Fsync response - empty response for successful fsync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsyncResponse {}

/// Fallocate request - allocate file space.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallocateRequest {
    /// Inode number of the file.
    pub inode: u64,
    /// Offset to start allocating from.
    pub offset: u64,
    /// Number of bytes to allocate.
    pub length: u64,
    /// Allocation mode flags.
    pub mode: u32,
}

/// Fallocate response - empty response for successful fallocate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallocateResponse {}

/// Open request - open a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenRequest {
    /// Inode number of the file.
    pub inode: u64,
    /// Open flags (O_RDONLY, O_WRONLY, O_RDWR, O_CREAT, etc.).
    pub flags: u32,
}

/// Open response - returns a file handle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenResponse {
    /// File handle for the open file.
    pub file_handle: u64,
}

/// Close request - close a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseRequest {
    /// File handle to close.
    pub file_handle: u64,
}

/// Close response - empty response for successful close.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseResponse {}

// ============================================================================
// Cluster operations (0x03xx)
// ============================================================================

/// Heartbeat request - periodic health check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatRequest {
    /// Node ID of the sending node.
    pub node_id: u64,
    /// Timestamp in milliseconds since epoch.
    pub timestamp_ms: u64,
    /// CPU load percentage (0-100).
    pub load_pct: u8,
}

/// Heartbeat response - acknowledges the heartbeat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatResponse {
    /// Node ID of the responding node.
    pub node_id: u64,
    /// Timestamp in milliseconds since epoch.
    pub timestamp_ms: u64,
}

/// Join cluster request - a node joining the cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinClusterRequest {
    /// Node ID of the joining node.
    pub node_id: u64,
    /// Network address of the joining node.
    pub addr: String,
    /// Authentication token.
    pub token: String,
}

/// Join cluster response - result of the join request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinClusterResponse {
    /// Whether the join request was accepted.
    pub accepted: bool,
    /// Cluster ID.
    pub cluster_id: u64,
    /// Current cluster members.
    pub members: Vec<ClusterMember>,
}

/// A member node in the cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterMember {
    /// Node ID of the member.
    pub node_id: u64,
    /// Network address of the member.
    pub addr: String,
    /// Role (0=storage, 1=metadata, 2=conduit, etc.).
    pub role: u8,
}

/// Leave cluster request - a node leaving the cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaveClusterRequest {
    /// Node ID of the leaving node.
    pub node_id: u64,
}

/// Leave cluster response - empty response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaveClusterResponse {}

/// Shard info request - query shard metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardInfoRequest {
    /// Shard ID to query.
    pub shard_id: u32,
}

/// Shard info response - returns shard metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardInfoResponse {
    /// Shard ID.
    pub shard_id: u32,
    /// Node ID of the shard leader.
    pub leader_node: u64,
    /// List of replica node IDs.
    pub replicas: Vec<u64>,
}

/// Node status request - query node status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatusRequest {
    /// Node ID to query.
    pub node_id: u64,
}

/// Node status response - returns node status information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatusResponse {
    /// Node ID.
    pub node_id: u64,
    /// Node state (0=offline, 1=online, 2=degraded, etc.).
    pub state: u8,
    /// Uptime in seconds.
    pub uptime_secs: u64,
    /// Total disk space in bytes.
    pub total_space: u64,
    /// Used disk space in bytes.
    pub used_space: u64,
    /// IO operations per second.
    pub iops: u64,
}

// ============================================================================
// Replication operations (0x04xx)
// ============================================================================

/// Journal sync request - synchronize journal entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalSyncRequest {
    /// Source node ID.
    pub source_node: u64,
    /// Journal offset to sync from.
    pub journal_offset: u64,
    /// Encoded journal entries.
    pub entries: Vec<u8>,
}

/// Journal sync response - acknowledges the sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalSyncResponse {
    /// Last acknowledged offset.
    pub acked_offset: u64,
}

/// Journal ack request - acknowledge journal receipt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalAckRequest {
    /// Node ID sending the acknowledgment.
    pub node_id: u64,
    /// Last acknowledged offset.
    pub acked_offset: u64,
}

/// Journal ack response - empty response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalAckResponse {}

/// Snapshot transfer request - transfer a snapshot chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotTransferRequest {
    /// Snapshot ID.
    pub snapshot_id: u64,
    /// Offset of this chunk in the snapshot.
    pub chunk_offset: u64,
    /// Chunk data bytes.
    pub chunk_data: Vec<u8>,
    /// Whether this is the last chunk.
    pub is_last: bool,
}

/// Snapshot transfer response - acknowledges the chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotTransferResponse {
    /// Number of bytes received.
    pub bytes_received: u64,
}

// ============================================================================
// RpcMessage enum
// ============================================================================

/// Wraps any request or response message for dispatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RpcMessage {
    // Metadata operations
    /// Lookup request.
    LookupRequest(LookupRequest),
    /// Lookup response.
    LookupResponse(LookupResponse),
    /// Create request.
    CreateRequest(CreateRequest),
    /// Create response.
    CreateResponse(CreateResponse),
    /// Mkdir request.
    MkdirRequest(MkdirRequest),
    /// Mkdir response.
    MkdirResponse(MkdirResponse),
    /// Unlink request.
    UnlinkRequest(UnlinkRequest),
    /// Unlink response.
    UnlinkResponse(UnlinkResponse),
    /// Rmdir request.
    RmdirRequest(RmdirRequest),
    /// Rmdir response.
    RmdirResponse(RmdirResponse),
    /// Rename request.
    RenameRequest(RenameRequest),
    /// Rename response.
    RenameResponse(RenameResponse),
    /// Getattr request.
    GetattrRequest(GetattrRequest),
    /// Getattr response.
    GetattrResponse(GetattrResponse),
    /// Setattr request.
    SetattrRequest(SetattrRequest),
    /// Setattr response.
    SetattrResponse(SetattrResponse),
    /// Readdir request.
    ReaddirRequest(ReaddirRequest),
    /// Readdir response.
    ReaddirResponse(ReaddirResponse),
    /// Symlink request.
    SymlinkRequest(SymlinkRequest),
    /// Symlink response.
    SymlinkResponse(SymlinkResponse),
    /// Readlink request.
    ReadlinkRequest(ReadlinkRequest),
    /// Readlink response.
    ReadlinkResponse(ReadlinkResponse),
    /// Link request.
    LinkRequest(LinkRequest),
    /// Link response.
    LinkResponse(LinkResponse),
    /// Statfs request.
    StatfsRequest(StatfsRequest),
    /// Statfs response.
    StatfsResponse(StatfsResponse),

    // Data operations
    /// Read request.
    ReadRequest(ReadRequest),
    /// Read response.
    ReadResponse(ReadResponse),
    /// Write request.
    WriteRequest(WriteRequest),
    /// Write response.
    WriteResponse(WriteResponse),
    /// Fsync request.
    FsyncRequest(FsyncRequest),
    /// Fsync response.
    FsyncResponse(FsyncResponse),
    /// Fallocate request.
    FallocateRequest(FallocateRequest),
    /// Fallocate response.
    FallocateResponse(FallocateResponse),
    /// Open request.
    OpenRequest(OpenRequest),
    /// Open response.
    OpenResponse(OpenResponse),
    /// Close request.
    CloseRequest(CloseRequest),
    /// Close response.
    CloseResponse(CloseResponse),

    // Cluster operations
    /// Heartbeat request.
    HeartbeatRequest(HeartbeatRequest),
    /// Heartbeat response.
    HeartbeatResponse(HeartbeatResponse),
    /// Join cluster request.
    JoinClusterRequest(JoinClusterRequest),
    /// Join cluster response.
    JoinClusterResponse(JoinClusterResponse),
    /// Leave cluster request.
    LeaveClusterRequest(LeaveClusterRequest),
    /// Leave cluster response.
    LeaveClusterResponse(LeaveClusterResponse),
    /// Shard info request.
    ShardInfoRequest(ShardInfoRequest),
    /// Shard info response.
    ShardInfoResponse(ShardInfoResponse),
    /// Node status request.
    NodeStatusRequest(NodeStatusRequest),
    /// Node status response.
    NodeStatusResponse(NodeStatusResponse),

    // Replication operations
    /// Journal sync request.
    JournalSyncRequest(JournalSyncRequest),
    /// Journal sync response.
    JournalSyncResponse(JournalSyncResponse),
    /// Journal ack request.
    JournalAckRequest(JournalAckRequest),
    /// Journal ack response.
    JournalAckResponse(JournalAckResponse),
    /// Snapshot transfer request.
    SnapshotTransferRequest(SnapshotTransferRequest),
    /// Snapshot transfer response.
    SnapshotTransferResponse(SnapshotTransferResponse),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize_lookup_request() {
        let req = LookupRequest {
            parent_inode: 123,
            name: "test.txt".to_string(),
        };
        let bytes = serialize_message(&req).unwrap();
        let decoded: LookupRequest = deserialize_message(&bytes).unwrap();
        assert_eq!(decoded.parent_inode, req.parent_inode);
        assert_eq!(decoded.name, req.name);
    }

    #[test]
    fn test_serialize_deserialize_read_response() {
        let resp = ReadResponse {
            data: b"Hello, World!".to_vec(),
        };
        let bytes = serialize_message(&resp).unwrap();
        let decoded: ReadResponse = deserialize_message(&bytes).unwrap();
        assert_eq!(decoded.data, resp.data);
    }

    #[test]
    fn test_serialize_deserialize_heartbeat_request() {
        let req = HeartbeatRequest {
            node_id: 42,
            timestamp_ms: 1234567890,
            load_pct: 75,
        };
        let bytes = serialize_message(&req).unwrap();
        let decoded: HeartbeatRequest = deserialize_message(&bytes).unwrap();
        assert_eq!(decoded.node_id, req.node_id);
        assert_eq!(decoded.timestamp_ms, req.timestamp_ms);
        assert_eq!(decoded.load_pct, req.load_pct);
    }

    #[test]
    fn test_serialize_deserialize_join_cluster_response() {
        let resp = JoinClusterResponse {
            accepted: true,
            cluster_id: 1,
            members: vec![
                ClusterMember {
                    node_id: 1,
                    addr: "192.168.1.1:9000".to_string(),
                    role: 0,
                },
                ClusterMember {
                    node_id: 2,
                    addr: "192.168.1.2:9000".to_string(),
                    role: 1,
                },
            ],
        };
        let bytes = serialize_message(&resp).unwrap();
        let decoded: JoinClusterResponse = deserialize_message(&bytes).unwrap();
        assert_eq!(decoded.accepted, resp.accepted);
        assert_eq!(decoded.cluster_id, resp.cluster_id);
        assert_eq!(decoded.members.len(), 2);
    }

    #[test]
    fn test_serialize_deserialize_readdir_response() {
        let resp = ReaddirResponse {
            entries: vec![
                ReaddirEntry {
                    inode: 1,
                    name: ".".to_string(),
                    file_type: 2,
                },
                ReaddirEntry {
                    inode: 2,
                    name: "..".to_string(),
                    file_type: 2,
                },
                ReaddirEntry {
                    inode: 100,
                    name: "file.txt".to_string(),
                    file_type: 1,
                },
            ],
        };
        let bytes = serialize_message(&resp).unwrap();
        let decoded: ReaddirResponse = deserialize_message(&bytes).unwrap();
        assert_eq!(decoded.entries.len(), 3);
        assert_eq!(decoded.entries[2].name, "file.txt");
    }

    #[test]
    fn test_rpc_message_serialization() {
        let msg = RpcMessage::WriteRequest(WriteRequest {
            inode: 100,
            offset: 0,
            data: b"test data".to_vec(),
        });
        let bytes = serialize_message(&msg).unwrap();
        let decoded: RpcMessage = deserialize_message(&bytes).unwrap();
        match decoded {
            RpcMessage::WriteRequest(req) => {
                assert_eq!(req.inode, 100);
                assert_eq!(req.data, b"test data");
            }
            _ => panic!("Expected WriteRequest"),
        }
    }
}
