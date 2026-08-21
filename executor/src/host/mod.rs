pub use genvm_common::host_fns;
pub mod message;

use genlayer_sdk::abi;
use genvm_common::internal_constants::top_limits;
use genvm_common::*;

use crate::public_abi::root_offsets;
use crate::public_abi::StorageType;
use genlayer_sdk::calldata::Address;
use genlayer_sdk::calldata::ADDRESS_SIZE;

use core::str;
use std::os::fd::FromRawFd;

use anyhow::{Context, Result};

use crate::int_traits::*;
use crate::{calldata, domain, rt};
pub use message::SlotID;

pub trait Sock: std::io::Read + std::io::Write + Send + Sync {}

impl Sock for bufreaderwriter::seq::BufReaderWriterSeq<std::os::unix::net::UnixStream> {}

impl Sock for bufreaderwriter::seq::BufReaderWriterSeq<std::net::TcpStream> {}

pub struct Host {
    sock: Box<dyn Sock>,
    metrics: sync::DArc<Metrics>,
}

#[derive(Default, serde::Serialize, Debug, genlayer_calldata::Encode)]
pub struct Metrics {
    pub time: stats::metric::Time,
}

impl Host {
    pub fn new(sock: Box<dyn Sock>, metrics: sync::DArc<Metrics>) -> Host {
        Self { sock, metrics }
    }
    pub fn connect(addr: &str, metrics: sync::DArc<Metrics>, hello_data: &[u8]) -> Result<Host> {
        const UNIX: &str = "unix://";
        let mut sock: Box<dyn Sock> = if let Some(addr_suff) = addr.strip_prefix(UNIX) {
            Box::new(bufreaderwriter::seq::BufReaderWriterSeq::new_writer(
                std::os::unix::net::UnixStream::connect(std::path::Path::new(addr_suff))
                    .with_context(|| format!("connecting to {addr}"))?,
            ))
        } else if let Some(fd_str) = addr.strip_prefix("fd://") {
            let fd: i32 = fd_str
                .parse()
                .with_context(|| format!("parsing fd number from '{fd_str}'"))?;
            let stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) };
            Box::new(bufreaderwriter::seq::BufReaderWriterSeq::new_writer(stream))
        } else {
            Box::new(bufreaderwriter::seq::BufReaderWriterSeq::new_writer(
                std::net::TcpStream::connect(addr)
                    .with_context(|| format!("connecting to {addr}"))?,
            ))
        };
        if !hello_data.is_empty() {
            sock.write_all(hello_data)
                .with_context(|| format!("writing host hello to {addr}"))?;
            sock.flush()
                .with_context(|| format!("flushing host hello to {addr}"))?;
        }
        Ok(Host { sock, metrics })
    }
}

fn read_u32(sock: &mut dyn Sock, context: &str) -> Result<u32> {
    let mut int_buf = [0; 4];
    sock.read_exact(&mut int_buf)
        .with_context(|| format!("reading u32 from host: {context}"))?;
    Ok(u32::from_le_bytes(int_buf))
}

fn read_exact_bytes(sock: &mut dyn Sock, len: usize, context: &str) -> Result<Vec<u8>> {
    use std::io::Read;

    let mut res = Vec::with_capacity(len);
    let mut taken = std::io::Read::take(&mut *sock, len.into_int_comptime());
    let read = taken
        .read_to_end(&mut res)
        .with_context(|| format!("reading {len} bytes from host: {context}"))?;

    anyhow::ensure!(read == len, "unexpected EOF reading {context}");
    Ok(res)
}

fn read_bytes(sock: &mut dyn Sock, context: &str) -> Result<Vec<u8>> {
    let len = read_u32(sock, context)?;

    read_exact_bytes(sock, len.into_int_comptime(), context)
}

fn read_or_discard_bytes(
    sock: &mut dyn Sock,
    context: &str,
    limit: u32,
) -> Result<Option<Box<[u8]>>> {
    let len = read_u32(sock, context)?;

    if len > limit {
        const DISCARD_BUF_SIZE: usize = 4096;

        let mut buf = vec![0u8; DISCARD_BUF_SIZE];
        let mut remaining = u32_into_usize(len);

        while remaining != 0 {
            let step = remaining.min(buf.len());

            sock.read_exact(&mut buf[..step])
                .with_context(|| format!("discarding {len} bytes from host: {context}"))?;

            remaining -= step;
        }

        return Ok(None);
    }

    let mut res = vec![0u8; len.into_int_comptime()].into_boxed_slice();

    sock.read_exact(&mut res)
        .with_context(|| format!("reading {len} bytes from host: {context}"))?;

    Ok(Some(res))
}

fn write_slice(sock: &mut dyn Sock, data: &[u8]) -> Result<()> {
    let len: u32 = data.len().into_int_downcast_panicking();

    sock.write_all(&len.to_le_bytes())?;
    sock.write_all(data)?;

    Ok(())
}

