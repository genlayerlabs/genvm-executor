use std::collections::BTreeMap;

use anyhow::Context as _;
use genvm::{caching, config, runners, wasmtime_to_anyhow};

use genvm_common::*;

#[derive(clap::Args, Debug)]
pub struct Args {
    #[arg(
        long,
        default_value_t = false,
        help = "instead of precompiling show information"
    )]
    info: bool,
}

fn compile_single_file_single_mode(
    result_path: &std::path::Path,
    engine: &wasmtime::Engine,
    wasm_data: &[u8],
    engine_type: &str,
    runner_path: &std::path::Path,
    path_in_runner: &str,
) -> anyhow::Result<()> {
    let time_start = std::time::Instant::now();
    let precompiled = engine
        .precompile_module(wasm_data)
        .map_err(wasmtime_to_anyhow)
        .with_context(|| "precompiling")?;

    log_info!(engine = engine_type, runner:? = runner_path, runner_path:? = path_in_runner, duration:? = time_start.elapsed();  "wasm compilation done");

    std::fs::create_dir_all(
        result_path
            .parent()
            .expect("precompiled wasm path has no parent directory"),
    )
    .with_context(|| format!("creating directory for {result_path:?}"))?;

    let sz = precompiled.len();

    std::fs::write(result_path, precompiled)
        .with_context(|| format!("writing to {result_path:?}"))?;

    log_info!("size" = sz, result:? = result_path, engine = engine_type, runner:? = runner_path, runner_path:? = path_in_runner, duration:? = time_start.elapsed(); "wasm writing done");

    Ok(())
}

fn compile_single_file(
    precompile_dir: &std::path::Path,
    engines: &genvm::rt::DetNondet<wasmtime::Engine>,
    runners_dir: &std::path::Path,
    zip_path: &std::path::Path,
) -> anyhow::Result<()> {
    let base_path = zip_path
        .strip_prefix(runners_dir)
        .with_context(|| format!("stripping {runners_dir:?} from {runners_dir:?}"))?;

    let base_path = if let Some(no_stem) = base_path.file_stem() {
        base_path.with_file_name(no_stem)
    } else {
        base_path.to_owned()
    };

    let mut result_dir_path = precompile_dir.to_owned();
    result_dir_path.push(base_path);

    let data = util::mmap_file(zip_path).with_context(|| format!("memory mapping {zip_path:?}"))?;

    let arch = genvm::runners::Archive::from_ustar(bytes::Bytes::copy_from_slice(data.as_ref()))
        .with_context(|| format!("parsing ustar archive {zip_path:?}"))?;

    for (entry_name, contents) in arch
        .data
        .iter()
        .filter(|(k, _v)| k.ends_with(".wasm") || k.ends_with(".so"))
    {
        if !wasmparser::Parser::is_core_wasm(contents.as_ref()) {
            continue;
        }

        let entry_name_hash = caching::path_in_zip_to_hash(entry_name);
        let result_file = result_dir_path.join(entry_name_hash);

        compile_single_file_single_mode(
            result_file
                .with_extension(caching::DET_NON_DET_PRECOMPILED_SUFFIX.det)
                .as_path(),
            &engines.det,
            contents.as_ref(),
            caching::DET_NON_DET_PRECOMPILED_SUFFIX.det,
            zip_path,
            entry_name,
        )
        .with_context(|| format!("processing det {entry_name}"))?;

        compile_single_file_single_mode(
            result_file
                .with_extension(caching::DET_NON_DET_PRECOMPILED_SUFFIX.non_det)
                .as_path(),
            &engines.non_det,
            contents.as_ref(),
            caching::DET_NON_DET_PRECOMPILED_SUFFIX.non_det,
            zip_path,
            entry_name,
        )
        .with_context(|| format!("processing non-det {entry_name}"))?;
    }
    Ok(())
}

