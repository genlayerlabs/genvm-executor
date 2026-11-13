//! Stands in for `directories-next` under wasmtime's cache: the executor always
//! sets the cache directory, so no platform default is ever resolved.

use std::convert::Infallible;
use std::path::Path;

pub struct ProjectDirs(Infallible);

impl ProjectDirs {
    pub fn from(_qualifier: &str, _organization: &str, _application: &str) -> Option<Self> {
        None
    }

    pub fn config_dir(&self) -> &Path {
        match self.0 {}
    }

    pub fn cache_dir(&self) -> &Path {
        match self.0 {}
    }
}
