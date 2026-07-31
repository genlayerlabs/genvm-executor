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

fn read_registry<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> anyhow::Result<T> {
    let contents = std::fs::read_to_string(path).with_context(|| format!("reading {path:?}"))?;
    serde_json::from_str(&contents).with_context(|| format!("parsing {path:?}"))
}

pub fn handle(args: Args, config: config::Config) -> anyhow::Result<()> {
    let registry_dir = std::path::Path::new(&config.registry_dir);
    let runners_dir = std::path::Path::new(&config.runners_dir);

    log_info!(registry_dir:? = registry_dir, runners_dir:? = runners_dir; "checking install");

    let all: BTreeMap<String, Vec<String>> = read_registry(&registry_dir.join("all.json"))?;
    let latest: BTreeMap<String, String> = read_registry(&registry_dir.join("latest.json"))?;

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
            runner_path.set_extension("tar");

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

#[cfg(test)]
mod tests {
    use super::*;

    fn bucket() -> config::FeesBucketConfig {
        config::FeesBucketConfig {
            bucket_no: vec![0],
            subtract_on_start_expr: "0".to_owned(),
            delta_expr: "0".to_owned(),
        }
    }

    fn test_root(label: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "genvm-check-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn test_config(root: &std::path::Path) -> config::Config {
        config::Config {
            modules: config::Modules {
                llm: config::Module {
                    address: String::new(),
                },
                web: config::Module {
                    address: String::new(),
                },
            },
            fees: config::FeesConfig {
                expr_prelude: String::new(),
                storage: bucket(),
                message_receipt: bucket(),
                nondet_output: bucket(),
                message_fee: bucket(),
                event: bucket(),
            },
            cache_dir: root.join("cache").to_string_lossy().into_owned(),
            runners_dir: root.join("runners").to_string_lossy().into_owned(),
            registry_dir: root.join("registry").to_string_lossy().into_owned(),
            base: genvm_common::BaseConfig {
                threads: 1,
                blocking_threads: 1,
                log_level: genvm_common::logger::Level::Info,
                log_disable: String::new(),
            },
        }
    }

    #[test]
    fn check_rejects_short_registry_hash_without_panicking() {
        let root = test_root("short-runner-hash");
        let registry = root.join("registry");
        let runners = root.join("runners");
        std::fs::create_dir_all(&registry).unwrap();
        std::fs::create_dir_all(&runners).unwrap();
        std::fs::write(registry.join("all.json"), r#"{"runner":["x"]}"#).unwrap();
        std::fs::write(registry.join("latest.json"), "{}").unwrap();

        let config = test_config(&root);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle(Args { precompile: false }, config)
        }));
        std::fs::remove_dir_all(root).unwrap();

        assert!(
            result.is_ok(),
            "genvm check must return an installation error instead of panicking on a short registry hash"
        );
        assert!(
            result.unwrap().is_err(),
            "genvm check must reject a malformed registry hash"
        );
    }

    #[test]
    fn check_rejects_runner_name_that_escapes_runners_directory() {
        let root = test_root("runner-name-traversal");
        let registry = root.join("registry");
        let runners = root.join("runners");
        std::fs::create_dir_all(&registry).unwrap();
        std::fs::create_dir_all(&runners).unwrap();

        let contents = b"outside runner artifact";
        use sha2::Digest as _;
        let digest: [u8; 32] = sha2::Sha256::digest(contents).into();
        let hash = genvm_common::Bytes32Hash::from_bytes(digest).to_gvm32();
        let mut escaped_path = root.join("outside");
        escaped_path.push(&hash[..2]);
        escaped_path.push(&hash[2..]);
        escaped_path.set_extension("tar");
        std::fs::create_dir_all(escaped_path.parent().unwrap()).unwrap();
        std::fs::write(&escaped_path, contents).unwrap();

        let all = serde_json::to_vec(&std::collections::BTreeMap::from([(
            "../outside",
            vec![hash],
        )]))
        .unwrap();
        std::fs::write(registry.join("all.json"), all).unwrap();
        std::fs::write(registry.join("latest.json"), "{}").unwrap();

        let result = handle(Args { precompile: false }, test_config(&root));
        std::fs::remove_dir_all(root).unwrap();

        assert!(
            result.is_err(),
            "genvm check must reject a registry runner name that traverses outside runners_dir"
        );
    }
}