fn read_host_error(sock: &mut dyn Sock, context: &str) -> Result<host_fns::Errors> {
    let mut has_some = [0; 1];
    sock.read_exact(&mut has_some)
        .with_context(|| format!("reading host error code: {context}"))?;

    host_fns::Errors::try_from(has_some[0])
        .map_err(|_| anyhow::anyhow!("invalid host error code {} for: {}", has_some[0], context))
}

fn handle_host_error(sock: &mut dyn Sock, context: &str) -> Result<()> {
    let e = read_host_error(sock, context)?;

    match e {
        host_fns::Errors::Ok => Ok(()),
        host_fns::Errors::EvmReverted => {
            Err(rt::errors::Error::vm(abi::consts::VmError::evm().reverted()).into())
        }
        // Reserved for gen_call-class host methods (e.g. eth_call) refused in
        // the current execution context; it must not occur during on-chain
        // consensus execution.
        host_fns::Errors::Forbidden => {
            Err(rt::errors::Error::vm(abi::consts::VmError::forbidden()).into())
        }
    }
}

fn encode_body(buf: &mut Vec<u8>, res: &FullResult) {
    match calldata::codec::Encode::encode(res, &mut calldata::Encoder::new(buf)) {
        Ok(()) => {}
        Err(e) => match e {},
    }
}

pub fn encode_result(res: &Result<FullResult>) -> Result<Vec<u8>> {
    match res {
        Ok(d) => {
            let mut encoded = Vec::from([d.reported.kind as u8]);
            encode_body(&mut encoded, d);
            Ok(encoded)
        }
        Err(e) => {
            let mut encoded = Vec::from([host_fns::ResultCode::InternalError as u8]);
            let fake_res = FullResult::new_internal_error(format!("{e:?}"));
            encode_body(&mut encoded, &fake_res);
            Ok(encoded)
        }
    }
}

pub fn write_result_to_sock(sock: &mut dyn Sock, res: &Result<FullResult>) -> Result<()> {
    let data = encode_result(res)?;
    write_slice(sock, &data)?;
    sock.flush()?;
    Ok(())
}

pub struct LockedSlotsSet(Box<[SlotID]>);

impl LockedSlotsSet {
    pub fn empty() -> Self {
        Self(Box::new([]))
    }

    pub fn contains(&self, slot: SlotID) -> bool {
        self.0.binary_search(&slot).is_ok()
    }
}

pub fn all_useful_work_done() {
    std::process::exit(0);
}

#[derive(Debug, Clone)]
pub struct FullResult {
    pub reported: genvm_modules_interfaces::ReportedResult,
    pub recorded_actions: Vec<rt::supervisor::RecordedAction>,
}

impl std::ops::Deref for FullResult {
    type Target = genvm_modules_interfaces::ReportedResult;

    fn deref(&self) -> &Self::Target {
        &self.reported
    }
}

impl<W: calldata::Writer> calldata::codec::Encode<W> for FullResult {
    type Error = W::Error;

    fn encode(&self, enc: &mut calldata::Encoder<W>) -> std::result::Result<(), Self::Error> {
        calldata::codec::Encode::encode(&self.reported, enc)
    }
}

impl FullResult {
    pub fn new_internal_error(msg: String) -> Self {
        Self {
            reported: genvm_modules_interfaces::ReportedResult {
                execution_hash: bytes::Bytes::new(),
                small_hash: bytes::Bytes::new(),
                kind: genvm_modules_interfaces::ResultCode::InternalError,
                data: calldata::Value::Str(msg).into(),
                backtrace: None,
                wasm_store_hashes: genvm_modules_interfaces::WasmStoreHashes::default(),
                storage_changes: Vec::new(),
                emissions: Vec::new(),
                nondet_disagreement: None,
                nondet_results: Vec::new(),
                data_fees_remaining: Vec::new(),
                data_fees_consumed: genvm_modules_interfaces::BucketsConsumed::default(),
                llm_consumption: primitive_types::U256::zero(),
            },
            recorded_actions: Vec::new(),
        }
    }
}

impl FullResult {
    pub fn new(
        rt_result: rt::vm::FullResult,
        nondet_results: Vec<bytes::Bytes>,
        nondet_disagreement: Option<u32>,
        data_fees_remaining: Vec<primitive_types::U256>,
        data_fees_consumed: rt::fees::BucketsConsumed,
        llm_consumption: primitive_types::U256,
        recorded_actions: Vec<rt::supervisor::RecordedAction>,
    ) -> Self {
        struct Hashable<'a> {
            backtrace: &'a Option<rt::errors::Backtrace>,
            data: &'a calldata::unparsed::Maybe<calldata::Value>,
            data_fees_consumed: &'a rt::fees::BucketsConsumed,
            data_fees_remaining: &'a Vec<primitive_types::U256>,
            emissions: &'a Vec<domain::ExecutionEmission>,
            kind: &'a host_fns::ResultCode,
            wasm_store_hashes: &'a rt::errors::WasmStoreHashes,
            storage_changes: &'a Vec<rt::vm::storage::Delta>,
            subvm_hashes: &'a bytes::Bytes,
        }

