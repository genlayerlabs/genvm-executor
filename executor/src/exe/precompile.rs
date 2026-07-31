use genvm::{caching, config};

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

/// Disabled until precompiled native artifacts are immutable and authenticated.
pub fn run(_config: &config::Config) -> anyhow::Result<()> {
    anyhow::bail!(
        "the precompiled module cache is disabled until its artifacts are immutable and authenticated"
    )
}
