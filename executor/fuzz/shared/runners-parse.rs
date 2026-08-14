use genvm::rt::supervisor::actions::check_mapping_target;
use genvm::runners::{self, InitAction};
use genvm::wasi::vfs::split_normalize_path;
use genvm_common::internal_constants::runner_limits;

/// What `Archive::validate_entry_name` promises, restated: every accepted name is
/// a relative path of ordinary components. Anything else reaches the file mapping
/// in `Ctx::apply` as a suffix and can steer where it lands.
fn assert_entry_name_is_safe(name: &str) {
    assert!(!name.is_empty(), "accepted entry name is empty");
    assert!(
        !name.starts_with('/'),
        "accepted entry name is absolute: {name:?}"
    );
    assert!(
        !name.contains('\\'),
        "accepted entry name contains a backslash: {name:?}"
    );
    assert!(
        !name.ends_with('/'),
        "accepted file entry name has a trailing slash: {name:?}"
    );
    for component in name.split('/') {
        assert!(
            !component.is_empty() && component != "." && component != "..",
            "accepted entry name has an invalid component: {name:?}"
        );
    }
}

fn lands_in_vm(path: &str) -> bool {
    split_normalize_path(path, true).first() == Some(&"vm")
}

/// A mapping target that survives the gate must still be outside `/vm/` once the
/// VFS has normalized it -- the gate inspects only the first component, so the
/// two can only agree while the `..` rejection holds.
fn assert_mapping_gate_agrees_with_vfs(to: &str) {
    if check_mapping_target(to).is_ok() {
        assert!(
            !lands_in_vm(to),
            "mapping target accepted by the gate resolves into /vm/: {to:?}"
        );
    }
}

/// Mirrors `InitAction::validate` without sharing its recursion: max nesting, and
/// whether any string carries a nul that would truncate a C path or env entry.
fn reference_walk(action: &InitAction, depth: usize) -> (usize, bool) {
    let strings: Vec<&str> = match action {
        InitAction::MapFile { to, file } => vec![to, file],
        InitAction::AddEnv { name, val } => vec![name, val],
        InitAction::SetArgs(args) => args.0.iter().map(String::as_str).collect(),
        InitAction::Depends(s) => vec![s],
        InitAction::LinkWasm(s) | InitAction::StartWasm(s) => vec![s],
        InitAction::When { .. } | InitAction::Seq(_) => vec![],
        InitAction::With { runner, .. } => vec![runner],
    };
    let mut has_nul = strings.iter().any(|s| s.contains('\0'));

    let children: Vec<&InitAction> = match action {
        InitAction::When { action, .. } | InitAction::With { action, .. } => vec![action],
        InitAction::Seq(actions) => actions.iter().collect(),
        _ => vec![],
    };

    let mut max_depth = depth;
    for child in children {
        let (child_depth, child_nul) = reference_walk(child, depth + 1);
        max_depth = max_depth.max(child_depth);
        has_nul |= child_nul;
    }
    (max_depth, has_nul)
}

fn for_each_action(action: &InitAction, visit: &mut impl FnMut(&InitAction)) {
    visit(action);
    match action {
        InitAction::When { action, .. } | InitAction::With { action, .. } => {
            for_each_action(action, visit)
        }
        InitAction::Seq(actions) => {
            for action in actions {
                for_each_action(action, visit);
            }
        }
        _ => {}
    }
}

fn assert_actions_are_appliable(actions: &InitAction, archive: &runners::Archive) {
    let (depth, has_nul) = reference_walk(actions, 0);
    assert!(!has_nul, "accepted runner.json carries a nul in a string");
    assert!(
        depth < runner_limits::INIT_ACTION_DEPTH as usize,
        "accepted runner.json nests {depth} deep"
    );

    for_each_action(actions, &mut |action| match action {
        InitAction::MapFile { to, .. } => {
            assert_mapping_gate_agrees_with_vfs(to);

            // The directory branch of `Ctx::apply` appends each archive entry to
            // `to`; a target the gate lets through must not become one it would
            // not, whichever names the archive supplies.
            if check_mapping_target(to).is_err() {
                return;
            }
            for name in archive.data.keys() {
                let mut composed = String::from(&**to);
                if !composed.ends_with('/') {
                    composed.push('/');
                }
                composed.push_str(name);
                assert_mapping_gate_agrees_with_vfs(&composed);
            }
        }
        InitAction::Depends(id) | InitAction::With { runner: id, .. } => {
            // Resolution needs a supervisor; the id grammar does not. The module
            // spells the builtin `name:hash` grammar out twice, and the two must
            // not drift apart -- `verify_runner` is what the disk lookup trusts.
            if let Some(runners::IdUnresolved::Builtin { name, hash }) =
                runners::parse_runner_id(id)
            {
                assert_eq!(
                    runners::verify_runner(id),
                    Some((name.as_str(), hash.as_str())),
                    "the two builtin runner id parsers disagree: {id:?}"
                );
            }
        }
        _ => {}
    });
}

fn archive_snapshot(archive: &runners::Archive) -> Vec<(String, bytes::Bytes)> {
    archive
        .data
        .iter()
        .map(|(name, data)| (name.clone(), data.clone()))
        .collect()
}

pub fn assert_parse_properties(runtime: &tokio::runtime::Runtime, code: Vec<u8>) {
    let code = bytes::Bytes::from(code);

    let Ok(archive) = runners::parse(code.clone()) else {
        assert!(
            runners::parse(code).is_err(),
            "parse rejected and then accepted the same code"
        );
        return;
    };

    // Runner ids are content hashes, so two runs of one blob that disagree on the
    // archive are a consensus fork rather than a local inconsistency.
    let again =
        runners::parse(code.clone()).expect("parse accepted and then rejected the same code");
    assert_eq!(
        archive_snapshot(&archive),
        archive_snapshot(&again),
        "parse is not deterministic"
    );
    assert_eq!(archive.total_size, again.total_size);
    assert_eq!(
        u64::from(archive.total_size),
        code.len() as u64,
        "total_size does not describe the parsed blob"
    );

    for name in archive.data.keys() {
        assert_entry_name_is_safe(name);
    }

    let cache = runners::ArchiveCache::new(
        symbol_table::GlobalSymbol::from("fuzz:parse"),
        archive.clone(),
    );

    // A malformed version is an error, never a panic, and never a runner that
    // claims a major it does not have.
    let _ = cache.get_version();

    let Ok(actions) = runtime.block_on(cache.get_actions()) else {
        return;
    };
    assert!(
        actions.validate().is_ok(),
        "get_actions returned a tree that does not validate"
    );
    assert_actions_are_appliable(&actions, &archive);
}
