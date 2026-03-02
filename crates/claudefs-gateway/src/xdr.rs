//! XDR encoding/decoding for NFS protocol (RFC 4506)

use prost::bytes::{Bytes, BytesMut};

/// XDR encoder for NFS protocol messages.
pub struct XdrEncoder {
    buf: BytesMut,
}

impl XdrEncoder {
    /// Creates a new XdrEncoder.
    pub fn new() -> Self {
        Self {
            buf: BytesMut::new(),
        }
    }

    /// Encodes a 32-bit unsigned integer.
    pub fn encode_u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    /// Encodes a 32-bit signed integer.
    pub fn encode_i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    /// Encodes a 64-bit unsigned integer.
    pub fn encode_u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    /// Encodes a 64-bit signed integer.
    pub fn encode_i64(&mut self, v: i64) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    /// Encodes a boolean as a 32-bit integer (0 or 1).
    pub fn encode_bool(&mut self, v: bool) {
        self.encode_u32(if v { 1 } else { 0 });
    }

    /// Encodes fixed-length opaque data (padded to 4-byte boundary).
    pub fn encode_opaque_fixed(&mut self, data: &[u8]) {
        let padding = (4 - (data.len() % 4)) % 4;
        self.buf.extend_from_slice(data);
        self.buf.extend(vec![0u8; padding]);
    }

    /// Encodes variable-length opaque data (length prefix + padded data).
    pub fn encode_opaque_variable(&mut self, data: &[u8]) {
        self.encode_u32(data.len() as u32);
        self.encode_opaque_fixed(data);
    }

    /// Encodes a string as length-prefixed opaque data.
    pub fn encode_string(&mut self, s: &str) {
        self.encode_opaque_variable(s.as_bytes());
    }

    /// Consumes the encoder and returns the encoded bytes.
    pub fn finish(self) -> Bytes {
        self.buf.freeze()
    }
}

/// XDR decoder for NFS protocol messages.
pub struct XdrDecoder {
    buf: Bytes,
    pos: usize,
}

impl XdrDecoder {
    /// Creates a new XdrDecoder from encoded bytes.
    pub fn new(buf: Bytes) -> Self {
        Self { buf, pos: 0 }
    }

    fn ensure_available(&self, len: usize) -> super::error::Result<()> {
        if self.pos + len > self.buf.len() {
            return Err(super::error::GatewayError::XdrDecodeError {
                reason: "truncated data".to_string(),
            });
        }
        Ok(())
    }

    fn read_bytes(&mut self, len: usize) -> super::error::Result<Vec<u8>> {
        self.ensure_available(len)?;
        let result = self.buf[self.pos..self.pos + len].to_vec();
        self.pos += len;
        Ok(result)
    }

