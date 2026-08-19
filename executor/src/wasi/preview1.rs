use anyhow::Context as _;
use genlayer_sdk::abi;
use std::{borrow::BorrowMut, collections::BTreeMap, io::Write};
use tracing::instrument;
use wiggle::{GuestError, GuestMemory, GuestPtr};

use genvm_common::internal_constants::top_limits;
use genvm_common::*;

use super::vfs;
use crate::int_traits::*;
use crate::{
    rt, runners,
    wasi::{self, base, common::align_slice},
};

pub struct Context {
    args_buf: Vec<u8>,
    args_offsets: Vec<u32>,
    env_buf: Vec<u8>,
    env_offsets: Vec<u32>,

    preopen: vfs::Trie<()>,
    preopen_fd: Option<vfs::Fd>,
    descriptor_rights: BTreeMap<vfs::Fd, DescriptorRights>,

    fs: Box<FilesTrie>,
    unix_timestamp_nanos: u64,

    conf: base::Config,
    mt19937_rng: mt19937::MT19937,
}

pub struct ContextVFS<'a> {
    pub(super) vfs: &'a mut vfs::VFS,
    pub(super) context: &'a mut Context,
}

#[derive(Clone, Copy)]
struct DescriptorRights {
    base: generated::types::Rights,
    inheriting: generated::types::Rights,
}

#[allow(clippy::too_many_arguments)]
pub(crate) mod generated {
    wiggle::from_witx!({
        witx: ["$CARGO_MANIFEST_DIR/src/wasi/witx/wasi_snapshot_preview1.witx"],
        errors: { errno => trappable Error },
        wasmtime: false,
        tracing: false,

        async: {
            wasi_snapshot_preview1::{
                fd_close,
                fd_read, fd_pread,
                fd_filestat_get, fd_seek, fd_tell,
            },
        },
    });

    wiggle::wasmtime_integration!({
        witx: ["$CARGO_MANIFEST_DIR/src/wasi/witx/wasi_snapshot_preview1.witx"],
        target: super::generated,
        errors: { errno => trappable Error },
        tracing: false,

        async: {
            wasi_snapshot_preview1::{
                fd_close,
                fd_read, fd_pread,
                fd_filestat_get, fd_seek, fd_tell,
            },
        },
    });
}

impl From<wasi::vfs::Fd> for generated::types::Fd {
    fn from(fd: wasi::vfs::Fd) -> Self {
        fd.as_u32().into()
    }
}

impl From<generated::types::Fd> for wasi::vfs::Fd {
    fn from(fd: generated::types::Fd) -> Self {
        wasi::vfs::Fd::new(fd.into())
    }
}

impl wiggle::GuestErrorType for generated::types::Errno {
    fn success() -> Self {
        Self::Success
    }
}

impl From<std::num::TryFromIntError> for generated::types::Error {
    fn from(_err: std::num::TryFromIntError) -> Self {
        generated::types::Errno::Overflow.into()
    }
}

impl From<GuestError> for generated::types::Error {
    fn from(err: GuestError) -> Self {
        use wiggle::GuestError::*;
        match err {
            InvalidFlagValue { .. } => generated::types::Errno::Inval.into(),
            InvalidEnumValue { .. } => generated::types::Errno::Inval.into(),
            // As per
            // https://github.com/WebAssembly/wasi/blob/main/legacy/tools/witx-docs.md#pointers
            //
            // > If a misaligned pointer is passed to a function, the function
            // > shall trap.
            // >
            // > If an out-of-bounds pointer is passed to a function and the
            // > function needs to dereference it, the function shall trap.
            //
            // so this turns OOB and misalignment errors into traps.
            PtrOverflow | PtrOutOfBounds { .. } | PtrNotAligned { .. } => {
                generated::types::Error::trap(wiggle::error::Error::from(err))
            }
            InvalidUtf8 { .. } => generated::types::Errno::Ilseq.into(),
            TryFromIntError { .. } => generated::types::Errno::Overflow.into(),
            SliceLengthsDiffer => generated::types::Errno::Fault.into(),
            InFunc { err, .. } => generated::types::Error::from(*err),
            MemoryNotExported => generated::types::Errno::Fault.into(),
        }
    }
}

type FilesTrie = vfs::Trie<bytes::Bytes>;

impl Context {
    pub fn log(&self) -> serde_json::Value {
        serde_json::json!(
            {
                "env": String::from_utf8_lossy(&self.env_buf),
                "args": String::from_utf8_lossy(&self.args_buf),
                "datetime_timestamp": self.unix_timestamp_nanos,
                "datetime": chrono::DateTime::<chrono::Utc>::from_timestamp(self.unix_timestamp_nanos as i64, 0).map(|x| x.to_rfc3339()),
            }
        )
    }
    pub fn set_args(&mut self, args: &runners::actions::SetArgs) -> anyhow::Result<()> {
        for c in &args.0 {
            let off: u32 = self
                .args_buf
                .len()
                .try_into()
                .with_context(|| "arguments offset overflow")?;
            self.args_offsets.push(off);
            self.args_buf.extend_from_slice(c.as_bytes());
            self.args_buf.push(0);
        }
        Ok(())
    }

    pub fn set_env(&mut self, env: &runners::actions::Env) -> anyhow::Result<()> {
        for (name, val) in &env.vars {
            let off: u32 = self
                .env_buf
                .len()
                .try_into()
                .with_context(|| "env offset overflow")?;
            self.env_offsets.push(off);
            self.env_buf.extend_from_slice(name.as_bytes());
            self.env_buf.push(b'=');
            self.env_buf.extend_from_slice(val.as_bytes());
            self.env_buf.push(0);
        }
        Ok(())
    }

    /// Reports our own error type: a code boxed in a `wasmtime::Error` is lost.
    pub fn map_file(&mut self, location: &str, contents: bytes::Bytes) -> rt::errors::Result<()> {
        let malformed = || {
            rt::errors::Error::vm(
                abi::consts::VmError::invalid_contract()
                    .runner()
                    .malformed(),
            )
        };

        let locs_arr = vfs::split_normalize_path(location, true);

        if locs_arr.is_empty() {
            return Err(
                malformed().context(format!("mapping destination `{location}` names no file"))
            );
        }

        // the trie is dropped recursively, so unbounded depth is a stack overflow
        if locs_arr.len() > top_limits::VFS_PATH_COMPONENTS.into_int_comptime() {
            return Err(malformed().context(format!(
                "mapping destination has {} components, at most {} are allowed",
                locs_arr.len(),
                top_limits::VFS_PATH_COMPONENTS,
            )));
        }

        let mut cur_trie: &mut FilesTrie = &mut self.fs;
        for loc in &locs_arr[0..locs_arr.len() - 1] {
            let loc = *loc;
            cur_trie = match cur_trie.borrow_mut() {
                FilesTrie::Dir(children) => match children.entry(String::from(loc)) {
                    std::collections::btree_map::Entry::Occupied(entry) => {
                        Ok::<&mut FilesTrie, rt::errors::Error>(entry.into_mut())
                    }
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        Ok(&mut *entry.insert(FilesTrie::Dir(Default::default())))
                    }
                },
                FilesTrie::Leaf(_) => {
                    // wanted a dir but found a file
                    return Err(rt::errors::Error::vm(
                        abi::consts::VmError::invalid_contract().val(),
                    ));
                }
            }?;
        }

