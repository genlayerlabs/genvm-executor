use std::collections::BTreeMap;

use genlayer_sdk::abi;
use genvm_common::internal_constants::{memory_limiter_consts, top_limits};
use genvm_common::log_trace;

use crate::int_traits::*;
use crate::rt;

pub enum Trie<T> {
    Dir(BTreeMap<String, Trie<T>>),
    Leaf(T),
}

pub enum TrieFollowResult<'a, T> {
    Found(&'a Trie<T>),
    NotDir,
    NotFound,
}

impl<T> Trie<T> {
    pub fn follow<'b>(
        &self,
        components: impl std::iter::Iterator<Item = &'b str>,
    ) -> TrieFollowResult<'_, T> {
        let mut cur_trie = self;

        for component in components {
            debug_assert!(is_normalized_path_component(component));

            match cur_trie {
                Trie::Dir(map) => {
                    cur_trie = match map.get(component) {
                        Some(next) => next,
                        None => return TrieFollowResult::NotFound,
                    };
                }
                Trie::Leaf(_) => return TrieFollowResult::NotDir,
            }
        }
        TrieFollowResult::Found(cur_trie)
    }
}

pub fn is_normalized_path_component(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && !s.contains('/')
}

pub fn split_normalize_paths<'a>(
    path_components: impl std::iter::Iterator<Item = &'a str>,
    absolute: bool,
) -> Vec<&'a str> {
    let mut result: Vec<&str> = Vec::new();

    enum Act {
        Pop,
        Push,
        Skip,
    }

    for c in path_components {
        debug_assert!(!c.contains('/'));

        match c {
            "" => continue,
            "." => continue,
            ".." => {
                let pop_last = match result.last().map(std::ops::Deref::deref) {
                    None => {
                        if absolute {
                            Act::Skip
                        } else {
                            Act::Push
                        }
                    }
                    Some("..") => Act::Push,
                    Some(_) => Act::Pop,
                };
                match pop_last {
                    Act::Pop => {
                        result.pop();
                    }
                    Act::Push => {
                        result.push("..");
                    }
                    Act::Skip => {}
                }
            }
            other => result.push(other),
        }
    }

    result
}

pub fn split_normalize_path(path: &str, absolute: bool) -> Vec<&str> {
    split_normalize_paths(path.split('/'), absolute)
}

pub struct FileContents {
    pub contents: bytes::Bytes,
    pub pos: u32,

    pub release_memory: bool,
}

impl From<bytes::Bytes> for FileContents {
    fn from(value: bytes::Bytes) -> Self {
        Self {
            contents: value,
            pos: 0,
            release_memory: true,
        }
    }
}

pub enum FileDescriptor {
    Stdin,
    Stdout,
    Stderr,
    File(FileContents),
    Dir { path: Vec<String> },
}

#[allow(dead_code)]
const _: FileDescriptor = FileDescriptor::Stdin;

pub(crate) struct VFS {
    pub fds: BTreeMap<Fd, FileDescriptor>,
    pub free_descriptors: Vec<Fd>,
    pub next_free_descriptor: Fd,

    pub limiter: rt::memlimiter::Limiter,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fd(u32);

pub const ROOT_PREOPEN_FD: Fd = Fd(3);

impl From<Fd> for u32 {
    fn from(value: Fd) -> Self {
        value.0
    }
}

impl Fd {
    pub fn new(fd: u32) -> Self {
        Self(fd)
    }

    pub fn as_u32(&self) -> u32 {
        self.0
    }
}

impl VFS {
    pub fn new(stdin: Vec<u8>, limiter: rt::memlimiter::Limiter) -> rt::errors::Result<Self> {
        log_trace!(stdin:bytes = stdin; "creating VFS");

        if !limiter.consume_size(stdin.len()) {
            return Err(rt::errors::Error::vm(
                abi::consts::VmError::out_of().memory().val(),
            ));
        }

        let stdin_data = bytes::Bytes::from(stdin);

        let fds = BTreeMap::from([
            (
                Fd::new(0),
                FileDescriptor::File(FileContents {
                    contents: stdin_data,
                    pos: 0,
                    release_memory: true,
                }),
            ),
            (Fd::new(1), FileDescriptor::Stdout),
            (Fd::new(2), FileDescriptor::Stderr),
            (ROOT_PREOPEN_FD, FileDescriptor::Dir { path: Vec::new() }),
        ]);
        let next_free_descriptor = fds.last_key_value().map(|x| *x.0).unwrap_or(Fd::default());
        Ok(Self {
            fds,
            next_free_descriptor,
            free_descriptors: Vec::new(),
            limiter,
        })
    }

    /// gives vacant fd
    pub fn alloc_fd(&mut self) -> anyhow::Result<Fd> {
        if self.fds.len() >= top_limits::MAX_FDS.into_int_comptime() {
            return Err(rt::errors::Error::vm(abi::consts::VmError::out_of().fds()).into());
        }
        match self.free_descriptors.pop() {
            Some(v) => Ok(v),
            None => {
                if !self.limiter.consume(memory_limiter_consts::FD_ALLOCATION) {
                    return Err(rt::errors::Error::vm(
                        abi::consts::VmError::out_of().memory().val(),
                    )
                    .into());
                }
                self.next_free_descriptor.0 += 1;
                Ok(self.next_free_descriptor)
            }
        }
    }

    /// it must be removed from fds beforehand
    pub fn free_fd(&mut self, fd: Fd) {
        self.free_descriptors.push(fd);
    }

    pub fn pop_fd(&mut self, fd: Fd) -> Option<FileDescriptor> {
        match self.fds.remove(&fd) {
            Some(v) => {
                if let FileDescriptor::File(v) = &v {
                    if v.release_memory {
                        self.limiter
                            .release(v.contents.len().into_int_downcast_panicking());
                    }
                }

                self.free_fd(fd);

                Some(v)
            }
            None => None,
        }
    }

    pub fn place_content(&mut self, value: FileContents) -> anyhow::Result<Fd> {
        if value.release_memory && !self.limiter.consume_size(value.contents.len()) {
            return Err(
                rt::errors::Error::vm(abi::consts::VmError::out_of().memory().val()).into(),
            );
        }

        let fd = match self.alloc_fd() {
            Ok(fd) => fd,
            Err(e) => {
                if value.release_memory {
                    self.limiter
                        .release(value.contents.len().into_int_downcast_panicking());
                }
                return Err(e);
            }
        };
        self.fds.insert(fd, FileDescriptor::File(value));
        Ok(fd)
    }
}
