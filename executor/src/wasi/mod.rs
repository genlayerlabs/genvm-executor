use crate::{calldata, rt};

use ::genlayer_sdk as original_genlayer_sdk;

pub mod base;
pub mod genlayer_sdk;
pub mod preview1;
pub mod vfs;

mod common;

pub struct Context {
    vfs: vfs::VFS,
    pub preview1: preview1::Context,
    pub genlayer_sdk: genlayer_sdk::Context,
}

impl Context {
    pub fn new(
        mut data: Box<genlayer_sdk::SingleVMData>,
        limiter: rt::memlimiter::Limiter,
    ) -> std::result::Result<Self, (rt::errors::Error, Box<genlayer_sdk::SingleVMData>)> {
        let msg_data: original_genlayer_sdk::abi::entry::ExtendedMessageFlat =
            data.message_data.into();
        let as_bytes = calldata::encode_obj(&msg_data);
        data.message_data = msg_data.into();

        // The deterministic RNG seed is the sha3-256 of the VM's stdin (the encoded
        // message data), so it is fully determined by the VM inputs at construction.
        let seed: [u8; 32] = {
            use sha3::Digest as _;
            sha3::Sha3_256::digest(&as_bytes).into()
        };

        let vfs = match vfs::VFS::new(as_bytes, limiter.clone()) {
            Ok(vfs) => vfs,
            Err(e) => return Err((e, data)),
        };
        Ok(Self {
            vfs,
            preview1: preview1::Context::new(
                data.message_data.message.datetime,
                data.conf.clone(),
                seed,
            ),
            genlayer_sdk: genlayer_sdk::Context::new(data, limiter),
        })
    }
}

pub(super) fn add_to_linker_sync<T: Send + 'static>(
    linker: &mut wasmtime::Linker<T>,
    f: impl Fn(&mut T) -> &mut Context + Copy + Send + Sync + 'static,
) -> anyhow::Result<()> {
    #[derive(Clone, Copy)]
    struct Fwd<F>(F);

    impl<T, F> preview1::AddToLinkerFn<T> for Fwd<F>
    where
        F: Fn(&mut T) -> &mut Context + Copy + Send + Sync + 'static,
    {
        fn call<'a>(&self, arg: &'a mut T) -> preview1::ContextVFS<'a> {
            let r = self.0(arg);
            preview1::ContextVFS {
                vfs: &mut r.vfs,
                context: &mut r.preview1,
            }
        }
    }

    impl<T, F> genlayer_sdk::AddToLinkerFn<T> for Fwd<F>
    where
        F: Fn(&mut T) -> &mut Context + Copy + Send + Sync + 'static,
    {
        fn call<'a>(&self, arg: &'a mut T) -> genlayer_sdk::ContextVFS<'a> {
            let r = self.0(arg);
            genlayer_sdk::ContextVFS {
                vfs: &mut r.vfs,
                preview1: &mut r.preview1,
                context: &mut r.genlayer_sdk,
            }
        }
    }

    preview1::add_to_linker_sync(linker, Fwd(f))?;
    genlayer_sdk::add_to_linker_sync(linker, Fwd(f))?;

    Ok(())
}