        impl<W: calldata::Writer> calldata::codec::Encode<W> for Hashable<'_> {
            type Error = W::Error;

            fn encode(&self, enc: &mut calldata::Encoder<W>) -> Result<(), Self::Error> {
                enc.start_map(9)?;

                enc.push_map_k("backtrace")?;
                calldata::codec::Encode::encode(self.backtrace, enc)?;

                enc.push_map_k("data")?;
                calldata::codec::Encode::encode(self.data, enc)?;

                enc.push_map_k("data_fees_consumed")?;
                calldata::codec::Encode::encode(self.data_fees_consumed, enc)?;

                enc.push_map_k("data_fees_remaining")?;
                calldata::codec::Encode::encode(self.data_fees_remaining, enc)?;

                enc.push_map_k("emissions")?;
                calldata::codec::Encode::encode(self.emissions, enc)?;

                enc.push_map_k("kind")?;
                calldata::codec::Encode::encode(self.kind, enc)?;

                enc.push_map_k("storage_changes")?;
                calldata::codec::Encode::encode(self.storage_changes, enc)?;

                enc.push_map_k("subvm_hashes")?;
                enc.push_bytes(self.subvm_hashes)?;

                enc.push_map_k("wasm_store_hashes")?;
                calldata::codec::Encode::encode(self.wasm_store_hashes, enc)?;

                Ok(())
            }
        }

        // Fatality is a control-flow property, not part of the outcome: a caller
        // may not catch it, but the value it names is the same `vm_error` the
        // host reports and `small_hash` folds. Hashing it apart would make one
        // outcome hash two ways.
        let hashed_kind = match rt_result.kind {
            host_fns::ResultCode::FatalVmError => host_fns::ResultCode::VmError,
            kind => kind,
        };

        let hashable = Hashable {
            kind: &hashed_kind,
            data: &rt_result.data,
            backtrace: &rt_result.backtrace,
            wasm_store_hashes: &rt_result.wasm_store_hashes,
            storage_changes: &rt_result.storage_changes,
            subvm_hashes: &rt_result.subvm_hashes,
            data_fees_remaining: &data_fees_remaining,
            data_fees_consumed: &data_fees_consumed,
            emissions: &rt_result.emissions,
        };

        let mut hasher = rt::vm::Sha3Writer(sha3::Digest::new());
        let mut enc = calldata::Encoder::new(&mut hasher);
        match calldata::codec::Encode::encode(&hashable, &mut enc) {
            Ok(()) => {}
            Err(e) => match e {},
        }
        let execution_hash = bytes::Bytes::from(sha3::Digest::finalize(hasher.0).to_vec());

        // The route-invariant half of the same outcome. A caller in another
        // executor folds this, exactly as an in-process caller folds
        // `RunResult::small_hash`, so the two routes agree.
        let small_hash = bytes::Bytes::from(
            genvm_modules_interfaces::small_hash(
                convert_result_code(rt_result.kind),
                &rt_result.data,
                &rt_result.subvm_hashes,
                &rt_result.wasm_store_hashes,
            )
            .to_vec(),
        );

        Self {
            reported: genvm_modules_interfaces::ReportedResult {
                execution_hash,
                small_hash,

                data: rt_result.data,
                backtrace: rt_result.backtrace.map(convert_backtrace),
                wasm_store_hashes: convert_wasm_store_hashes(rt_result.wasm_store_hashes),
                kind: convert_result_code(rt_result.kind),
                storage_changes: rt_result
                    .storage_changes
                    .iter()
                    .map(convert_storage_delta)
                    .collect(),
                emissions: rt_result
                    .emissions
                    .into_iter()
                    .map(convert_emission)
                    .collect(),
                nondet_results,
                nondet_disagreement,
                data_fees_remaining,
                data_fees_consumed: convert_buckets_consumed(data_fees_consumed),
                llm_consumption,
            },
            recorded_actions,
        }
    }
}

fn convert_result_code(code: host_fns::ResultCode) -> genvm_modules_interfaces::ResultCode {
    match code {
        host_fns::ResultCode::Return => genvm_modules_interfaces::ResultCode::Return,
        host_fns::ResultCode::UserError => genvm_modules_interfaces::ResultCode::UserError,
        host_fns::ResultCode::VmError => genvm_modules_interfaces::ResultCode::VmError,
        host_fns::ResultCode::FatalVmError => genvm_modules_interfaces::ResultCode::FatalVmError,
        host_fns::ResultCode::InternalError => genvm_modules_interfaces::ResultCode::InternalError,
    }
}

