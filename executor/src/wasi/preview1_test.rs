use genvm_common::Bytes32Hash;

use super::*;
use generated::wasi_snapshot_preview1::WasiSnapshotPreview1 as _;

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
                can_use_balance_for_message_fees: false,
            },
            execution: base::Execution {
                state_mode: crate::public_abi::StorageType::Default,
                topmost_runner_id: crate::runners::Id::Custom {
                    hash: Bytes32Hash::ZERO,
                },
            },
        },
        [0; 32],
    )
}

fn assert_errno(error: generated::types::Error, expected: generated::types::Errno) {
    assert_eq!(error.downcast_ref(), Some(&expected));
}

#[test]
fn supported_rights_match_descriptor_operations() {
    let file = supported_rights(generated::types::Filetype::RegularFile);
    assert_eq!(
        file.base,
        generated::types::Rights::FD_DATASYNC
            | generated::types::Rights::FD_READ
            | generated::types::Rights::FD_SEEK
            | generated::types::Rights::FD_SYNC
            | generated::types::Rights::FD_TELL
            | generated::types::Rights::FD_ADVISE
            | generated::types::Rights::FD_FILESTAT_GET
    );
    assert!(file.inheriting.is_empty());

    let directory = supported_rights(generated::types::Filetype::Directory);
    assert_eq!(
        directory.base,
        generated::types::Rights::PATH_OPEN
            | generated::types::Rights::FD_READDIR
            | generated::types::Rights::PATH_FILESTAT_GET
            | generated::types::Rights::FD_FILESTAT_GET
    );
    assert_eq!(directory.inheriting, file.base | directory.base);

    let limited = limit_rights(
        generated::types::Rights::FD_SEEK,
        generated::types::Rights::empty(),
        file.base,
        generated::types::Filetype::RegularFile,
    );
    assert_eq!(
        limited.base,
        generated::types::Rights::FD_SEEK | generated::types::Rights::FD_TELL
    );
}

#[test]
fn path_open_enforces_type_and_limits_rights() {
    let mut context = context();
    context
        .map_file("/file", bytes::Bytes::from_static(b"contents"))
        .unwrap();
    let limiter = rt::memlimiter::Limiter::new();
    let mut vfs = vfs::VFS::new(Vec::new(), limiter.clone()).unwrap();
    let mut guest_bytes = *b".file";
    let mut memory = GuestMemory::Unshared(&mut guest_bytes);
    let mut wasi = ContextVFS {
        vfs: &mut vfs,
        context: &mut context,
    };
    let root_fd = generated::types::Fd::from(3);
    let root_status = wasi.fd_fdstat_get(&mut memory, root_fd).unwrap();
    assert_eq!(root_status.fs_rights_base, directory_base_rights());
    assert_eq!(
        root_status.fs_rights_inheriting,
        directory_base_rights() | regular_file_base_rights()
    );

    let error = wasi
        .path_open(
            &mut memory,
            root_fd,
            generated::types::Lookupflags::empty(),
            GuestPtr::new((1, 4)),
            generated::types::Oflags::DIRECTORY,
            generated::types::Rights::all(),
            generated::types::Rights::all(),
            generated::types::Fdflags::empty(),
        )
        .unwrap_err();
    assert_errno(error, generated::types::Errno::Notdir);
    assert_eq!(limiter.get_remaining_memory(), u32::MAX);

    let dir_fd = wasi
        .path_open(
            &mut memory,
            root_fd,
            generated::types::Lookupflags::empty(),
            GuestPtr::new((0, 1)),
            generated::types::Oflags::empty(),
            generated::types::Rights::PATH_OPEN,
            generated::types::Rights::FD_READ,
            generated::types::Fdflags::empty(),
        )
        .unwrap();
    let dir_fd_raw: u32 = dir_fd.into();
    assert_eq!(
        dir_fd_raw, 4,
        "a failed open must not allocate a descriptor"
    );
    let dir_status = wasi.fd_fdstat_get(&mut memory, dir_fd).unwrap();
    assert_eq!(
        dir_status.fs_rights_base,
        generated::types::Rights::PATH_OPEN
    );
    assert_eq!(
        dir_status.fs_rights_inheriting,
        generated::types::Rights::FD_READ
    );
    let file_fd = wasi
        .path_open(
            &mut memory,
            dir_fd,
            generated::types::Lookupflags::empty(),
            GuestPtr::new((1, 4)),
            generated::types::Oflags::empty(),
            generated::types::Rights::all(),
            generated::types::Rights::all(),
            generated::types::Fdflags::empty(),
        )
        .unwrap();
    let status = wasi.fd_fdstat_get(&mut memory, file_fd).unwrap();
    assert_eq!(status.fs_rights_base, generated::types::Rights::FD_READ);
    assert!(status.fs_rights_inheriting.is_empty());

    let error = wasi
        .fd_advise(&mut memory, file_fd, 0, 0, generated::types::Advice::Normal)
        .unwrap_err();
    assert_errno(error, generated::types::Errno::Notcapable);
}

#[test]
fn only_the_root_descriptor_is_preopened() {
    let mut context = context();
    let mut vfs = vfs::VFS::new(Vec::new(), rt::memlimiter::Limiter::new()).unwrap();
    let mut guest_bytes = *b".x";
    let root_fd = generated::types::Fd::from(3);
    {
        let mut memory = GuestMemory::Unshared(&mut guest_bytes);
        let mut wasi = ContextVFS {
            vfs: &mut vfs,
            context: &mut context,
        };
        let generated::types::Prestat::Dir(root) =
            wasi.fd_prestat_get(&mut memory, root_fd).unwrap();
        assert_eq!(root.pr_name_len, 1);
        let error = wasi
            .fd_prestat_dir_name(&mut memory, root_fd, GuestPtr::new(1), 0)
            .unwrap_err();
        assert_errno(error, generated::types::Errno::Overflow);
        wasi.fd_prestat_dir_name(&mut memory, root_fd, GuestPtr::new(1), 1)
            .unwrap();
    }
    assert_eq!(&guest_bytes[1..2], b"/");

    let mut memory = GuestMemory::Unshared(&mut guest_bytes);
    let mut wasi = ContextVFS {
        vfs: &mut vfs,
        context: &mut context,
    };
    let dir_fd = wasi
        .path_open(
            &mut memory,
            root_fd,
            generated::types::Lookupflags::empty(),
            GuestPtr::new((0, 1)),
            generated::types::Oflags::empty(),
            generated::types::Rights::all(),
            generated::types::Rights::all(),
            generated::types::Fdflags::empty(),
        )
        .unwrap();
    let error = wasi.fd_prestat_get(&mut memory, dir_fd).unwrap_err();
    assert_errno(error, generated::types::Errno::Badf);
    let error = wasi
        .fd_prestat_dir_name(&mut memory, dir_fd, GuestPtr::new(1), 1)
        .unwrap_err();
    assert_errno(error, generated::types::Errno::Badf);
}

#[test]
fn shared_filestat_uses_one_link() {
    assert_eq!(
        filestat(generated::types::Filetype::RegularFile, 7).nlink,
        1
    );
    assert_eq!(filestat(generated::types::Filetype::Directory, 0).nlink, 1);
}
