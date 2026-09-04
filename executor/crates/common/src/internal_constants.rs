// This file is auto-generated. Do not edit!

#![allow(dead_code, clippy::all)]

pub mod memory_limiter_consts {
    pub const TABLE_ENTRY: u32 = 64;
    pub const FILE_MAPPING: u32 = 256;
    pub const FD_ALLOCATION: u32 = 96;
    pub const RUNNER_LOAD_COST: u32 = 4096;
    pub const VM_SPAWN_COST: u32 = 134217728;
    pub const NEW_STORAGE_PAGE: u32 = 256;
    pub const STORAGE_PAGE_INHERITED: u32 = 128;
    pub const EXECUTION_EMISSION_BASE_SIZE: u32 = 256;
    pub const MESSAGE_FEE_ROTATION_ELEMENT_SIZE: u32 = 32;
    pub const NONDET_OUTPUT_BASE_SIZE: u32 = 32;
}

pub mod top_limits {
    pub const NONDET_BLOCKS: u32 = 4096;
    pub const LOCKED_SLOTS: u32 = 256;
    pub const UPGRADERS: u32 = 32;
    pub const VM_RECURSION: u32 = 512;
    pub const WEB_REQUEST_MIN_SPACE: u32 = 65536;
    pub const WEB_RENDER_MIN_SPACE: u32 = 134217728;
    pub const MAX_FDS: u32 = 1024;
    pub const WASM_CALL_DEPTH: u32 = 1024;
    pub const WASM_STACK_VALUE_SLOTS: u32 = 65535;
    pub const VFS_PATH_COMPONENTS: u32 = 128;
}

pub mod runner_limits {
    pub const ENV_NAME_LEN: u32 = 128;
    pub const ENV_VALUE_LEN: u32 = 1048576;
    pub const INIT_ACTION_DEPTH: u32 = 128;
}

// EOF
