use anyhow::{Context, Result};
use std::path::PathBuf;

/// tries to get cache directory
pub fn get_cache_dir(base_path: &str) -> Result<PathBuf> {
    let base_path = std::path::Path::new(base_path);

    std::fs::create_dir_all(base_path)
        .with_context(|| format!("creating cache directory at {}", base_path.display()))?;

    let test_path = base_path.join(".test");
    std::fs::write(&test_path, "").with_context(|| {
        format!(
            "writing test file to verify cache dir at {}",
            test_path.display()
        )
    })?;
    Ok(base_path.to_owned())
}

pub struct DetNonDetSuffixes {
    pub det: &'static str,
    pub non_det: &'static str,
}

pub const PRECOMPILE_DIR_NAME: &str = "pc";

pub const DET_NON_DET_PRECOMPILED_SUFFIX: DetNonDetSuffixes = DetNonDetSuffixes {
    det: "det",
    non_det: "non-det",
};

pub fn path_in_zip_to_hash(path: &str) -> String {
    use sha3::digest::FixedOutput;
    use sha3::{Digest, Sha3_224};

    let mut hasher = Sha3_224::new();
    hasher.update(path.as_bytes());
    let digits = hasher.finalize_fixed();

    let digits = digits.as_slice();

    base32::encode(base32::Alphabet::Rfc4648 { padding: false }, digits)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    #[test]
    fn native_module_cache_rejects_group_or_world_writable_directory() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "genvm-untrusted-cache-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
        let path = root.to_string_lossy();

        let precompile_cache_accepted = get_cache_dir(&path).is_ok();
        let runtime_cache_accepted = crate::runners::cache::get_cache_dir(&path).is_ok();
        std::fs::remove_dir_all(root).unwrap();

        assert!(
            !precompile_cache_accepted && !runtime_cache_accepted,
            "the precompile and runtime cache entry points must reject group/world-writable directories before files from them reach unsafe Wasmtime deserialization (precompile accepted: {precompile_cache_accepted}, runtime accepted: {runtime_cache_accepted})"
        );
    }
}
