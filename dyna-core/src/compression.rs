//! Compression utilities for Dyna storage and transport.
//!
//! Provides gzip-based compression and decompression for both storage (local
//! repository files, S3 objects) and HTTP transport. Uses `flate2` which works
//! on both native and WASM targets.
//!
//! ## Storage format
//!
//! Compressed files use a gzip envelope. The helpers transparently detect
//! whether data is gzip-compressed (by checking the magic bytes `1f 8b`) and
//! decompress accordingly, so they are backwards-compatible with existing
//! uncompressed data.
//!
//! ## Transport format
//!
//! HTTP bodies are compressed with gzip and signalled via `Content-Encoding:
//! gzip` / `Accept-Encoding: gzip` headers.

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::{Read, Write};

/// The gzip magic bytes used to detect compressed data.
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// Default compression level (6 = balanced speed/ratio).
const DEFAULT_LEVEL: Compression = Compression::new(6);

/// Compress a byte slice with gzip, returning the compressed bytes.
pub fn compress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = GzEncoder::new(Vec::new(), DEFAULT_LEVEL);
    encoder.write_all(data)?;
    encoder.finish()
}

/// Compress a string with gzip, returning the compressed bytes.
pub fn compress_str(data: &str) -> Result<Vec<u8>, std::io::Error> {
    compress(data.as_bytes())
}

/// Decompress gzip-compressed bytes, returning the decompressed bytes.
pub fn decompress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = GzDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

/// Decompress gzip-compressed bytes to a UTF-8 string.
pub fn decompress_to_string(data: &[u8]) -> Result<String, std::io::Error> {
    decompress(data).and_then(|bytes| {
        String::from_utf8(bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    })
}

/// Check whether a byte slice starts with the gzip magic bytes.
pub fn is_gzip(data: &[u8]) -> bool {
    data.len() >= 2 && data[0] == GZIP_MAGIC[0] && data[1] == GZIP_MAGIC[1]
}

/// Transparently read data: if it is gzip-compressed, decompress it;
/// otherwise return it as-is. Returns the raw bytes.
pub fn read_transparent(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    is_gzip(data)
        .then(|| decompress(data))
        .unwrap_or_else(|| Ok(data.to_vec()))
}

/// Transparently read data to a string: if it is gzip-compressed, decompress
/// it; otherwise interpret as UTF-8 directly.
pub fn read_transparent_str(data: &[u8]) -> Result<String, std::io::Error> {
    is_gzip(data)
        .then(|| decompress_to_string(data))
        .unwrap_or_else(|| {
            String::from_utf8(data.to_vec())
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })
}

/// Serialize a value to JSON, then compress with gzip.
pub fn compress_json<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, std::io::Error> {
    serde_json::to_vec(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        .and_then(|json_bytes| compress(&json_bytes))
}

/// Serialize a value to pretty JSON, then compress with gzip.
pub fn compress_json_pretty<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, std::io::Error> {
    serde_json::to_vec_pretty(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        .and_then(|json_bytes| compress(&json_bytes))
}

/// Transparently decompress (if needed) and deserialize JSON.
pub fn decompress_json<T: serde::de::DeserializeOwned>(data: &[u8]) -> Result<T, std::io::Error> {
    read_transparent(data).and_then(|bytes| {
        serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_bytes() {
        let original = b"Hello, compressed world!";
        let compressed = compress(original).unwrap();
        assert!(is_gzip(&compressed));
        assert_ne!(compressed, original);
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn roundtrip_string() {
        let original = "{ \"key\": \"value\" }";
        let compressed = compress_str(original).unwrap();
        let decompressed = decompress_to_string(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn transparent_read_compressed() {
        let original = "test data";
        let compressed = compress_str(original).unwrap();
        let result = read_transparent_str(&compressed).unwrap();
        assert_eq!(result, original);
    }

    #[test]
    fn transparent_read_uncompressed() {
        let original = "plain text";
        let result = read_transparent_str(original.as_bytes()).unwrap();
        assert_eq!(result, original);
    }

    #[test]
    fn roundtrip_json() {
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct TestData {
            name: String,
            value: i32,
        }
        let original = TestData {
            name: "test".into(),
            value: 42,
        };
        let compressed = compress_json(&original).unwrap();
        assert!(is_gzip(&compressed));
        let decompressed: TestData = decompress_json(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn decompress_json_from_uncompressed() {
        let json = r#"{"name":"test","value":42}"#;
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct TestData {
            name: String,
            value: i32,
        }
        let result: TestData = decompress_json(json.as_bytes()).unwrap();
        assert_eq!(result.name, "test");
        assert_eq!(result.value, 42);
    }
}
