use std::collections::BTreeMap;

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

    let all: BTreeMap<String, Vec<String>> =
        genvm::runners::cache::read_registry(&registry_dir.join("all.json"))?;
    let latest: BTreeMap<String, String> =
        genvm::runners::cache::read_registry(&registry_dir.join("latest.json"))?;

    // (a) every runner `latest` resolves to must also be listed in `all`.
    for (id, hash) in &latest {
        let present = all
            .get(id)
            .is_some_and(|hashes| hashes.iter().any(|h| h == hash));
        if !present {
            anyhow::bail!("latest runner {id}:{hash} is not present in all.json");
        }
    }

    // (b) every runner in `all` exists on disk with the expected content hash.
    let mut checked = 0usize;
    for (id, hashes) in &all {
        for hash in hashes {
            let mut runner_path = runners_dir.to_owned();
            runners::append_runner_subpath(id, hash, &mut runner_path);
            runner_path.set_extension("zip");

            let data = std::fs::read(&runner_path)
                .with_context(|| format!("reading runner {id}:{hash} at {runner_path:?}"))?;

            use sha2::Digest as _;
            let digest: [u8; 32] = sha2::Sha256::digest(&data).into();
            let got = genvm_common::Bytes32Hash::from_bytes(digest).to_gvm32();

            if got != *hash {
                anyhow::bail!(
                    "runner {id} at {runner_path:?} has wrong content hash: expected {hash}, got {got}"
                );
            }

            checked += 1;
        }
    }

    log_info!(runners = checked; "all runners present with correct hashes");

    if args.precompile {
        super::precompile::run(&config)?;
    }

    Ok(())
}