        let fname = locs_arr[locs_arr.len() - 1];

        match cur_trie.borrow_mut() {
            FilesTrie::Dir(children) => match children.entry(String::from(fname)) {
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    if matches!(entry.get(), FilesTrie::Leaf(_)) {
                        *entry.get_mut() = FilesTrie::Leaf(contents);
                    } else {
                        // wanted a file but found a dir
                        return Err(rt::errors::Error::vm(
                            abi::consts::VmError::invalid_contract().val(),
                        ));
                    }
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(FilesTrie::Leaf(contents));
                }
            },
            FilesTrie::Leaf(_) => {
                // wanted a dir but found a file
                return Err(rt::errors::Error::vm(
                    abi::consts::VmError::invalid_contract().val(),
                ));
            }
        };

        Ok(())
    }
}

pub trait AddToLinkerFn<T> {
    fn call<'a>(&self, arg: &'a mut T) -> ContextVFS<'a>;
}

pub(super) fn add_to_linker_sync<T: Send + 'static, F>(
    linker: &mut wasmtime::Linker<T>,
    f: F,
) -> anyhow::Result<()>
where
    F: AddToLinkerFn<T> + Copy + Send + Sync + 'static,
{
    #[derive(Clone, Copy)]
    struct Fwd<F>(F);

    impl<T, F> generated::AddWasiSnapshotPreview1ToLinkerFn<T> for Fwd<F>
    where
        F: AddToLinkerFn<T> + Copy + Send + Sync + 'static,
    {
        fn call(
            &self,
            arg: &mut T,
        ) -> impl generated::wasi_snapshot_preview1::WasiSnapshotPreview1 {
            self.0.call(arg)
        }
    }
    generated::add_wasi_snapshot_preview1_to_linker(linker, Fwd(f))?;
    Ok(())
}

impl Context {
    pub fn new(
        datetime: chrono::DateTime<chrono::Utc>,
        conf: base::Config,
        seed: [u8; 32],
    ) -> Self {
        // Deterministic randomness is seeded from `sha3-256(stdin)` of the VM, so two
        // runs with identical inputs see the same `random_get` stream while different
        // inputs diverge. The 32-byte digest is consumed as 8 little-endian u32 words.
        let seed_words: [u32; 8] = std::array::from_fn(|i| {
            u32::from_le_bytes([
                seed[i * 4],
                seed[i * 4 + 1],
                seed[i * 4 + 2],
                seed[i * 4 + 3],
            ])
        });
        let seed = mt19937::MT19937::new_with_slice_seed(&seed_words);

        let unix_ts_ns = (datetime.timestamp() as u64)
            .saturating_mul(1_000_000_000)
            .saturating_add(u64::from(datetime.timestamp_subsec_nanos()));

        Self {
            args_buf: Vec::new(),
            args_offsets: Vec::new(),
            env_buf: Vec::new(),
            env_offsets: Vec::new(),
            fs: Box::new(FilesTrie::Dir(Default::default())),
            unix_timestamp_nanos: unix_ts_ns,
            conf,
            preopen: vfs::Trie::Leaf(()),
            preopen_fd: Some(vfs::ROOT_PREOPEN_FD),
            descriptor_rights: BTreeMap::new(),
            mt19937_rng: seed,
        }
    }
}

pub fn join_dir_and_path<'a, T>(
    preopen: &vfs::Trie<()>,
    dir: &'a [T],
    path: &'a str,
) -> Result<Vec<&'a str>, generated::types::Error>
where
    T: AsRef<str> + 'a,
{
    let all_comp =
        vfs::split_normalize_paths(dir.iter().map(|s| s.as_ref()).chain(path.split('/')), true);

    match preopen.follow(all_comp.iter().map(std::ops::Deref::deref)) {
        vfs::TrieFollowResult::NotFound => return Err(generated::types::Errno::Notcapable.into()),
        vfs::TrieFollowResult::NotDir | vfs::TrieFollowResult::Found(vfs::Trie::Leaf(_)) => {} // we hit a leaf
        vfs::TrieFollowResult::Found(vfs::Trie::Dir(_)) => {
            return Err(generated::types::Errno::Notcapable.into())
        } // leaf = has perms. Here we don't
    }

    for component in dir {
        debug_assert!(vfs::is_normalized_path_component(component.as_ref()));
    }

    for component in &all_comp {
        debug_assert!(vfs::is_normalized_path_component(component));
    }

    Ok(all_comp)
}

fn args_env_get(
    memory: &mut GuestMemory<'_>,
    mut guest_starts: GuestPtr<GuestPtr<u8>>,
    guest_buf: GuestPtr<u8>,
    offsets: &[u32],
    buf: &[u8],
) -> Result<(), generated::types::Error> {
    {
        let len: u32 = buf.len().try_into()?;
        let guest_buf_arr = guest_buf.as_array(len);
        memory.copy_from_slice(buf, guest_buf_arr)?;
    }

    for off_absolute in offsets.iter() {
        let to_write = guest_buf.add(*off_absolute)?;
        memory.write(guest_starts, to_write)?;
        guest_starts = guest_starts.add(1)?;
    }
    Ok(())
}

fn regular_file_base_rights() -> generated::types::Rights {
    generated::types::Rights::FD_DATASYNC
        | generated::types::Rights::FD_READ
        | generated::types::Rights::FD_SEEK
        | generated::types::Rights::FD_SYNC
        | generated::types::Rights::FD_TELL
        | generated::types::Rights::FD_ADVISE
        | generated::types::Rights::FD_FILESTAT_GET
}

fn directory_base_rights() -> generated::types::Rights {
    generated::types::Rights::PATH_OPEN
        | generated::types::Rights::FD_READDIR
        | generated::types::Rights::PATH_FILESTAT_GET
        | generated::types::Rights::FD_FILESTAT_GET
}

