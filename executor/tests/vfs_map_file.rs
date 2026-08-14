use genvm::wasi::{base, preview1::Context};
use genvm_common::Bytes32Hash;

fn context() -> Context {
    Context::new(
        chrono::DateTime::from_timestamp(0, 0).unwrap(),
        base::Config {
            needs_error_fingerprint: false,
            permissions: base::Permissions {
                deterministic: true,
                write_storage: false,
                send_messages: false,
                call_others: false,
                spawn_nondet: false,
                register_runners: false,
                can_use_balance_for_message_fees: false,
            },
            execution: base::Execution {
                state_mode: genvm::public_abi::StorageType::Default,
                topmost_runner_id: genvm::runners::Id::Custom {
                    hash: Bytes32Hash::ZERO,
                },
            },
        },
        [0; 32],
    )
}

/// A `.` component in a destination must normalize away instead of becoming a
/// directory name no guest path can reach.
#[test]
fn dot_component_in_a_destination_stays_reachable() {
    let mut context = context();

    context
        .map_file("/parent/./file", bytes::Bytes::from_static(b"value"))
        .unwrap();

    context
        .map_file("/parent/file", bytes::Bytes::from_static(b"other"))
        .expect("the `.` destination must have landed at /parent/file");
}

/// Overwriting a directory with a file drops every entry beneath it, so the
/// contents of an earlier mapping vanish without a diagnostic.
#[test]
fn mapping_a_file_over_a_directory_does_not_delete_the_subtree() {
    let mut context = context();
    context
        .map_file(
            "/mapped/child",
            bytes::Bytes::from_static(b"child contents"),
        )
        .unwrap();

    assert!(
        context
            .map_file("/mapped", bytes::Bytes::from_static(b"replacement"))
            .is_err(),
        "mapping a file over an existing directory must not silently delete its children"
    );
}
