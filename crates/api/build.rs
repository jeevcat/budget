fn main() {
    // Capture SOURCE_DATE_EPOCH at build time for reproducible Last-Modified headers.
    // Nix sets this based on the source tree's latest commit timestamp.
    // Falls back to current time if not set (local development).
    let epoch = std::env::var("SOURCE_DATE_EPOCH").unwrap_or_else(|_| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or_else(|_| "0".to_string(), |d| d.as_secs().to_string())
    });
    println!("cargo::rerun-if-env-changed=SOURCE_DATE_EPOCH");
    println!("cargo::rustc-env=SOURCE_DATE_EPOCH={epoch}");
}