fn convert_backtrace(backtrace: rt::errors::Backtrace) -> genvm_modules_interfaces::Backtrace {
    genvm_modules_interfaces::Backtrace {
        frames: backtrace
            .frames
            .into_iter()
            .map(|frame| genvm_modules_interfaces::Frame {
                module_name: frame.module_name,
                func: frame.func,
            })
            .collect(),
    }
}

fn convert_wasm_store_hashes(
    hashes: rt::errors::WasmStoreHashes,
) -> genvm_modules_interfaces::WasmStoreHashes {
    genvm_modules_interfaces::WasmStoreHashes(
        hashes
            .0
            .into_iter()
            .map(|(module, fingerprint)| {
                (
                    module,
                    genvm_modules_interfaces::ModuleFingerprint {
                        memories: fingerprint
                            .memories
                            .into_iter()
                            .map(|memory| memory.0)
                            .collect(),
                    },
                )
            })
            .collect(),
    )
}

fn convert_storage_delta(delta: &rt::vm::storage::Delta) -> genvm_modules_interfaces::StorageDelta {
    let (key, value) = delta.cloned_parts();
    genvm_modules_interfaces::StorageDelta::new(key, value)
}

fn convert_buckets_consumed(
    consumed: rt::fees::BucketsConsumed,
) -> genvm_modules_interfaces::BucketsConsumed {
    genvm_modules_interfaces::BucketsConsumed {
        storage: consumed.storage,
        message_receipt: consumed.message_receipt,
        nondet_output: consumed.nondet_output,
        message_fee: consumed.message_fee,
        event: consumed.event,
    }
}

fn convert_on(on: genlayer_sdk::abi::gl_call::On) -> genvm_modules_interfaces::On {
    match on {
        genlayer_sdk::abi::gl_call::On::Finalized => genvm_modules_interfaces::On::Finalized,
        genlayer_sdk::abi::gl_call::On::Accepted => genvm_modules_interfaces::On::Accepted,
    }
}

fn convert_call_key(call_key: genlayer_sdk::abi::CallKey) -> genvm_modules_interfaces::CallKey {
    genvm_modules_interfaces::CallKey(call_key.0)
}

fn convert_internal_message_params(
    params: genlayer_sdk::abi::fees::InternalMessageParams,
) -> genvm_modules_interfaces::fees::InternalMessageParams {
    genvm_modules_interfaces::fees::InternalMessageParams {
        leader_timeunits_allocation: params.leader_timeunits_allocation,
        validator_timeunits_allocation: params.validator_timeunits_allocation,
        execution_budget_per_round: params.execution_budget_per_round,
        rotations: params.rotations,
        max_price_gen_per_time_unit: params.max_price_gen_per_time_unit,
        storage_fee_max_gas_price: params.storage_fee_max_gas_price,
        receipt_fee_max_gas_price: params.receipt_fee_max_gas_price,
    }
}

fn convert_external_message_params(
    params: genlayer_sdk::abi::fees::ExternalMessageParams,
) -> genvm_modules_interfaces::fees::ExternalMessageParams {
    genvm_modules_interfaces::fees::ExternalMessageParams {
        gas_limit: params.gas_limit,
        max_gas_price: params.max_gas_price,
    }
}

fn convert_emission(
    emission: domain::ExecutionEmission,
) -> genvm_modules_interfaces::ExecutionEmission {
    match emission {
        domain::ExecutionEmission::EthSend {
            address,
            calldata,
            value,
            message_fee,
            receipt_fee,
            fee_params,
        } => genvm_modules_interfaces::ExecutionEmission::EthSend {
            address,
            calldata,
            value,
            message_fee,
            receipt_fee,
            fee_params: convert_external_message_params(fee_params),
        },
        domain::ExecutionEmission::PostMessage {
            call_key,
            address,
            calldata,
            value,
            on,
            message_fee,
            receipt_fee,
            fee_params,
            subtree,
            use_balance,
        } => genvm_modules_interfaces::ExecutionEmission::PostMessage {
            call_key: convert_call_key(call_key),
            address,
            calldata: calldata::unparsed::Maybe::from_raw(calldata::encode_obj(&calldata).into()),
            value,
            on: convert_on(on),
            message_fee,
            receipt_fee,
            fee_params: convert_internal_message_params(fee_params),
            subtree,
            use_balance,
        },
        domain::ExecutionEmission::DeployContract {
            calldata,
            code,
            value,
            on,
            salt_nonce,
            message_fee,
            receipt_fee,
            fee_params,
            subtree,
            use_balance,
        } => genvm_modules_interfaces::ExecutionEmission::DeployContract {
            calldata: calldata::unparsed::Maybe::from_raw(calldata::encode_obj(&calldata).into()),
            code,
            value,
            on: convert_on(on),
            salt_nonce,
            message_fee,
            receipt_fee,
            fee_params: convert_internal_message_params(fee_params),
            subtree,
            use_balance,
        },
        domain::ExecutionEmission::EmitEvent {
            topics,
            blob,
            storage_fee,
        } => genvm_modules_interfaces::ExecutionEmission::EmitEvent {
            topics,
            blob,
            storage_fee,
        },
    }
}

