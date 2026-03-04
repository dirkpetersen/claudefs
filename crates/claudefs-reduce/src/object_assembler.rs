use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlobKey {
    pub prefix: String,
    pub object_id: u64,
}

impl BlobKey {
    pub fn new(prefix: impl Into<String>, object_id: u64) -> Self {
        Self {
            prefix: prefix.into(),
            object_id,
        }
    }

    pub fn s3_key(&self) -> String {
        format!("{}/{:016x}", self.prefix, self.object_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkLocation {
    pub chunk_hash: [u8; 32],
    pub blob_key: BlobKey,
    pub offset: u32,
    pub len: u32,
}

#[derive(Debug, Clone)]
pub struct ObjectAssemblerConfig {
    pub target_blob_bytes: u64,
    pub key_prefix: String,
}

impl Default for ObjectAssemblerConfig {
    fn default() -> Self {
        Self {
            target_blob_bytes: 64 * 1024 * 1024,
            key_prefix: "claudefs".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ObjectAssemblerStats {
    pub blobs_completed: u64,
    pub total_chunks_packed: u64,
    pub total_bytes_packed: u64,
    pub current_blob_chunks: u64,
    pub current_blob_bytes: u64,
}

pub struct ObjectAssembler {
    config: ObjectAssemblerConfig,
    current_blob_id: u64,
    current_data: Vec<u8>,
    current_chunk_index: Vec<ChunkLocation>,
    stats: ObjectAssemblerStats,
}

#[derive(Debug)]
pub struct CompletedBlob {
    pub key: BlobKey,
    pub data: Vec<u8>,
    pub chunk_locations: Vec<ChunkLocation>,
}

impl ObjectAssembler {
    pub fn new(config: ObjectAssemblerConfig) -> Self {
        Self {
            config,
            current_blob_id: 0,
            current_data: Vec::new(),
            current_chunk_index: Vec::new(),
            stats: ObjectAssemblerStats::default(),
        }
    }

    pub fn pack(&mut self, chunk_hash: [u8; 32], chunk_data: &[u8]) -> Option<CompletedBlob> {
        let offset = self.current_data.len() as u32;
        let len = chunk_data.len() as u32;

        let blob_key = BlobKey::new(self.config.key_prefix.clone(), self.current_blob_id);
        let location = ChunkLocation {
            chunk_hash,
            blob_key,
            offset,
            len,
        };

        self.current_data.extend_from_slice(chunk_data);
        self.current_chunk_index.push(location);
        self.stats.total_chunks_packed += 1;
        self.stats.total_bytes_packed += chunk_data.len() as u64;
        self.stats.current_blob_chunks += 1;
        self.stats.current_blob_bytes = self.current_data.len() as u64;

        if self.current_data.len() as u64 >= self.config.target_blob_bytes {
            return Some(self.seal_current());
        }
        None
    }

    pub fn flush(&mut self) -> Option<CompletedBlob> {
        if self.current_data.is_empty() {
            return None;
        }
        Some(self.seal_current())
    }

    fn seal_current(&mut self) -> CompletedBlob {
        let key = BlobKey::new(self.config.key_prefix.clone(), self.current_blob_id);
        let data = std::mem::take(&mut self.current_data);
        let locations = std::mem::take(&mut self.current_chunk_index);
        self.current_blob_id += 1;
        self.stats.blobs_completed += 1;
        self.stats.current_blob_chunks = 0;
        self.stats.current_blob_bytes = 0;
        CompletedBlob {
            key,
            data,
            chunk_locations: locations,
        }
    }

    pub fn current_blob_size(&self) -> usize {
        self.current_data.len()
    }
    pub fn current_blob_chunks(&self) -> usize {
        self.current_chunk_index.len()
    }
    pub fn stats(&self) -> &ObjectAssemblerStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_hash(v: u8) -> [u8; 32] {
        let mut h = [0u8; 32];
        h[0] = v;
        h
    }

    #[test]
    fn object_assembler_config_default() {
        let config = ObjectAssemblerConfig::default();
        assert_eq!(config.target_blob_bytes, 64 * 1024 * 1024);
        assert_eq!(config.key_prefix, "claudefs");
    }

    #[test]
    fn blob_key_s3_key_format() {
        let key = BlobKey::new("prefix", 0x123);
        assert_eq!(key.s3_key(), "prefix/0000000000000123");
    }

    #[test]
    fn blob_key_equality() {
        let k1 = BlobKey::new("a", 1);
        let k2 = BlobKey::new("a", 1);
        let k3 = BlobKey::new("a", 2);
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }

    #[test]
    fn new_assembler_empty() {
        let asm = ObjectAssembler::new(ObjectAssemblerConfig::default());
        assert_eq!(asm.current_blob_size(), 0);
        assert_eq!(asm.current_blob_chunks(), 0);
    }

    #[test]
    fn pack_single_chunk_no_seal() {
        let mut asm = ObjectAssembler::new(ObjectAssemblerConfig::default());
        let result = asm.pack(make_hash(1), b"test data");
        assert!(result.is_none());
    }

    #[test]
    fn pack_returns_some_at_target() {
        let mut asm = ObjectAssembler::new(ObjectAssemblerConfig {
            target_blob_bytes: 10,
            key_prefix: "test".to_string(),
        });
        let result = asm.pack(make_hash(1), b"12345678901");
        assert!(result.is_some());
    }

    #[test]
    fn pack_increments_total_chunks() {
        let mut asm = ObjectAssembler::new(ObjectAssemblerConfig::default());
        asm.pack(make_hash(1), b"x");
        asm.pack(make_hash(2), b"x");
        assert_eq!(asm.stats().total_chunks_packed, 2);
    }

    #[test]
    fn pack_increments_total_bytes() {
        let mut asm = ObjectAssembler::new(ObjectAssemblerConfig::default());
        asm.pack(make_hash(1), b"hello");
        asm.pack(make_hash(2), b"world");
        assert_eq!(asm.stats().total_bytes_packed, 10);
    }

    #[test]
    fn pack_updates_current_blob_size() {
        let mut asm = ObjectAssembler::new(ObjectAssemblerConfig::default());
        asm.pack(make_hash(1), b"hello");
        assert_eq!(asm.current_blob_size(), 5);
    }

    #[test]
    fn pack_updates_current_chunks() {
        let mut asm = ObjectAssembler::new(ObjectAssemblerConfig::default());
        asm.pack(make_hash(1), b"x");
        asm.pack(make_hash(2), b"y");
        assert_eq!(asm.current_blob_chunks(), 2);
    }

    #[test]
    fn flush_returns_none_when_empty() {
        let mut asm = ObjectAssembler::new(ObjectAssemblerConfig::default());
        let result = asm.flush();
        assert!(result.is_none());
    }

    #[test]
    fn flush_returns_blob_when_nonempty() {
        let mut asm = ObjectAssembler::new(ObjectAssemblerConfig::default());
        asm.pack(make_hash(1), b"test");
        let result = asm.flush();
        assert!(result.is_some());
    }

    #[test]
    fn flush_clears_current() {
        let mut asm = ObjectAssembler::new(ObjectAssemblerConfig::default());
        asm.pack(make_hash(1), b"test");
        asm.flush();
        assert_eq!(asm.current_blob_size(), 0);
    }

    #[test]
    fn completed_blob_has_data() {
        let mut asm = ObjectAssembler::new(ObjectAssemblerConfig::default());
        asm.pack(make_hash(1), b"hello world");
        let blob = asm.flush().unwrap();
        assert!(!blob.data.is_empty());
    }

    #[test]
    fn completed_blob_chunk_locations() {
        let mut asm = ObjectAssembler::new(ObjectAssemblerConfig::default());
        asm.pack(make_hash(1), b"hello");
        let blob = asm.flush().unwrap();
        assert_eq!(blob.chunk_locations.len(), 1);
    }

    #[test]
    fn chunk_location_offset_correct() {
        let mut asm = ObjectAssembler::new(ObjectAssemblerConfig::default());
        asm.pack(make_hash(1), b"hello");
        let blob = asm.flush().unwrap();
        assert_eq!(blob.chunk_locations[0].offset, 0);
    }

    #[test]
    fn chunk_location_len_correct() {
        let mut asm = ObjectAssembler::new(ObjectAssemblerConfig::default());
        asm.pack(make_hash(1), b"hello");
        let blob = asm.flush().unwrap();
        assert_eq!(blob.chunk_locations[0].len, 5);
    }

    #[test]
    fn multiple_packs_sequential_offsets() {
        let mut asm = ObjectAssembler::new(ObjectAssemblerConfig::default());
        asm.pack(make_hash(1), b"aa");
        asm.pack(make_hash(2), b"bb");
        let blob = asm.flush().unwrap();
        assert_eq!(blob.chunk_locations[0].offset, 0);
        assert_eq!(blob.chunk_locations[1].offset, 2);
    }

    #[test]
    fn blob_id_increments_after_seal() {
        let mut asm = ObjectAssembler::new(ObjectAssemblerConfig::default());
        asm.pack(make_hash(1), b"test");
        asm.flush();
        asm.pack(make_hash(2), b"test2");
        let blob = asm.flush().unwrap();
        assert_eq!(blob.key.object_id, 1);
    }

    #[test]
    fn stats_blobs_completed_after_flush() {
        let mut asm = ObjectAssembler::new(ObjectAssemblerConfig::default());
        asm.pack(make_hash(1), b"x");
        asm.flush();
        assert_eq!(asm.stats().blobs_completed, 1);
    }

    #[test]
    fn stats_current_blob_reset_after_seal() {
        let mut asm = ObjectAssembler::new(ObjectAssemblerConfig::default());
        asm.pack(make_hash(1), b"test");
        asm.flush();
        assert_eq!(asm.stats().current_blob_chunks, 0);
        assert_eq!(asm.stats().current_blob_bytes, 0);
    }

    #[test]
    fn pack_large_chunk_triggers_seal() {
        let mut asm = ObjectAssembler::new(ObjectAssemblerConfig {
            target_blob_bytes: 5,
            key_prefix: "t".to_string(),
        });
        let result = asm.pack(make_hash(1), b"123456");
        assert!(result.is_some());
    }
}