pub fn handle(args: Args, config: config::Config) -> anyhow::Result<()> {
    if args.info {
        log_info!(version = genvm_common::version::CURRENT.clone(); "current version");

        let cache_dir = caching::get_cache_dir(&config.cache_dir)?;
        let mut precompile_dir = cache_dir.clone();
        precompile_dir.push(caching::PRECOMPILE_DIR_NAME);

        let registry_dir = std::path::Path::new(&config.registry_dir);

        log_info!(cache_dir:? = cache_dir, precompile_dir:? = precompile_dir, registry_dir:? = registry_dir; "information");
        return Ok(());
    }

    run(&config)
}

/// Precompiles every runner listed in `all.json` into the on-disk cache. Shared
/// by the `precompile` subcommand and `check --precompile`.
pub fn run(config: &config::Config) -> anyhow::Result<()> {
    log_info!(version = genvm_common::version::CURRENT.clone(); "current version");

    let cache_dir = caching::get_cache_dir(&config.cache_dir)?;
    let mut precompile_dir = cache_dir.clone();
    precompile_dir.push(caching::PRECOMPILE_DIR_NAME);

    let registry_dir = std::path::Path::new(&config.registry_dir);

    log_info!(cache_dir:? = cache_dir, precompile_dir:? = precompile_dir, registry_dir:? = registry_dir; "information");

    let engines = genvm::rt::supervisor::create_engines(|conf| {
        conf.cranelift_opt_level(wasmtime::OptLevel::Speed);
        Ok(())
    })?;

    let all_json_path = registry_dir.join("all.json");
    let all_json = std::fs::read_to_string(&all_json_path)
        .with_context(|| format!("reading {all_json_path:?}"))?;
    let all: BTreeMap<String, Vec<String>> =
        serde_json::from_str(&all_json).with_context(|| format!("parsing {all_json_path:?}"))?;

    let runners_dir = std::path::Path::new(&config.runners_dir);

    for (runner_id, hashes) in all {
        for hash in hashes {
            let mut runner_path = runners_dir.to_owned();
            runners::append_runner_subpath(&runner_id, &hash, &mut runner_path);
            runner_path.set_extension("tar");

            compile_single_file(&precompile_dir, &engines, runners_dir, &runner_path)
                .with_context(|| format!("processing {runner_path:?}"))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root(label: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "genvm-precompile-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn write_ustar(path: &std::path::Path, entry_name: &str, contents: &[u8]) {
        let mut header = [0_u8; 512];
        header[..entry_name.len()].copy_from_slice(entry_name.as_bytes());
        header[100..108].copy_from_slice(b"0000644\0");
        header[108..116].copy_from_slice(b"0000000\0");
        header[116..124].copy_from_slice(b"0000000\0");
        header[124..136].copy_from_slice(format!("{:011o}\0", contents.len()).as_bytes());
        header[136..148].copy_from_slice(b"00000000000\0");
        header[148..156].fill(b' ');
        header[156] = b'0';
        header[257..265].copy_from_slice(b"ustar\0\x30\x30");
        let checksum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
        header[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());

        let padded_contents = contents.len().next_multiple_of(512);
        let mut archive = Vec::with_capacity(512 + padded_contents + 1024);
        archive.extend_from_slice(&header);
        archive.extend_from_slice(contents);
        archive.resize(512 + padded_contents + 1024, 0);

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, archive).unwrap();
    }

    #[test]
    fn failed_recompile_does_not_leave_previous_module_loadable() {
        let root = test_root("failed-recompile");
        let result_path = root.join("module.det");
        let engines = genvm::rt::supervisor::create_engines(|_| Ok(())).unwrap();
        let valid_wasm = b"\0asm\x01\0\0\0";

        compile_single_file_single_mode(
            &result_path,
            &engines.det,
            valid_wasm,
            "det",
            std::path::Path::new("runner.tar"),
            "module.wasm",
        )
        .unwrap();

        // This retains the Wasm magic/version but ends in an incomplete section,
        // so cold compilation rejects it after recognizing it as core Wasm.
        let invalid_wasm = b"\0asm\x01\0\0\0\x01";
        let recompile = compile_single_file_single_mode(
            &result_path,
            &engines.det,
            invalid_wasm,
            "det",
            std::path::Path::new("runner.tar"),
            "module.wasm",
        );
        let malformed_replacement_rejected = recompile.is_err();

        // SAFETY: this file is the exact, unmodified output produced above by
        // the same Wasmtime engine; the failed replacement never writes it.
        let stale_module_still_loads =
            unsafe { wasmtime::Module::deserialize_file(&engines.det, &result_path) }.is_ok();
        std::fs::remove_dir_all(root).unwrap();

        assert!(
            malformed_replacement_rejected,
            "the malformed replacement must fail cold compilation"
        );
        assert!(
            !stale_module_still_loads,
            "a failed recompile must invalidate the previous module at the same cache key; otherwise cached loading accepts code that cold compilation rejects"
        );
    }

    #[test]
    fn successful_recompile_does_not_keep_module_for_non_wasm_source() {
        let root = test_root("non-wasm-replacement");
        let runners_dir = root.join("runners");
        let precompile_dir = root.join("pc");
        let runner_path = runners_dir.join("runner/hash.tar");
        let entry_name = "module.wasm";
        let engines = genvm::rt::supervisor::create_engines(|_| Ok(())).unwrap();

        write_ustar(&runner_path, entry_name, b"\0asm\x01\0\0\0");
        compile_single_file(&precompile_dir, &engines, &runners_dir, &runner_path).unwrap();

        let module_key = caching::path_in_zip_to_hash(entry_name);
        let cache_path = precompile_dir.join("runner/hash").join(module_key);
        let det_path = cache_path.with_extension(caching::DET_NON_DET_PRECOMPILED_SUFFIX.det);
        let non_det_path =
            cache_path.with_extension(caching::DET_NON_DET_PRECOMPILED_SUFFIX.non_det);
        assert!(det_path.exists());
        assert!(non_det_path.exists());

        write_ustar(&runner_path, entry_name, b"this is no longer wasm");
        compile_single_file(&precompile_dir, &engines, &runners_dir, &runner_path).unwrap();

        let stale_pair_still_exists = det_path.exists() || non_det_path.exists();
        std::fs::remove_dir_all(root).unwrap();

        assert!(
            !stale_pair_still_exists,
            "a successful precompile pass must remove cached modules for a source that is no longer Wasm; otherwise cached execution accepts bytes that cold execution rejects"
        );
    }

    #[cfg(unix)]
    #[test]
    fn recompile_replaces_precompiled_file_without_mutating_live_inode() {
        use std::os::unix::fs::MetadataExt as _;

        let root = test_root("atomic-replacement");
        let result_path = root.join("module.det");
        let engines = genvm::rt::supervisor::create_engines(|_| Ok(())).unwrap();
        let empty_module = b"\0asm\x01\0\0\0";
        let module_returning_one = b"\0asm\x01\0\0\0\x01\x05\x01\x60\0\x01\x7f\x03\x02\x01\0\x07\x09\x01\x05value\0\0\x0a\x06\x01\x04\0\x41\x01\x0b";

        compile_single_file_single_mode(
            &result_path,
            &engines.det,
            empty_module,
            "det",
            std::path::Path::new("runner.tar"),
            "module.wasm",
        )
        .unwrap();
        let first_inode = std::fs::metadata(&result_path).unwrap().ino();
        let first_artifact = std::fs::read(&result_path).unwrap();

        compile_single_file_single_mode(
            &result_path,
            &engines.det,
            module_returning_one,
            "det",
            std::path::Path::new("runner.tar"),
            "module.wasm",
        )
        .unwrap();
        let second_inode = std::fs::metadata(&result_path).unwrap().ino();
        let second_artifact = std::fs::read(&result_path).unwrap();
        std::fs::remove_dir_all(root).unwrap();

        assert_ne!(
            first_artifact, second_artifact,
            "test modules must produce different artifacts"
        );
        assert_ne!(
            first_inode, second_inode,
            "recompilation must atomically replace the cache file instead of modifying the inode that a live Wasmtime module may still have memory-mapped"
        );
    }
}