impl Host {
    fn lock_sock(&mut self) -> sync::Lock<&mut dyn Sock, stats::tracker::Time> {
        sync::Lock::new(
            &mut *self.sock,
            stats::tracker::Time::new(self.metrics.gep(|x| &x.time)),
        )
    }

    fn get_locked_slots(
        &mut self,
        contract_address: calldata::Address,
        limiter: &rt::memlimiter::Limiter,
    ) -> Result<LockedSlotsSet> {
        let locked_slot = SlotID::ZERO.indirection(root_offsets::LOCKED_SLOTS);

        let mut len_buf = [0; 4];
        self.storage_read(
            StorageType::Default,
            contract_address,
            locked_slot,
            0,
            &mut len_buf,
        )?;
        let len = u32::from_le_bytes(len_buf);

        if len > top_limits::LOCKED_SLOTS {
            return Err(
                rt::errors::Error::vm(abi::consts::VmError::out_of().locked_slots()).into(),
            );
        }

        if !limiter.consume_mul(len, SlotID::SIZE) {
            return Err(
                rt::errors::Error::vm(abi::consts::VmError::out_of().memory().val()).into(),
            );
        }

        let mut res = Box::<[SlotID]>::new_uninit_slice(len.into_int_comptime());
        for slot in &mut res {
            slot.write(SlotID::ZERO);
        }
        let mut res = unsafe { res.assume_init() };

        let read_to = unsafe {
            std::slice::from_raw_parts_mut(res.as_mut_ptr().cast(), std::mem::size_of_val(&*res))
        };

        self.storage_read(
            StorageType::Default,
            contract_address,
            locked_slot,
            4,
            read_to,
        )?;

        res.sort();

        Ok(LockedSlotsSet(res))
    }

    pub fn get_locked_slots_for_sender(
        &mut self,
        contract_address: calldata::Address,
        sender: calldata::Address,
        limiter: &rt::memlimiter::Limiter,
    ) -> Result<LockedSlotsSet> {
        let upgraders_slot = SlotID::ZERO.indirection(root_offsets::UPGRADERS);

        let mut len_buf = [0; 4];
        self.storage_read(
            StorageType::Default,
            contract_address,
            upgraders_slot,
            0,
            &mut len_buf,
        )?;
        let len = u32::from_le_bytes(len_buf);

        if len > top_limits::UPGRADERS {
            return Err(rt::errors::Error::vm(abi::consts::VmError::out_of().upgraders()).into());
        }

        for i in 0..len {
            let mut read_sender = [0; ADDRESS_SIZE];

            self.storage_read(
                StorageType::Default,
                contract_address,
                upgraders_slot,
                4 + i * Address::SIZE,
                &mut read_sender,
            )?;

            if read_sender == sender.raw() {
                return Ok(LockedSlotsSet(Box::from([])));
            }
        }

        self.get_locked_slots(contract_address, limiter)
    }

    pub fn storage_read(
        &mut self,
        mode: StorageType,
        account: calldata::Address,
        slot: SlotID,
        index: u32,
        buf: &mut [u8],
    ) -> Result<()> {
        let mut sock = self.lock_sock();

        sock.write_all(&[host_fns::Methods::StorageRead as u8])?;
        sock.write_all(&[mode as u8; 1])?;
        sock.write_all(&account.raw())?;
        sock.write_all(&slot.raw())?;
        sock.write_all(&index.to_le_bytes())?;
        let buf_len: u32 = buf.len().into_int_downcast_panicking();
        sock.write_all(&buf_len.to_le_bytes())?;

        handle_host_error(&mut **sock, "storage_read")?;

        sock.read_exact(buf)
            .with_context(|| format!("reading {} bytes from storage slot {:?}", buf.len(), slot))?;

        log_trace!(slot:bytes = slot.0, index = index, data:bytes = buf; "read");

        Ok(())
    }

    pub fn resolve_callcontract_executor(
        &mut self,
        contract_address: calldata::Address,
        state_mode: StorageType,
        advisory_major: u8,
    ) -> Result<Option<bytes::Bytes>> {
        log_trace!("resolve_callcontract_executor");

        let mut sock = self.lock_sock();
        sock.write_all(&[host_fns::Methods::ResolveCallcontractExecutor as u8])?;
        sock.write_all(&contract_address.raw())?;
        sock.write_all(&[state_mode as u8, advisory_major])?;

        handle_host_error(&mut **sock, "resolve_callcontract_executor")?;
        let encoded = read_bytes(&mut **sock, "resolve_callcontract_executor result")?;
        calldata::decode_obj(&encoded).context("decoding resolve_callcontract_executor result")
    }