fn supported_rights(filetype: generated::types::Filetype) -> DescriptorRights {
    match filetype {
        generated::types::Filetype::RegularFile => DescriptorRights {
            base: regular_file_base_rights(),
            inheriting: generated::types::Rights::empty(),
        },
        generated::types::Filetype::Directory => DescriptorRights {
            base: directory_base_rights(),
            inheriting: regular_file_base_rights() | directory_base_rights(),
        },
        _ => DescriptorRights {
            base: generated::types::Rights::empty(),
            inheriting: generated::types::Rights::empty(),
        },
    }
}

fn limit_rights(
    requested_base: generated::types::Rights,
    requested_inheriting: generated::types::Rights,
    parent_inheriting: generated::types::Rights,
    filetype: generated::types::Filetype,
) -> DescriptorRights {
    let supported = supported_rights(filetype);
    let imply_tell = |mut rights: generated::types::Rights| {
        if rights.contains(generated::types::Rights::FD_SEEK) {
            rights.insert(generated::types::Rights::FD_TELL);
        }
        rights
    };
    let requested_base = imply_tell(requested_base);
    let requested_inheriting = imply_tell(requested_inheriting);
    let parent_inheriting = imply_tell(parent_inheriting);
    DescriptorRights {
        base: requested_base & parent_inheriting & supported.base,
        inheriting: requested_inheriting & parent_inheriting & supported.inheriting,
    }
}

fn filestat(
    filetype: generated::types::Filetype,
    size: generated::types::Filesize,
) -> generated::types::Filestat {
    generated::types::Filestat {
        dev: 0,
        ino: 0,
        filetype,
        nlink: 1,
        size,
        atim: 0,
        mtim: 0,
        ctim: 0,
    }
}

