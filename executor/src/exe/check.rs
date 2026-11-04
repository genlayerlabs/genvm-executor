use anyhow::Context as _;
use genvm::{config, runners};

use genvm_common::*;

#[derive(clap::Args, Debug)]
pub struct Args {
    #[arg(
        long,
        default_value_t = false,
        help = "after verification, precompile all runners into the on-disk cache"
    )]
    precompile: bool,
}

pub fn handle(args: Args, config: config::Config) -> anyhow::Result<()> {
    let registry_dir = std::path::Path::new(&config.registry_dir);
    let runners_dir = std::path::Path::new(&config.runners_dir);

    log_info!(registry_dir:? = registry_dir, runners_dir:? = runners_dir; "checking install");

    let registry = genvm::runners::cache::read_registry(&registry_dir.join("all.json"))?;

    // Every listed runner exists on disk with the expected content hash.
    let mut checked = 0usize;
    for (id, entry) in &registry {
        for hash in entry.versions.keys() {
            let mut runner_path = runners_dir.to_owned();
            runners::append_runner_subpath(id.as_str(), hash.as_str(), &mut runner_path);
            runner_path.set_extension("zip");

            let data = std::fs::read(&runner_path)
                .with_context(|| format!("reading runner {id}:{hash} at {runner_path:?}"))?;

            use sha2::Digest as _;
            let digest: [u8; 32] = sha2::Sha256::digest(&data).into();
            let got = genvm_common::Bytes32Hash::from_bytes(digest).to_gvm32();

            if got != hash.as_str() {
                anyhow::bail!(
                    "runner {id} at {runner_path:?} has wrong content hash: expected {hash}, got {got}"
                );
            }

            checked += 1;
        }
    }

    log_info!(@operator, runners = checked; "all runners present with correct hashes");

    if args.precompile {
        super::precompile::run(&config)?;
    }

    Ok(())
}