    pub fn run_nested(
        &mut self,
        envelope: &genvm_modules_interfaces::NestedRunEnvelope,
    ) -> Result<genvm_modules_interfaces::NestedRunReply> {
        log_trace!("run_nested");

        let encoded = calldata::encode_obj(envelope);
        let mut sock = self.lock_sock();
        sock.write_all(&[host_fns::Methods::RunNested as u8])?;
        write_slice(&mut **sock, &encoded)?;

        let reply = read_bytes(&mut **sock, "run_nested result")?;
        calldata::decode_obj(&reply).context("decoding run_nested result")
    }

    pub fn consume_result(&mut self, res: &Result<FullResult>) -> Result<()> {
        log_trace!("consume_result");

        if let Ok(r) = res {
            log_debug!(
                emissions = r.emissions.len(),
                kind:? = r.kind;
                "consume_result: serializing FullResult to host"
            );
        }

        let data = encode_result(res)?;

        let mut sock = self.lock_sock();

        sock.write_all(&[host_fns::Methods::ConsumeResult as u8])?;
        write_slice(&mut **sock, &data)?;

        log_debug!("wrote consumed result to host");

        let mut int_buf = [0; 1];
        sock.read_exact(&mut int_buf)?;

        log_debug!("consume_result: ACK");

        Ok(())
    }

    pub fn consume_fuel(&mut self, gas: primitive_types::U256) -> Result<()> {
        log_trace!("consume_fuel");

        let mut sock = self.lock_sock();
        sock.write_all(&[host_fns::Methods::ConsumeFuel as u8])?;
        let buf = gas.to_little_endian();
        sock.write_all(&buf)?;

        sock.flush()?;
        Ok(())
    }

    pub fn eth_call(
        &mut self,
        address: calldata::Address,
        calldata: &[u8],
        limit: u32,
    ) -> Result<Option<Box<[u8]>>> {
        log_trace!("eth_call");

        let mut sock = self.lock_sock();
        sock.write_all(&[host_fns::Methods::EthCall as u8])?;

        sock.write_all(&address.raw())?;

        let calldata_len: u32 = calldata.len().into_int_downcast_panicking();
        sock.write_all(&calldata_len.to_le_bytes())?;
        sock.write_all(calldata)?;

        handle_host_error(&mut **sock, "eth_call")?;

        read_or_discard_bytes(&mut **sock, "eth_call result", limit)
    }

    pub fn get_balance(&mut self, address: calldata::Address) -> Result<primitive_types::U256> {
        log_trace!("get_balance");

        let mut sock = self.lock_sock();
        sock.write_all(&[host_fns::Methods::GetBalance as u8])?;

        sock.write_all(&address.raw())?;

        handle_host_error(&mut **sock, "get_balance")?;

        let mut buf: [u8; 32] = [0; 32];
        sock.read_exact(&mut buf)
            .with_context(|| format!("reading balance for address {:?}", address))?;
        Ok(primitive_types::U256::from_little_endian(&buf))
    }

    pub fn remaining_fuel_as_gen(&mut self) -> Result<primitive_types::U256> {
        log_trace!("remaining_fuel_as_gen");

        let mut sock = self.lock_sock();
        sock.write_all(&[host_fns::Methods::RemainingFuelAsGen as u8])?;

        handle_host_error(&mut **sock, "remaining_fuel_as_gen")?;

        let mut buf: [u8; 32] = [0; 32];
        sock.read_exact(&mut buf)
            .with_context(|| "reading remaining fuel")?;
        Ok(primitive_types::U256::from_little_endian(&buf))
    }

    pub fn notify_nondet_disagreement(&mut self, call_no: u32) -> Result<()> {
        log_trace!(call_no = call_no; "notify_nondet_disagreement");

        let mut sock = self.lock_sock();
        sock.write_all(&[host_fns::Methods::NotifyNondetDisagreement as u8])?;
        sock.write_all(&call_no.to_le_bytes())?;

        sock.flush()?;

        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.lock_sock().flush().context("flushing host socket")
    }
}

pub struct MultiHost {
    hosts: Vec<tokio::sync::Mutex<Host>>,
    method_hosts: Vec<u8>,
}

impl MultiHost {
    pub fn new(hosts: Vec<Host>, method_hosts: Vec<u8>) -> Self {
        Self {
            hosts: hosts.into_iter().map(tokio::sync::Mutex::new).collect(),
            method_hosts,
        }
    }