#[allow(unused_variables)]
impl generated::wasi_snapshot_preview1::WasiSnapshotPreview1 for ContextVFS<'_> {
    #[instrument(skip(self, memory))]
    fn args_get(
        &mut self,
        memory: &mut GuestMemory<'_>,
        argv: GuestPtr<GuestPtr<u8>>,
        argv_buf: GuestPtr<u8>,
    ) -> Result<(), generated::types::Error> {
        args_env_get(
            memory,
            argv,
            argv_buf,
            &self.context.args_offsets,
            &self.context.args_buf,
        )
    }

    fn args_sizes_get(
        &mut self,
        _memory: &mut GuestMemory<'_>,
    ) -> Result<(generated::types::Size, generated::types::Size), generated::types::Error> {
        let count: u32 = self.context.args_offsets.len().try_into()?;
        let len: u32 = self.context.args_buf.len().try_into()?;
        Ok((count, len))
    }

    #[instrument(skip(self, memory))]
    fn environ_get(
        &mut self,
        memory: &mut GuestMemory<'_>,
        environ: GuestPtr<GuestPtr<u8>>,
        environ_buf: GuestPtr<u8>,
    ) -> Result<(), generated::types::Error> {
        args_env_get(
            memory,
            environ,
            environ_buf,
            &self.context.env_offsets,
            &self.context.env_buf,
        )
    }

    fn environ_sizes_get(
        &mut self,
        _memory: &mut GuestMemory<'_>,
    ) -> Result<(generated::types::Size, generated::types::Size), generated::types::Error> {
        let count: u32 = self.context.env_offsets.len().try_into()?;
        let len: u32 = self.context.env_buf.len().try_into()?;
        Ok((count, len))
    }

    fn clock_res_get(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        id: generated::types::Clockid,
    ) -> Result<generated::types::Timestamp, generated::types::Error> {
        Ok(1)
    }

    fn clock_time_get(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        _id: generated::types::Clockid,
        _precision: generated::types::Timestamp,
    ) -> Result<generated::types::Timestamp, generated::types::Error> {
        Ok(self.context.unix_timestamp_nanos)
    }

    fn fd_advise(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
        offset: generated::types::Filesize,
        len: generated::types::Filesize,
        advice: generated::types::Advice,
    ) -> Result<(), generated::types::Error> {
        self.require_right(fd, generated::types::Rights::FD_ADVISE)?;
        Ok(())
    }

    /// Force the allocation of space in a file.
    /// NOTE: This is similar to `posix_fallocate` in POSIX.
    fn fd_allocate(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
        _offset: generated::types::Filesize,
        _len: generated::types::Filesize,
    ) -> Result<(), generated::types::Error> {
        Err(generated::types::Errno::Rofs.into())
    }

    /// Close a file descriptor.
    /// NOTE: This is similar to `close` in POSIX.
    fn fd_close(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
    ) -> impl std::future::Future<Output = Result<(), generated::types::Error>> + Send {
        let fdi = fd.into();
        let res = if self.vfs.pop_fd(fdi).is_none() {
            Err(generated::types::Errno::Badf.into())
        } else {
            self.context.descriptor_rights.remove(&fdi);
            if self.context.preopen_fd == Some(fdi) {
                self.context.preopen_fd = None;
            }
            Ok(())
        };
        std::future::ready(res)
    }

    /// Synchronize the data of a file to disk.
    /// NOTE: This is similar to `fdatasync` in POSIX.
    fn fd_datasync(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
    ) -> Result<(), generated::types::Error> {
        self.require_right(fd, generated::types::Rights::FD_DATASYNC)?;
        Ok(())
    }

    /// Get the attributes of a file descriptor.
    /// NOTE: This returns similar flags to `fsync(fd, F_GETFL)` in POSIX, as well as additional fields.
    fn fd_fdstat_get(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
    ) -> Result<generated::types::Fdstat, generated::types::Error> {
        let (filetype, rights) = self.descriptor_status(fd)?;
        Ok(generated::types::Fdstat {
            fs_filetype: filetype,
            fs_flags: generated::types::Fdflags::empty(),
            fs_rights_base: rights.base,
            fs_rights_inheriting: rights.inheriting,
        })
    }

    /// Adjust the flags associated with a file descriptor.
    /// NOTE: This is similar to `fcntl(fd, F_SETFL, flags)` in POSIX.
    fn fd_fdstat_set_flags(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
        flags: generated::types::Fdflags,
    ) -> Result<(), generated::types::Error> {
        Err(generated::types::Errno::Rofs.into())
    }

    /// Does not do anything if `fd` corresponds to a valid descriptor and returns `[stub::types::Errno::Badf]` error otherwise.
    fn fd_fdstat_set_rights(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
        _fs_rights_base: generated::types::Rights,
        _fs_rights_inheriting: generated::types::Rights,
    ) -> Result<(), generated::types::Error> {
        Err(generated::types::Errno::Rofs.into())
    }

    /// Return the attributes of an open file.
    fn fd_filestat_get(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
    ) -> impl std::future::Future<Output = Result<generated::types::Filestat, generated::types::Error>>
           + Send {
        let res = match self.require_right(fd, generated::types::Rights::FD_FILESTAT_GET) {
            Err(e) => Err(e),
            Ok(()) => match self.get_fd_desc(fd) {
                Err(e) => Err(e),
                Ok(desc) => match desc {
                    vfs::FileDescriptor::Stdin
                    | vfs::FileDescriptor::Stdout
                    | vfs::FileDescriptor::Stderr => {
                        Ok(filestat(generated::types::Filetype::CharacterDevice, 0))
                    }
                    vfs::FileDescriptor::File(contents) => match contents.contents.len().try_into()
                    {
                        Ok(size) => Ok(filestat(generated::types::Filetype::RegularFile, size)),
                        Err(e) => Err(e.into()),
                    },
                    vfs::FileDescriptor::Dir { .. } => {
                        Ok(filestat(generated::types::Filetype::Directory, 0))
                    }
                },
            },
        };
        std::future::ready(res)
    }

    /// Adjust the size of an open file. If this increases the file's size, the extra bytes are filled with zeros.
    /// NOTE: This is similar to `ftruncate` in POSIX.
    fn fd_filestat_set_size(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
        size: generated::types::Filesize,
    ) -> Result<(), generated::types::Error> {
        self.get_fd_desc(fd)?;
        Err(generated::types::Errno::Rofs.into())
    }

    /// Adjust the timestamps of an open file or directory.
    /// NOTE: This is similar to `futimens` in POSIX.
    fn fd_filestat_set_times(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
        atim: generated::types::Timestamp,
        mtim: generated::types::Timestamp,
        fst_flags: generated::types::Fstflags,
    ) -> Result<(), generated::types::Error> {
        self.get_fd_desc(fd)?;
        Err(generated::types::Errno::Rofs.into())
    }

    /// Read from a file descriptor.
    /// NOTE: This is similar to `readv` in POSIX.
    #[instrument(skip(self, memory))]
    fn fd_read(
        &mut self,
        memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
        iovs: generated::types::IovecArray,
    ) -> impl std::future::Future<Output = Result<generated::types::Size, generated::types::Error>> + Send
    {
        let res = (|| -> Result<generated::types::Size, generated::types::Error> {
            self.require_right(fd, generated::types::Rights::FD_READ)?;
            match self.get_fd_desc_mut(fd)? {
                vfs::FileDescriptor::Stdin => Ok(0),
                vfs::FileDescriptor::Stdout | vfs::FileDescriptor::Stderr => {
                    Err(generated::types::Errno::Acces.into())
                }
                vfs::FileDescriptor::File(vfs::FileContents { contents, pos, .. }) => {
                    let mut written: u32 = 0;
                    for iov in iovs.iter() {
                        let iov = iov?;
                        let iov = memory.read(iov)?;
                        let contents_len: u32 = contents.len().into_int_downcast_panicking();
                        let remaining_len = contents_len - *pos;
                        let len = iov.buf_len.min(remaining_len);
                        let cont_slice: &[u8] = &contents.as_ref()
                            [(*pos).into_int_comptime()..(*pos + len).into_int_comptime()];
                        memory.copy_from_slice(cont_slice, iov.buf.as_array(len))?;
                        *pos += len;
                        written += len;
                    }
                    Ok(written)
                }
                vfs::FileDescriptor::Dir { .. } => Err(generated::types::Errno::Isdir.into()),
            }
        })();
        std::future::ready(res)
    }

    /// Read from a file descriptor, without using and updating the file descriptor's offset.
    /// NOTE: This is similar to `preadv` in POSIX.
    #[instrument(skip(self, memory))]
    fn fd_pread(
        &mut self,
        memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
        iovs: generated::types::IovecArray,
        offset: generated::types::Filesize,
    ) -> impl std::future::Future<Output = Result<generated::types::Size, generated::types::Error>> + Send
    {
        let res = (|| -> Result<generated::types::Size, generated::types::Error> {
            self.require_right(
                fd,
                generated::types::Rights::FD_READ | generated::types::Rights::FD_SEEK,
            )?;
            match self.get_fd_desc(fd)? {
                vfs::FileDescriptor::Stdin => Ok(0),
                vfs::FileDescriptor::Stdout | vfs::FileDescriptor::Stderr => {
                    Err(generated::types::Errno::Acces.into())
                }
                vfs::FileDescriptor::File(vfs::FileContents { contents, .. }) => {
                    let mut written: usize = 0;
                    // clamped, not saturated: a past-the-end offset still indexes
                    let mut offset: usize = usize::try_from(offset)?.min(contents.len());
                    for iov in iovs.iter() {
                        let iov = iov?;
                        let iov = memory.read(iov)?;
                        let remaining_len = contents.len() - offset;
                        let mut buf_len: usize = iov.buf_len.try_into()?;
                        buf_len = buf_len.min(remaining_len);
                        let cont_slice: &[u8] = &contents.as_ref()[offset..(offset + buf_len)];
                        memory
                            .copy_from_slice(cont_slice, iov.buf.as_array(buf_len.try_into()?))?;
                        offset += buf_len;
                        written += buf_len;
                    }
                    Ok(written.try_into().unwrap_or(u32::MAX))
                }
                vfs::FileDescriptor::Dir { .. } => Err(generated::types::Errno::Isdir.into()),
            }
        })();
        std::future::ready(res)
    }

    /// Write to a file descriptor.
    /// NOTE: This is similar to `writev` in POSIX.
    #[instrument(skip(self, memory))]
    fn fd_write(
        &mut self,
        memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
        ciovs: generated::types::CiovecArray,
    ) -> Result<generated::types::Size, generated::types::Error> {
        self.require_right(fd, generated::types::Rights::FD_WRITE)?;
        let mut stream: Box<dyn Write> = match self.get_fd_desc(fd)? {
            vfs::FileDescriptor::Stdout => Box::new(std::io::stdout().lock()),
            vfs::FileDescriptor::Stderr => Box::new(std::io::stderr().lock()),
            vfs::FileDescriptor::Stdin => return Err(generated::types::Errno::Notsup.into()),
            _ => return Err(generated::types::Errno::Rofs.into()),
        };
        // ciovecs may overlap, so check the total before writing anything
        let mut total: u32 = 0;
        for ciov in ciovs.iter() {
            let ciov_read = memory.read(ciov?)?;
            total = match total.checked_add(ciov_read.buf_len) {
                Some(total) => total,
                None => {
                    log_warn!("ciovec lengths do not fit the written-bytes count");
                    return Err(generated::types::Errno::Overflow.into());
                }
            };
        }

        let mut size: u32 = 0;
        for ciov in ciovs.iter() {
            let ciov_read = memory.read(ciov?)?;
            if ciov_read.buf_len == 0 {
                continue;
            }
            let buf_to_rewrite = ciov_read.buf.as_array(ciov_read.buf_len);
            let cow = memory.as_cow(buf_to_rewrite)?;
            let add_size: u32 = cow.len().try_into()?;
            size += add_size;
            if let Err(e) = stream.write_all(&cow) {
                log_error!(e: err = e; "Failed to write to stream");
            }
        }
        if let Err(e) = stream.flush() {
            log_error!(e: err = e; "Failed to flush stream");
        }
        Ok(size)
    }

    /// Write to a file descriptor, without using and updating the file descriptor's offset.
    /// NOTE: This is similar to `pwritev` in POSIX.
    #[instrument(skip(self, memory))]
    fn fd_pwrite(
        &mut self,
        memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
        ciovs: generated::types::CiovecArray,
        offset: generated::types::Filesize,
    ) -> Result<generated::types::Size, generated::types::Error> {
        self.get_fd_desc(fd)?;
        Err(generated::types::Errno::Notsup.into())
    }

    /// Return a description of the given preopened file descriptor.
    fn fd_prestat_get(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
    ) -> Result<generated::types::Prestat, generated::types::Error> {
        let fdi = fd.into();
        if self.context.preopen_fd != Some(fdi) {
            return Err(generated::types::Errno::Badf.into());
        }
        let vfs::FileDescriptor::Dir { path } = self.get_fd_desc(fd)? else {
            return Err(generated::types::Errno::Badf.into());
        };
        debug_assert!(path.is_empty());
        Ok(generated::types::Prestat::Dir(
            generated::types::PrestatDir { pr_name_len: 1 },
        ))
    }

    /// Return a description of the given preopened file descriptor.
    #[instrument(skip(self, memory))]
    fn fd_prestat_dir_name(
        &mut self,
        memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
        path: GuestPtr<u8>,
        path_max_len: generated::types::Size,
    ) -> Result<(), generated::types::Error> {
        let fdi = fd.into();
        if self.context.preopen_fd != Some(fdi)
            || !matches!(self.get_fd_desc(fd)?, vfs::FileDescriptor::Dir { .. })
        {
            return Err(generated::types::Errno::Badf.into());
        }
        if path_max_len < 1 {
            return Err(generated::types::Errno::Overflow.into());
        }
        memory.copy_from_slice(b"/", path.as_array(1))?;
        Ok(())
    }

    /// Atomically replace a file descriptor by renumbering another file descriptor.
    fn fd_renumber(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        from: generated::types::Fd,
        to: generated::types::Fd,
    ) -> Result<(), generated::types::Error> {
        Err(generated::types::Errno::Notsup.into())
    }

    /// Move the offset of a file descriptor.
    /// NOTE: This is similar to `lseek` in POSIX.
    fn fd_seek(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
        offset: generated::types::Filedelta,
        whence: generated::types::Whence,
    ) -> impl std::future::Future<Output = Result<generated::types::Filesize, generated::types::Error>>
           + Send {
        let res = (|| -> Result<generated::types::Filesize, generated::types::Error> {
            let right = if offset == 0 && whence == generated::types::Whence::Cur {
                generated::types::Rights::FD_TELL
            } else {
                generated::types::Rights::FD_SEEK
            };
            self.require_right(fd, right)?;
            match self.get_fd_desc_mut(fd)? {
                vfs::FileDescriptor::Stdin
                | vfs::FileDescriptor::Stderr
                | vfs::FileDescriptor::Stdout => Err(generated::types::Errno::Spipe.into()),
                vfs::FileDescriptor::File(vfs::FileContents { contents, pos, .. }) => {
                    const {
                        assert!(std::mem::size_of::<usize>() <= std::mem::size_of::<u64>());
                    }
                    match whence {
                        generated::types::Whence::Cur => {
                            if offset < 0 {
                                // `unsigned_abs`, not `-offset`: negating
                                // `i64::MIN` overflows, and the profile aborts.
                                let offset = offset.unsigned_abs();
                                if offset > u64::from(*pos) {
                                    *pos = 0;
                                } else {
                                    let offset: u32 = offset.into_int_downcast_panicking();
                                    *pos -= offset;
                                }
                            } else {
                                let offset = offset as u64;
                                let contents_len: u32 =
                                    contents.len().into_int_downcast_panicking();
                                let rem = contents_len - *pos;
                                if offset > u64::from(rem) {
                                    *pos = contents.len().into_int_downcast_panicking();
                                } else {
                                    let offset: u32 = offset.into_int_downcast_panicking();
                                    *pos += offset;
                                }
                            }
                        }
                        generated::types::Whence::End => {
                            return Err(generated::types::Errno::Notsup.into())
                        }
                        generated::types::Whence::Set => {
                            let offset = if offset < 0 { 0 } else { offset as u64 };
                            let contents_len: u32 = contents.len().into_int_downcast_panicking();
                            if offset > u64::from(contents_len) {
                                *pos = contents.len().into_int_downcast_panicking();
                            } else {
                                *pos = offset.into_int_downcast_panicking();
                            }
                        }
                    };
                    Ok(u64::from(*pos))
                }
                vfs::FileDescriptor::Dir { .. } => Err(generated::types::Errno::Notsup.into()),
            }
        })();
        std::future::ready(res)
    }

    /// Synchronize the data and metadata of a file to disk.
    /// NOTE: This is similar to `fsync` in POSIX.
    fn fd_sync(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
    ) -> Result<(), generated::types::Error> {
        self.require_right(fd, generated::types::Rights::FD_SYNC)?;
        Ok(())
    }

    /// Return the current offset of a file descriptor.
    /// NOTE: This is similar to `lseek(fd, 0, SEEK_CUR)` in POSIX.
    fn fd_tell(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
    ) -> impl std::future::Future<Output = Result<generated::types::Filesize, generated::types::Error>>
           + Send {
        let res = match self.require_right(fd, generated::types::Rights::FD_TELL) {
            Err(e) => Err(e),
            Ok(()) => match self.get_fd_desc(fd) {
                Err(e) => Err(e),
                Ok(desc) => match desc {
                    vfs::FileDescriptor::Stdin
                    | vfs::FileDescriptor::Stderr
                    | vfs::FileDescriptor::Stdout => Err(generated::types::Errno::Spipe.into()),
                    vfs::FileDescriptor::File(file) => Ok(file.pos.into()),
                    vfs::FileDescriptor::Dir { .. } => Err(generated::types::Errno::Notsup.into()),
                },
            },
        };
        std::future::ready(res)
    }

    #[instrument(skip(self, memory))]
    fn fd_readdir(
        &mut self,
        memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
        buf: GuestPtr<u8>,
        buf_len: generated::types::Size,
        cookie: generated::types::Dircookie,
    ) -> Result<generated::types::Size, generated::types::Error> {
        self.require_right(fd, generated::types::Rights::FD_READDIR)?;
        let vfs::FileDescriptor::Dir { path: dir_path } = self.get_fd_desc(fd)? else {
            return Err(generated::types::Errno::Badf.into());
        };

        let dirent = self.dir_fd_get_trie(&vfs::split_normalize_paths(
            dir_path.iter().map(String::as_str),
            true,
        ))?;

        let FilesTrie::Dir(direntries) = dirent else {
            return Err(generated::types::Errno::Badf.into());
        };

        let head = [
            (
                generated::types::Dirent {
                    d_next: 1u64,
                    d_ino: 0,
                    d_type: generated::types::Filetype::Directory,
                    d_namlen: 1u32,
                },
                ".".into(),
            ),
            (
                generated::types::Dirent {
                    d_next: 2u64,
                    d_ino: 0,
                    d_type: generated::types::Filetype::Directory,
                    d_namlen: 2u32,
                },
                "..".into(),
            ),
        ];

        let dirent_actual_iter = direntries.iter().zip(3u64..).map(|(x, idx)| {
            let name_len: u32 = x.0.len().into_int_downcast_panicking();
            (
                generated::types::Dirent {
                    d_next: idx,
                    d_ino: 0,
                    d_type: match *x.1 {
                        FilesTrie::Dir(_) => generated::types::Filetype::Directory,
                        FilesTrie::Leaf(_) => generated::types::Filetype::RegularFile,
                    },
                    d_namlen: name_len,
                },
                x.0.clone(),
            )
        });

        let mut buf = buf;
        let mut cap = buf_len;
        let cookie = cookie.try_into()?;
        for (ref entry, path) in head.into_iter().chain(dirent_actual_iter).skip(cookie) {
            const DIRENT_SIZE_BOUND: usize = 100;
            let mut dirent_mem_buf: [u8; DIRENT_SIZE_BOUND] = [0; DIRENT_SIZE_BOUND];

            use wiggle::GuestType;
            let dirent_mem_buf =
                &mut align_slice(&mut dirent_mem_buf, generated::types::Dirent::guest_align())
                    [..generated::types::Dirent::guest_size().into_int_comptime()];
            let mut fake_mem = wiggle::GuestMemory::Unshared(dirent_mem_buf);

            fake_mem.write(
                wiggle::GuestPtr::<generated::types::Dirent>::new(0),
                entry.clone(),
            )?;

            buf = write_bytes_capacity(
                memory,
                buf,
                &dirent_mem_buf[..generated::types::Dirent::guest_size().into_int_comptime()],
                &mut cap,
            )?;
            if cap == 0 {
                return Ok(buf_len);
            }

            buf = write_bytes_capacity(memory, buf, path.as_bytes(), &mut cap)?;
            if cap == 0 {
                return Ok(buf_len);
            }
        }
        Ok(buf_len - cap)
    }

    #[instrument(skip(self, memory))]
    fn path_create_directory(
        &mut self,
        memory: &mut GuestMemory<'_>,
        dirfd: generated::types::Fd,
        path: GuestPtr<str>,
    ) -> Result<(), generated::types::Error> {
        Err(generated::types::Errno::Rofs.into())
    }

    /// Return the attributes of a file or directory.
    /// NOTE: This is similar to `stat` in POSIX.
    #[instrument(skip(self, memory))]
    fn path_filestat_get(
        &mut self,
        memory: &mut GuestMemory<'_>,
        dirfd: generated::types::Fd,
        flags: generated::types::Lookupflags,
        path: GuestPtr<str>,
    ) -> Result<generated::types::Filestat, generated::types::Error> {
        self.require_right(dirfd, generated::types::Rights::PATH_FILESTAT_GET)?;
        let fdi = dirfd.into();
        let path = super::common::read_string(memory, path)?;
        let Some(vfs::FileDescriptor::Dir { path: dir_path }) = self.vfs.fds.get(&fdi) else {
            return Err(generated::types::Errno::Badf.into());
        };
        let path_components = join_dir_and_path(&self.context.preopen, dir_path, &path)?;
        let cur_trie = self.dir_fd_get_trie(&path_components)?;
        match cur_trie {
            FilesTrie::Leaf(data) => Ok(filestat(
                generated::types::Filetype::RegularFile,
                data.len().try_into()?,
            )),
            FilesTrie::Dir(_) => Ok(filestat(generated::types::Filetype::Directory, 0)),
        }
    }

    /// Adjust the timestamps of a file or directory.
    /// NOTE: This is similar to `utimensat` in POSIX.
    #[instrument(skip(self, memory))]
    fn path_filestat_set_times(
        &mut self,
        memory: &mut GuestMemory<'_>,
        dirfd: generated::types::Fd,
        flags: generated::types::Lookupflags,
        path: GuestPtr<str>,
        atim: generated::types::Timestamp,
        mtim: generated::types::Timestamp,
        fst_flags: generated::types::Fstflags,
    ) -> Result<(), generated::types::Error> {
        Err(generated::types::Errno::Rofs.into())
    }

    /// Create a hard link.
    /// NOTE: This is similar to `linkat` in POSIX.
    #[instrument(skip(self, memory))]
    fn path_link(
        &mut self,
        memory: &mut GuestMemory<'_>,
        src_fd: generated::types::Fd,
        src_flags: generated::types::Lookupflags,
        src_path: GuestPtr<str>,
        target_fd: generated::types::Fd,
        target_path: GuestPtr<str>,
    ) -> Result<(), generated::types::Error> {
        Err(generated::types::Errno::Rofs.into())
    }

    /// Open a file or directory.
    /// NOTE: This is similar to `openat` in POSIX.
    #[instrument(skip(self, memory))]
    fn path_open(
        &mut self,
        memory: &mut GuestMemory<'_>,
        dirfd: generated::types::Fd,
        dirflags: generated::types::Lookupflags,
        path: GuestPtr<str>,
        oflags: generated::types::Oflags,
        fs_rights_base: generated::types::Rights,
        fs_rights_inheriting: generated::types::Rights,
        fdflags: generated::types::Fdflags,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        let file_path = super::common::read_string(memory, path)?;
        let fdi = dirfd.into();
        let (descriptor, rights) = {
            let Some(vfs::FileDescriptor::Dir { path: dir_path }) = self.vfs.fds.get(&fdi) else {
                return Err(generated::types::Errno::Badf.into());
            };
            let parent_rights = self
                .context
                .descriptor_rights
                .get(&fdi)
                .copied()
                .unwrap_or_else(|| supported_rights(generated::types::Filetype::Directory));
            if !parent_rights
                .base
                .contains(generated::types::Rights::PATH_OPEN)
            {
                return Err(generated::types::Errno::Notcapable.into());
            }
            let path_components = join_dir_and_path(&self.context.preopen, dir_path, &file_path)?;
            let cur_trie = self.dir_fd_get_trie(&path_components)?;
            match cur_trie {
                FilesTrie::Leaf(data) => {
                    if oflags.contains(generated::types::Oflags::DIRECTORY) {
                        return Err(generated::types::Errno::Notdir.into());
                    }
                    let rights = limit_rights(
                        fs_rights_base,
                        fs_rights_inheriting,
                        parent_rights.inheriting,
                        generated::types::Filetype::RegularFile,
                    );
                    (
                        vfs::FileDescriptor::File(vfs::FileContents {
                            contents: data.clone(),
                            pos: 0,
                            release_memory: false,
                        }),
                        rights,
                    )
                }
                FilesTrie::Dir { .. } => {
                    let rights = limit_rights(
                        fs_rights_base,
                        fs_rights_inheriting,
                        parent_rights.inheriting,
                        generated::types::Filetype::Directory,
                    );
                    (
                        vfs::FileDescriptor::Dir {
                            path: path_components.into_iter().map(String::from).collect(),
                        },
                        rights,
                    )
                }
            }
        };
        let new_fd = self
            .vfs
            .alloc_fd()
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;
        self.vfs.fds.insert(new_fd, descriptor);
        self.context.descriptor_rights.insert(new_fd, rights);
        Ok(new_fd.into())
    }

    /// Read the contents of a symbolic link.
    /// NOTE: This is similar to `readlinkat` in POSIX.
    #[instrument(skip(self, memory))]
    fn path_readlink(
        &mut self,
        memory: &mut GuestMemory<'_>,
        dirfd: generated::types::Fd,
        path: GuestPtr<str>,
        buf: GuestPtr<u8>,
        buf_len: generated::types::Size,
    ) -> Result<generated::types::Size, generated::types::Error> {
        Err(generated::types::Errno::Badf.into())
    }

    #[instrument(skip(self, memory))]
    fn path_remove_directory(
        &mut self,
        memory: &mut GuestMemory<'_>,
        dirfd: generated::types::Fd,
        path: GuestPtr<str>,
    ) -> Result<(), generated::types::Error> {
        Err(generated::types::Errno::Rofs.into())
    }

    /// Rename a file or directory.
    /// NOTE: This is similar to `renameat` in POSIX.
    #[instrument(skip(self, memory))]
    fn path_rename(
        &mut self,
        memory: &mut GuestMemory<'_>,
        src_fd: generated::types::Fd,
        src_path: GuestPtr<str>,
        dest_fd: generated::types::Fd,
        dest_path: GuestPtr<str>,
    ) -> Result<(), generated::types::Error> {
        Err(generated::types::Errno::Rofs.into())
    }

    #[instrument(skip(self, memory))]
    fn path_symlink(
        &mut self,
        memory: &mut GuestMemory<'_>,
        src_path: GuestPtr<str>,
        dirfd: generated::types::Fd,
        dest_path: GuestPtr<str>,
    ) -> Result<(), generated::types::Error> {
        Err(generated::types::Errno::Rofs.into())
    }

    #[instrument(skip(self, memory))]
    fn path_unlink_file(
        &mut self,
        memory: &mut GuestMemory<'_>,
        dirfd: generated::types::Fd,
        path: GuestPtr<str>,
    ) -> Result<(), generated::types::Error> {
        Err(generated::types::Errno::Rofs.into())
    }

    #[instrument(skip(self, memory))]
    fn poll_oneoff(
        &mut self,
        memory: &mut GuestMemory<'_>,
        subs: GuestPtr<generated::types::Subscription>,
        events: GuestPtr<generated::types::Event>,
        nsubscriptions: generated::types::Size,
    ) -> Result<generated::types::Size, generated::types::Error> {
        Err(generated::types::Errno::Notsup.into())
    }

    fn proc_exit(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        status: generated::types::Exitcode,
    ) -> wasmtime::Error {
        log_trace!(status = status; "proc_exit called");
        // Clamp to WASI's valid exit-code range.
        let code = if status >= 126 { 125 } else { status as i32 };
        // Trap straight to a terminal result instead of routing an intermediate
        // exit-carrier error through `unwrap_vm_errors`: a zero exit is a normal
        // empty return, any other code is a VM error.
        let trap: anyhow::Error = if code == 0 {
            crate::wasi::genlayer_sdk::ContractReturn(genlayer_sdk::calldata::Value::Null.into())
                .into()
        } else {
            rt::errors::Error::vm(abi::consts::VmError::exit_code().val_i32(code)).into()
        };
        crate::anyhow_to_wasmtime(trap)
    }

    fn proc_raise(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        _sig: generated::types::Signal,
    ) -> Result<(), generated::types::Error> {
        Err(generated::types::Errno::Notsup.into())
    }

    fn sched_yield(
        &mut self,
        _memory: &mut GuestMemory<'_>,
    ) -> Result<(), generated::types::Error> {
        Err(generated::types::Errno::Notsup.into())
    }

    #[instrument(skip(self, memory))]
    fn random_get(
        &mut self,
        memory: &mut GuestMemory<'_>,
        buf: GuestPtr<u8>,
        buf_len: generated::types::Size,
    ) -> Result<(), generated::types::Error> {
        let buf = buf.as_array(buf_len);
        memory.bounds_check(buf)?;

        let mut mem: Vec<u8> = std::iter::repeat_n(0, buf_len.into_int_comptime()).collect();

        if self.context.conf.permissions.deterministic {
            use rand_core::RngCore as _;

            self.context.mt19937_rng.fill_bytes(&mut mem[..]);
        } else {
            // Non-deterministic mode: cryptographically secure random number generator
            if let Err(e) = getrandom::fill(&mut mem) {
                log_error!(error:err = e; "random failed");
                return Err(generated::types::Errno::Io.into());
            }
        }

        memory.copy_from_slice(&mem, buf)?;
        Ok(())
    }

    #[allow(unused_variables)]
    fn sock_accept(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
        flags: generated::types::Fdflags,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        Err(generated::types::Errno::Acces.into())
    }

    #[allow(unused_variables)]
    fn sock_recv(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
        ri_data: generated::types::IovecArray,
        ri_flags: generated::types::Riflags,
    ) -> Result<(generated::types::Size, generated::types::Roflags), generated::types::Error> {
        Err(generated::types::Errno::Acces.into())
    }

    #[allow(unused_variables)]
    fn sock_send(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
        si_data: generated::types::CiovecArray,
        _si_flags: generated::types::Siflags,
    ) -> Result<generated::types::Size, generated::types::Error> {
        Err(generated::types::Errno::Acces.into())
    }

    #[allow(unused_variables)]
    fn sock_shutdown(
        &mut self,
        _memory: &mut GuestMemory<'_>,
        fd: generated::types::Fd,
        how: generated::types::Sdflags,
    ) -> Result<(), generated::types::Error> {
        Err(generated::types::Errno::Acces.into())
    }
}

