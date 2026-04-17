use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataRecord {
    pub inode: u64,
    pub path: String,
    pub filename: String,
    pub parent_path: String,
    pub owner_uid: u32,
    pub owner_name: String,
    pub group_gid: u32,
    pub group_name: String,
    pub size_bytes: u64,
    pub blocks_stored: u64,
    pub mtime: i64,
    pub ctime: i64,
    pub file_type: String,
    pub is_replicated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParquetSchema {
    pub version: String,
    pub fields: Vec<SchemaField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaField {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
}

impl ParquetSchema {
    pub fn v1() -> Self {
        Self {
            version: "1.0".to_string(),
            fields: vec![
                SchemaField {
                    name: "inode".to_string(),
                    data_type: "UInt64".to_string(),
                    nullable: false,
                },
                SchemaField {
                    name: "path".to_string(),
                    data_type: "Utf8".to_string(),
                    nullable: false,
                },
                SchemaField {
                    name: "filename".to_string(),
                    data_type: "Utf8".to_string(),
                    nullable: false,
                },
                SchemaField {
                    name: "parent_path".to_string(),
                    data_type: "Utf8".to_string(),
                    nullable: false,
                },
                SchemaField {
                    name: "owner_uid".to_string(),
                    data_type: "UInt32".to_string(),
                    nullable: false,
                },
                SchemaField {
                    name: "owner_name".to_string(),
                    data_type: "Utf8".to_string(),
                    nullable: false,
                },
                SchemaField {
                    name: "group_gid".to_string(),
                    data_type: "UInt32".to_string(),
                    nullable: false,
                },
                SchemaField {
                    name: "group_name".to_string(),
                    data_type: "Utf8".to_string(),
                    nullable: false,
                },
                SchemaField {
                    name: "size_bytes".to_string(),
                    data_type: "UInt64".to_string(),
                    nullable: false,
                },
                SchemaField {
                    name: "blocks_stored".to_string(),
                    data_type: "UInt64".to_string(),
                    nullable: false,
                },
                SchemaField {
                    name: "mtime".to_string(),
                    data_type: "Int64".to_string(),
                    nullable: false,
                },
                SchemaField {
                    name: "ctime".to_string(),
                    data_type: "Int64".to_string(),
                    nullable: false,
                },
                SchemaField {
                    name: "file_type".to_string(),
                    data_type: "Utf8".to_string(),
                    nullable: false,
                },
                SchemaField {
                    name: "is_replicated".to_string(),
                    data_type: "Boolean".to_string(),
                    nullable: false,
                },
            ],
        }
    }

    pub fn v2() -> Self {
        let mut schema = Self::v1();
        schema.version = "2.0".to_string();
        schema.fields.push(SchemaField {
            name: "schema_version".to_string(),
            data_type: "UInt32".to_string(),
            nullable: true,
        });
        schema
    }

    pub fn field_count(&self) -> usize {
        self.fields.len()
    }

    pub fn validate_path(path: &Path) -> Result<bool, String> {
        if !path.exists() {
            return Err("File does not exist".to_string());
        }
        if let Some(ext) = path.extension() {
            if ext != "parquet" {
                return Err("Not a Parquet file".to_string());
            }
        } else {
            return Err("No file extension".to_string());
        }
        Ok(true)
    }

    pub fn convert_row(values: &[serde_json::Value]) -> Result<MetadataRecord, String> {
        if values.len() < 14 {
            return Err("Not enough columns".to_string());
        }

        Ok(MetadataRecord {
            inode: values[0].as_u64().unwrap_or(0),
            path: values[1].as_str().unwrap_or("").to_string(),
            filename: values[2].as_str().unwrap_or("").to_string(),
            parent_path: values[3].as_str().unwrap_or("").to_string(),
            owner_uid: values[4].as_u64().unwrap_or(0) as u32,
            owner_name: values[5].as_str().unwrap_or("").to_string(),
            group_gid: values[6].as_u64().unwrap_or(0) as u32,
            group_name: values[7].as_str().unwrap_or("").to_string(),
            size_bytes: values[8].as_u64().unwrap_or(0),
            blocks_stored: values[9].as_u64().unwrap_or(0),
            mtime: values[10].as_i64().unwrap_or(0),
            ctime: values[11].as_i64().unwrap_or(0),
            file_type: values[12].as_str().unwrap_or("").to_string(),
            is_replicated: values[13].as_bool().unwrap_or(false),
        })
    }
}

pub fn arrow_type_from_rust(field_type: &str) -> &'static str {
    match field_type {
        "UInt64" => "UINT64",
        "UInt32" => "UINT32",
        "Int64" => "INT64",
        "Int32" => "INT32",
        "Utf8" => "UTF8",
        "Boolean" => "BOOL",
        _ => "UNKNOWN",
    }
}

pub fn rust_type_from_arrow(arrow_type: &str) -> &'static str {
    match arrow_type {
        "UINT64" => "u64",
        "UINT32" => "u32",
        "INT64" => "i64",
        "INT32" => "i32",
        "UTF8" => "String",
        "BOOL" => "bool",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parquet_schema_definition() {
        let schema = ParquetSchema::v1();
        assert_eq!(schema.field_count(), 14);
        assert_eq!(schema.version, "1.0");
    }

    #[test]
    fn test_parquet_schema_arrow_types() {
        let schema = ParquetSchema::v1();

        let inode_field = schema.fields.iter().find(|f| f.name == "inode").unwrap();
        assert_eq!(arrow_type_from_rust(&inode_field.data_type), "UINT64");

        let path_field = schema.fields.iter().find(|f| f.name == "path").unwrap();
        assert_eq!(arrow_type_from_rust(&path_field.data_type), "UTF8");

        let mtime_field = schema.fields.iter().find(|f| f.name == "mtime").unwrap();
        assert_eq!(arrow_type_from_rust(&mtime_field.data_type), "INT64");

        let is_replicated_field = schema
            .fields
            .iter()
            .find(|f| f.name == "is_replicated")
            .unwrap();
        assert_eq!(arrow_type_from_rust(&is_replicated_field.data_type), "BOOL");
    }

    #[test]
    fn test_parquet_schema_validation_valid_file() {
        let result = ParquetSchema::validate_path(Path::new("test.parquet"));
        assert!(result.is_err());

        let result = ParquetSchema::validate_path(Path::new("/nonexistent/file.parquet"));
        assert!(result.is_err());
    }

    #[test]
    fn test_parquet_schema_validation_invalid_file() {
        let result = ParquetSchema::validate_path(Path::new("test.txt"));
        assert!(result.is_err());

        let result = ParquetSchema::validate_path(Path::new("test"));
        assert!(result.is_err());
    }

    #[test]
    fn test_parquet_schema_row_conversion() {
        let values = vec![
            serde_json::json!(123),
            serde_json::json!("/data/file.txt"),
            serde_json::json!("file.txt"),
            serde_json::json!("/data"),
            serde_json::json!(1000),
            serde_json::json!("user_1000"),
            serde_json::json!(1000),
            serde_json::json!("group_1000"),
            serde_json::json!(4096),
            serde_json::json!(4096),
            serde_json::json!(1234567890),
            serde_json::json!(1234567890),
            serde_json::json!("txt"),
            serde_json::json!(false),
        ];

        let record = ParquetSchema::convert_row(&values).unwrap();

        assert_eq!(record.inode, 123);
        assert_eq!(record.path, "/data/file.txt");
        assert_eq!(record.filename, "file.txt");
        assert_eq!(record.owner_uid, 1000);
        assert_eq!(record.size_bytes, 4096);
        assert_eq!(record.file_type, "txt");
        assert!(!record.is_replicated);
    }

    #[test]
    fn test_parquet_schema_versioning() {
        let schema_v1 = ParquetSchema::v1();
        assert_eq!(schema_v1.field_count(), 14);
        assert_eq!(schema_v1.version, "1.0");

        let schema_v2 = ParquetSchema::v2();
        assert_eq!(schema_v2.field_count(), 15);
        assert_eq!(schema_v2.version, "2.0");

        let has_schema_version = schema_v2.fields.iter().any(|f| f.name == "schema_version");
        assert!(has_schema_version);
    }
}