    pub async fn lock_for(&self, method: host_fns::Methods) -> tokio::sync::MutexGuard<'_, Host> {
        let idx = if (method as usize) < self.method_hosts.len() {
            self.method_hosts[method as usize].into()
        } else {
            0
        };
        self.hosts[idx].lock().await
    }

    pub async fn flush_all(&self) -> Result<()> {
        for host in &self.hosts {
            host.lock().await.flush()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    use std::path::PathBuf;
    use std::thread;

    fn metrics() -> sync::DArc<Metrics> {
        sync::DArc::new(Metrics::default())
    }

    fn bind_socket(name: &str) -> (PathBuf, String, UnixListener) {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "genvm-host-{name}-{}-{suffix}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).expect("bind unix listener");
        let addr = format!("unix://{}", path.display());
        (path, addr, listener)
    }

    fn connect_consume_fuel_and_read(hello_data: &[u8], read_len: usize) -> Vec<u8> {
        let (path, addr, listener) = bind_socket("hello");
        let reader = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept host connection");
            let mut buf = vec![0; read_len];
            stream.read_exact(&mut buf).expect("read host bytes");
            buf
        });

        let mut host = Host::connect(&addr, metrics(), hello_data).expect("connect host");
        host.consume_fuel(primitive_types::U256::from(7))
            .expect("consume fuel");
        let buf = reader.join().expect("reader thread");
        let _ = std::fs::remove_file(path);
        buf
    }

    #[test]
    fn writes_hello_before_first_method_byte() {
        let hello_data = b"hello-prefix";
        let buf = connect_consume_fuel_and_read(hello_data, hello_data.len() + 1 + 32);

        assert_eq!(&buf[..hello_data.len()], hello_data);
        assert_eq!(buf[hello_data.len()], host_fns::Methods::ConsumeFuel as u8);
    }

    #[test]
    fn short_hello_vector_means_empty_for_missing_host_index() {
        let host_hello_data = [bytes::Bytes::from_static(b"host-zero-only")];
        let hello_data = host_hello_data.get(1).map_or(&[][..], bytes::Bytes::as_ref);
        let buf = connect_consume_fuel_and_read(hello_data, 1 + 32);

        assert_eq!(buf[0], host_fns::Methods::ConsumeFuel as u8);
    }

    fn resolve_reply(reply: Option<bytes::Bytes>) -> Option<bytes::Bytes> {
        let (path, addr, listener) = bind_socket("resolve");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept host connection");
            let mut request = [0; 23];
            stream
                .read_exact(&mut request)
                .expect("read resolve request");
            assert_eq!(
                request[0],
                host_fns::Methods::ResolveCallcontractExecutor as u8
            );
            assert_eq!(&request[1..21], &[7; 20]);
            assert_eq!(request[21], StorageType::LatestFinalized as u8);
            assert_eq!(request[22], 3);

            let encoded = calldata::encode_obj(&reply);
            stream
                .write_all(&[host_fns::Errors::Ok as u8])
                .expect("write host status");
            stream
                .write_all(&(encoded.len() as u32).to_le_bytes())
                .expect("write resolve length");
            stream.write_all(&encoded).expect("write resolve reply");
        });

        let mut host = Host::connect(&addr, metrics(), &[]).expect("connect host");
        let result = host
            .resolve_callcontract_executor(
                calldata::Address::from([7; 20]),
                StorageType::LatestFinalized,
                3,
            )
            .expect("resolve executor");
        server.join().expect("server thread");
        let _ = std::fs::remove_file(path);
        result
    }

    #[test]
    fn resolve_callcontract_executor_preserves_null_and_empty_payload() {
        assert_eq!(resolve_reply(None), None);
        assert_eq!(
            resolve_reply(Some(bytes::Bytes::new())),
            Some(bytes::Bytes::new())
        );
    }

    #[test]
    fn run_nested_uses_length_prefixed_calldata_frames() {
        let (path, addr, listener) = bind_socket("nested");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept host connection");
            let mut method = [0; 1];
            stream.read_exact(&mut method).expect("read nested method");
            assert_eq!(method[0], host_fns::Methods::RunNested as u8);

            let mut len = [0; 4];
            stream.read_exact(&mut len).expect("read nested length");
            let mut body = vec![0; u32::from_le_bytes(len) as usize];
            stream.read_exact(&mut body).expect("read nested body");
            let envelope: genvm_modules_interfaces::NestedRunEnvelope =
                calldata::decode_obj(&body).expect("decode nested envelope");
            assert_eq!(
                envelope.routing_payload,
                bytes::Bytes::from_static(b"route")
            );
            assert_eq!(envelope.remaining_recursion, 4);

            let reply = genvm_modules_interfaces::NestedRunReply {
                result: genvm_modules_interfaces::NestedRunResult {
                    kind: genvm_modules_interfaces::ResultCode::Return,
                    data: calldata::Value::Null.into(),
                },
                small_hash: bytes::Bytes::from(vec![9; 32]),
                effect_free: true,
            };
            let encoded = calldata::encode_obj(&reply);
            stream
                .write_all(&(encoded.len() as u32).to_le_bytes())
                .expect("write nested length");
            stream.write_all(&encoded).expect("write nested reply");
        });

        let envelope = genvm_modules_interfaces::NestedRunEnvelope {
            routing_payload: bytes::Bytes::from_static(b"route"),
            calldata: bytes::Bytes::from_static(b"call"),
            message: genvm_modules_interfaces::MessageData {
                contract_address: calldata::Address::zero(),
                sender_address: calldata::Address::zero(),
                origin_address: calldata::Address::zero(),
                signer_address: calldata::Address::zero(),
                chain_id: num_bigint::BigInt::ZERO,
                value: num_bigint::BigInt::ZERO,
                is_init: false,
                datetime: chrono::DateTime::parse_from_rfc3339("2026-07-28T00:00:00Z")
                    .unwrap()
                    .to_utc(),
            },
            stack: Vec::new(),
            permissions: genvm_modules_interfaces::NestedPermissions::DETERMINISTIC,
            state_mode: genvm_modules_interfaces::NestedStorageType::LatestDecided,
            topmost_runner_id: genvm_modules_interfaces::NestedRunnerId("contract".to_owned()),
            remaining_recursion: 4,
            remaining_det_fuel: primitive_types::U256::from(10),
            memory_limit: 1024,
        };
        let mut host = Host::connect(&addr, metrics(), &[]).expect("connect host");
        let reply = host.run_nested(&envelope).expect("run nested");
        assert!(reply.effect_free);
        assert_eq!(reply.small_hash, bytes::Bytes::from(vec![9; 32]));

        server.join().expect("server thread");
        let _ = std::fs::remove_file(path);
    }

    fn eth_send_with_calldata(calldata: &[u8]) -> domain::ExecutionEmission {
        domain::ExecutionEmission::EthSend {
            address: Address::from([0; ADDRESS_SIZE]),
            calldata: bytes::Bytes::copy_from_slice(calldata),
            value: primitive_types::U256::zero(),
            message_fee: primitive_types::U256::zero(),
            receipt_fee: primitive_types::U256::zero(),
            fee_params: genlayer_sdk::abi::fees::ExternalMessageParams {
                gas_limit: primitive_types::U256::zero(),
                max_gas_price: primitive_types::U256::zero(),
            },
        }
    }

    fn execution_hash_with(emissions: Vec<domain::ExecutionEmission>) -> bytes::Bytes {
        let mut rt_result = rt::vm::FullResult::empty_from(rt::vm::RunOk::empty_return());
        rt_result.emissions = emissions;

        FullResult::new(
            rt_result,
            Vec::new(),
            None,
            Vec::new(),
            rt::fees::BucketsConsumed::default(),
            primitive_types::U256::zero(),
            Vec::new(),
        )
        .reported
        .execution_hash
    }

    // An emission's content is consensus-critical, and equal-fee emissions are
    // indistinguishable by the fee keys alone.
    #[test]
    fn execution_hash_covers_emission_content() {
        let one = execution_hash_with(vec![eth_send_with_calldata(b"one")]);
        let other = execution_hash_with(vec![eth_send_with_calldata(b"two")]);

        assert_ne!(one, other);
        assert_ne!(one, execution_hash_with(Vec::new()));
    }

    // FullResult preserves nested fatality until the publication boundary
    #[test]
    fn fatal_run_ok_keeps_its_code_and_payload() {
        let code = crate::public_abi::VmError::timeout();
        let result =
            rt::vm::FullResult::empty_from(rt::vm::RunOk::FatalVMError(code.clone(), None));

        assert_eq!(result.kind, host_fns::ResultCode::FatalVmError);
        assert_eq!(result.data, calldata::Value::Str(code.into()).into());
    }

    #[test]
    fn top_level_fatal_is_framed_and_reported_as_vm_error() {
        let mut rt_result = rt::vm::FullResult::empty_from(rt::vm::RunOk::FatalVMError(
            crate::public_abi::VmError::timeout(),
            None,
        ));
        rt_result.coalesce_fatal_for_top_level();
        let result = FullResult::new(
            rt_result,
            Vec::new(),
            None,
            Vec::new(),
            rt::fees::BucketsConsumed::default(),
            primitive_types::U256::zero(),
            Vec::new(),
        );

        let encoded = encode_result(&Ok(result)).unwrap();
        let reported: genvm_modules_interfaces::ReportedResult =
            calldata::decode_obj(&encoded[1..]).unwrap();

        assert_eq!(encoded[0], host_fns::ResultCode::VmError as u8);
        assert_eq!(reported.kind, genvm_modules_interfaces::ResultCode::VmError);
    }
}