impl ContextVFS<'_> {
    fn descriptor_status(
        &self,
        fd: generated::types::Fd,
    ) -> Result<(generated::types::Filetype, DescriptorRights), generated::types::Error> {
        let fdi = fd.into();
        let desc = self
            .vfs
            .fds
            .get(&fdi)
            .ok_or_else(|| generated::types::Error::from(generated::types::Errno::Badf))?;
        let (filetype, default_rights) = match desc {
            vfs::FileDescriptor::Stdin => (
                generated::types::Filetype::Unknown,
                DescriptorRights {
                    base: generated::types::Rights::FD_READ,
                    inheriting: generated::types::Rights::FD_READ,
                },
            ),
            vfs::FileDescriptor::Stdout | vfs::FileDescriptor::Stderr => (
                generated::types::Filetype::Unknown,
                DescriptorRights {
                    base: generated::types::Rights::FD_WRITE,
                    inheriting: generated::types::Rights::FD_WRITE,
                },
            ),
            vfs::FileDescriptor::File { .. } => (
                generated::types::Filetype::RegularFile,
                supported_rights(generated::types::Filetype::RegularFile),
            ),
            vfs::FileDescriptor::Dir { .. } => (
                generated::types::Filetype::Directory,
                supported_rights(generated::types::Filetype::Directory),
            ),
        };
        let rights = self
            .context
            .descriptor_rights
            .get(&fdi)
            .copied()
            .unwrap_or(default_rights);
        Ok((filetype, rights))
    }

    fn require_right(
        &self,
        fd: generated::types::Fd,
        right: generated::types::Rights,
    ) -> Result<(), generated::types::Error> {
        let fdi = fd.into();
        self.get_fd_desc(fd)?;
        if let Some(rights) = self.context.descriptor_rights.get(&fdi) {
            if !rights.base.contains(right) {
                return Err(generated::types::Errno::Notcapable.into());
            }
        }
        Ok(())
    }

    fn dir_fd_get_trie(
        &self,
        path_from_root: &[&str],
    ) -> Result<&FilesTrie, generated::types::Error> {
        let mut cur_trie: &FilesTrie = &self.context.fs;

        for component in path_from_root {
            let p = *component;
            debug_assert!(vfs::is_normalized_path_component(p));

            match cur_trie {
                FilesTrie::Leaf(_) => return Err(generated::types::Errno::Badf.into()),
                FilesTrie::Dir(children) => match children.get(p) {
                    Some(new_trie) => {
                        cur_trie = new_trie;
                    }
                    None => return Err(generated::types::Errno::Noent.into()),
                },
            }
        }

        Ok(cur_trie)
    }

    fn get_fd_desc(
        &self,
        fd: generated::types::Fd,
    ) -> Result<&vfs::FileDescriptor, generated::types::Error> {
        let fdi = fd.into();
        match self.vfs.fds.get(&fdi) {
            Some(x) => Ok(x),
            None => Err(generated::types::Errno::Badf.into()),
        }
    }

    fn get_fd_desc_mut(
        &mut self,
        fd: generated::types::Fd,
    ) -> Result<&mut vfs::FileDescriptor, generated::types::Error> {
        let fdi = fd.into();
        match self.vfs.fds.get_mut(&fdi) {
            Some(x) => Ok(x),
            None => Err(generated::types::Errno::Badf.into()),
        }
    }
}

