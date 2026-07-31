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

/// Writes `data` to a uniquely named sibling of `path`, then renames it onto
/// `path`.
///
/// The reader `deserialize_file`s whatever sits at the final path without
/// validating it, so a crash mid-write must not leave a truncated module there.
/// The temp name mixes pid, an in-process counter and the wall clock, so
/// concurrent writers never pick the same one; the fsync keeps the rename from
/// exposing a name whose contents are still only in page cache.
fn write_atomically(path: &std::path::Path, data: &[u8]) -> anyhow::Result<()> {
    use std::io::Write as _;

    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since_epoch| since_epoch.as_nanos());
    let mut tmp_name = path
        .file_name()
        .expect("precompiled wasm path has no file name")
        .to_owned();
    tmp_name.push(format!(
        ".tmp-{}-{}-{stamp}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    ));
    let tmp_path = path.with_file_name(tmp_name);

    let write_tmp = || -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp_path)?;
        file.write_all(data)?;
        file.sync_all()?;
        std::fs::rename(&tmp_path, path)
    };

    if let Err(e) = write_tmp() {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e).with_context(|| format!("writing {tmp_path:?} and renaming it to {path:?}"));
    }

    Ok(())
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

    write_atomically(result_path, &precompiled)?;

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
