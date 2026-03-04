fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .out_dir(std::env::var("OUT_DIR").unwrap())
        .compile_protos(&["proto/replication.proto"], &["proto/"])?;
    Ok(())
}