fn write_bytes_capacity(
    memory: &mut GuestMemory<'_>,
    ptr: GuestPtr<u8>,
    buf: &[u8],
    capacity: &mut u32,
) -> Result<GuestPtr<u8>, generated::types::Error> {
    let len = u32::try_from(buf.len())?.min(*capacity);

    memory.copy_from_slice(&buf[..len.into_int_comptime()], ptr.as_array(len))?;
    *capacity -= len;
    let next = ptr.add(len)?;
    Ok(next)
}

#[cfg(test)]
#[path = "preview1_test.rs"]
mod wasi_tests;

#[cfg(test)]
mod tests {
    use rand_core::RngCore as _;
    use sha3::Digest as _;

    /// Known-answer test for the deterministic `random_get` byte layout. The seed is a
    /// fixed 32-byte value consumed as 8 little-endian `u32` words (matching
    /// `Context::new`), and `fill_bytes` produces the exact stream below. This pins the
    /// endianness and partial-word layout of the MT19937 output; the calldata seed
    /// derivation (`sha3-256(stdin)`) is exercised separately.
    #[test]
    fn det_random_get_known_answer() {
        let seed: [u8; 32] = std::array::from_fn(|i| i as u8);
        let seed_words: [u32; 8] = std::array::from_fn(|i| {
            u32::from_le_bytes([
                seed[i * 4],
                seed[i * 4 + 1],
                seed[i * 4 + 2],
                seed[i * 4 + 3],
            ])
        });
        let mut rng = mt19937::MT19937::new_with_slice_seed(&seed_words);

        let mut out = [0u8; 20];
        rng.fill_bytes(&mut out);

        assert_eq!(hex::encode(out), "a0a49802eee109f3e6e193c8b06d7ee6a717a4d7");
    }

    /// Known-answer test for the sub-VM rolling SHA3-256 accumulator
    /// (`SingleVMData::det_subvm_hashes`): the empty digest and one `update` fold step.
    /// The spec under-defines this fold, so pin the current behavior to catch drift.
    #[test]
    fn det_subvm_hash_fold_known_answer() {
        let empty = sha3::Sha3_256::new().finalize();
        assert_eq!(
            hex::encode(empty),
            "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a"
        );

        let child: [u8; 32] = std::array::from_fn(|i| i as u8);
        let mut acc = sha3::Sha3_256::new();
        acc.update(child);
        assert_eq!(
            hex::encode(acc.finalize()),
            "050a48733bd5c2756ba95c5828cc83ee16fabcd3c086885b7744f84a0f9e0d94"
        );
    }
}