    pub fn decode_u32(&mut self) -> super::error::Result<u32> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub fn decode_i32(&mut self) -> super::error::Result<i32> {
        let bytes = self.read_bytes(4)?;
        Ok(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub fn decode_u64(&mut self) -> super::error::Result<u64> {
        let bytes = self.read_bytes(8)?;
        Ok(u64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    pub fn decode_i64(&mut self) -> super::error::Result<i64> {
        let bytes = self.read_bytes(8)?;
        Ok(i64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    pub fn decode_bool(&mut self) -> super::error::Result<bool> {
        let v = self.decode_u32()?;
        Ok(v != 0)
    }

    pub fn decode_opaque_fixed(&mut self, len: usize) -> super::error::Result<Vec<u8>> {
        let padding = (4 - (len % 4)) % 4;
        let total_len = len + padding;
        let result = self.read_bytes(total_len)?;
        Ok(result[..len].to_vec())
    }

    pub fn decode_opaque_variable(&mut self) -> super::error::Result<Vec<u8>> {
        let len = self.decode_u32()? as usize;
        self.decode_opaque_fixed(len)
    }

    pub fn decode_string(&mut self) -> super::error::Result<String> {
        let data = self.decode_opaque_variable()?;
        String::from_utf8(data).map_err(|e| super::error::GatewayError::XdrDecodeError {
            reason: format!("invalid UTF-8: {}", e),
        })
    }

    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    pub fn remaining_bytes(&self) -> Vec<u8> {
        self.buf[self.pos..].to_vec()
    }
}

impl Default for XdrEncoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_u32() {
        let mut enc = XdrEncoder::new();
        enc.encode_u32(0x12345678);
        let buf = enc.finish();
        assert_eq!(&buf[..], &[0x12, 0x34, 0x56, 0x78]);

        let mut dec = XdrDecoder::new(buf);
        assert_eq!(dec.decode_u32().unwrap(), 0x12345678);
    }

    #[test]
    fn test_encode_decode_i32() {
        let mut enc = XdrEncoder::new();
        enc.encode_i32(-12345);
        let buf = enc.finish();

        let mut dec = XdrDecoder::new(buf);
        assert_eq!(dec.decode_i32().unwrap(), -12345);
    }

    #[test]
    fn test_encode_decode_u64() {
        let mut enc = XdrEncoder::new();
        enc.encode_u64(0x123456789ABCDEF0);
        let buf = enc.finish();
        assert_eq!(buf.len(), 8);

        let mut dec = XdrDecoder::new(buf);
        assert_eq!(dec.decode_u64().unwrap(), 0x123456789ABCDEF0);
    }

    #[test]
    fn test_encode_decode_i64() {
        let mut enc = XdrEncoder::new();
        enc.encode_i64(-12345678901234567);
        let buf = enc.finish();

        let mut dec = XdrDecoder::new(buf);
        assert_eq!(dec.decode_i64().unwrap(), -12345678901234567);
    }

    #[test]
    fn test_encode_decode_bool() {
        let mut enc = XdrEncoder::new();
        enc.encode_bool(true);
        enc.encode_bool(false);
        let buf = enc.finish();

        let mut dec = XdrDecoder::new(buf);
        assert!(dec.decode_bool().unwrap());
        assert!(!dec.decode_bool().unwrap());
    }

    #[test]
    fn test_encode_decode_opaque_fixed() {
        let mut enc = XdrEncoder::new();
        enc.encode_opaque_fixed(b"abc");
        let buf = enc.finish();
        assert_eq!(&buf[..], &[b'a', b'b', b'c', 0]);

        let mut dec = XdrDecoder::new(buf);
        assert_eq!(dec.decode_opaque_fixed(3).unwrap(), b"abc");
    }

    #[test]
    fn test_encode_decode_opaque_fixed_aligned() {
        let mut enc = XdrEncoder::new();
        enc.encode_opaque_fixed(b"abcd");
        let buf = enc.finish();
        assert_eq!(&buf[..], &[b'a', b'b', b'c', b'd']);

        let mut dec = XdrDecoder::new(buf);
        assert_eq!(dec.decode_opaque_fixed(4).unwrap(), b"abcd");
    }

    #[test]
    fn test_encode_decode_opaque_variable() {
        let mut enc = XdrEncoder::new();
        enc.encode_opaque_variable(b"hello");
        let buf = enc.finish();

        let mut dec = XdrDecoder::new(buf);
        assert_eq!(dec.decode_opaque_variable().unwrap(), b"hello");
    }

    #[test]
    fn test_encode_decode_string() {
        let mut enc = XdrEncoder::new();
        enc.encode_string("test string");
        let buf = enc.finish();

        let mut dec = XdrDecoder::new(buf);
        assert_eq!(dec.decode_string().unwrap(), "test string");
    }

    #[test]
    fn test_encode_decode_empty_string() {
        let mut enc = XdrEncoder::new();
        enc.encode_string("");
        let buf = enc.finish();

        let mut dec = XdrDecoder::new(buf);
        assert_eq!(dec.decode_string().unwrap(), "");
    }

    #[test]
    fn test_encode_decode_empty_opaque() {
        let mut enc = XdrEncoder::new();
        enc.encode_opaque_variable(b"");
        let buf = enc.finish();

        let mut dec = XdrDecoder::new(buf);
        assert_eq!(dec.decode_opaque_variable().unwrap(), b"");
    }

    #[test]
    fn test_padding_with_various_lengths() {
        for len in 0..10 {
            let data = vec![0xFF; len];
            let mut enc = XdrEncoder::new();
            enc.encode_opaque_fixed(&data);
            let buf = enc.finish();

            let mut dec = XdrDecoder::new(buf);
            let result = dec.decode_opaque_fixed(len).unwrap();
            assert_eq!(result, data);
        }
    }

    #[test]
    fn test_error_truncated_data() {
        let buf = Bytes::from_static(&[0x12, 0x34]);
        let mut dec = XdrDecoder::new(buf);
        let result = dec.decode_u32();
        assert!(result.is_err());
    }

    #[test]
    fn test_roundtrip_multiple_values() {
        let mut enc = XdrEncoder::new();
        enc.encode_u32(42);
        enc.encode_string("hello");
        enc.encode_u64(100);
        enc.encode_bool(true);
        let buf = enc.finish();

        let mut dec = XdrDecoder::new(buf);
        assert_eq!(dec.decode_u32().unwrap(), 42);
        assert_eq!(dec.decode_string().unwrap(), "hello");
        assert_eq!(dec.decode_u64().unwrap(), 100);
        assert!(dec.decode_bool().unwrap());
        assert_eq!(dec.remaining(), 0);
    }

    #[test]
    fn test_remaining() {
        let mut enc = XdrEncoder::new();
        enc.encode_u32(42);
        let buf = enc.finish();

        let dec = XdrDecoder::new(buf);
        assert_eq!(dec.remaining(), 4);
    }

    #[test]
    fn test_long_string() {
        let s = "a".repeat(1000);
        let mut enc = XdrEncoder::new();
        enc.encode_string(&s);
        let buf = enc.finish();

        let mut dec = XdrDecoder::new(buf);
        assert_eq!(dec.decode_string().unwrap(), s);
    }
}
