use super::*;
use crate::runners;
use sha3::digest::Update;
use std::sync::Arc;

async fn consume_nondet_output(
    shared_data: &rt::SharedData,
    output_length: u64,
) -> Result<(), generated::types::Error> {
    if !shared_data
        .data_fees_limit
        .consume_nondet_output(output_length)
        .await
        .map_err(internal_trap)?
    {
        return Err(internal_trap(rt::errors::Error::vm(
            abi::consts::VmError::out_of().receipt().nondet_output(),
        )));
    }
    Ok(())
}

impl ContextVFS<'_> {
    pub(super) async fn gl_call_eth_call(
        &mut self,
        address: calldata::Address,
        calldata: bytes::Bytes,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        if !self.context.data.conf.permissions.deterministic {
            return Err(generated::types::Errno::Forbidden.into());
        }
        if !self.context.data.conf.permissions.call_others {
            return Err(generated::types::Errno::Forbidden.into());
        }

        let supervisor = self.context.data.supervisor.clone();
        let data = supervisor
            .host
            .lock_for(host::host_fns::Methods::EthCall)
            .await
            .eth_call(address, &calldata)
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;
        self.place_content(vfs::FileContents::from(bytes::Bytes::from(data)))
    }
    pub(super) async fn gl_call_contract(
        &mut self,
        address: calldata::Address,
        calldata: calldata::unparsed::Maybe<calldata::Value>,
        mut state: public_abi::StorageType,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        if !self.context.data.conf.permissions.deterministic {
            return Err(generated::types::Errno::Forbidden.into());
        }
        if !self.context.data.conf.permissions.call_others {
            return Err(generated::types::Errno::Forbidden.into());
        }

        let supervisor = self.context.data.supervisor.clone();

        let my_conf = self.context.data.conf.clone();

        let calldata_encoded = calldata::encode_obj(&calldata);

        let mut my_data = self
            .context
            .data
            .message_data
            .fork(public_abi::EntryKind::Main, calldata_encoded.into());
        my_data.message.stack.push(my_data.message.contract_address);

        if state == public_abi::StorageType::Default {
            state = my_conf.execution.state_mode;
        }

        let mut child_storage = rt::vm::storage::Storage::new(
            address,
            supervisor.get_storage_limiter(),
            StorageHostHolder(
                supervisor.host.clone(),
                ReadToken {
                    account: address,
                    mode: state,
                },
            ),
        );

        let code_slot = child_storage
            .check_major_and_resolve_code_slot()
            .await
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e.into())))?;

        let vm_data = Box::new(SingleVMData {
            limiter: self.context.limiter.derived(),
            depth: self.context.data.depth + 1,
            spawn_kind: "call_contract".to_owned(),
            // Permission model: docs/website/src/spec/03-vm/02-meta-properties.rst
            conf: base::Config {
                needs_error_fingerprint: true,
                permissions: base::Permissions {
                    deterministic: true,
                    write_storage: false,
                    spawn_nondet: false,
                    call_others: my_conf.permissions.call_others,
                    send_messages: false,
                    register_runners: my_conf.permissions.register_runners,
                    can_use_balance_for_message_fees: false,
                },
                execution: base::Execution {
                    state_mode: state,
                    topmost_runner_id: runners::Id::Chain {
                        address,
                        on: if state == public_abi::StorageType::LatestFinal {
                            runners::ChainState::Finalized
                        } else {
                            runners::ChainState::Accepted
                        },
                        slot: code_slot,
                    },
                },
            },
            message_data: ExtendedMessage {
                message: genlayer_sdk::abi::entry::MessageData {
                    contract_address: address,
                    sender_address: my_data.message.sender_address,
                    origin_address: my_data.message.origin_address,
                    signer_address: my_data.message.signer_address,
                    value: num_bigint::BigInt::ZERO,
                    is_init: false,
                    datetime: my_data.message.datetime,
                    chain_id: my_data.message.chain_id,
                    stack: my_data.message.stack,
                },
                entry_kind: my_data.entry_kind,
                entry_data: my_data.entry_data,
                entry_stage_data: default_entry_stage_data(),
            },
            storage: child_storage,
            supervisor: supervisor.clone(),
            accumulator: VMDataAccumulator {
                data_fees_limit: self.context.data.accumulator.data_fees_limit.clone(),
                messages_value_decremented: primitive_types::U256::zero(),
                emissions: Vec::new(),
                message_fee_allocation: Vec::new(),
            },
            det_subvm_hashes: Default::default(),
            // A CallContract child is granted the caller's full custom set
            // (ADR-012 §4); its spawn load-actions each into the child.
            granted_custom: self.context.loaded.custom_pins(),
        });

        let res = spawn_sub_vm(supervisor.clone(), vm_data)
            .await
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

        // The child is read-only (static), so its accumulator must be
        // empty — otherwise an effect was charged but discarded here.
        res.vm_data
            .accumulator
            .check_empty()
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

        let hash = res.small_hash();
        self.context.data.det_subvm_hashes.update(&hash);

        self.set_vm_run_result(res.run_ok).map(|x| x.0)
    }
    pub(super) async fn run_nondet(
        &mut self,
        data_leader: bytes::Bytes,
        data_validator: bytes::Bytes,
        runner: Option<String>,
        custom_runners: Option<Vec<String>>,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        if !self.context.data.conf.permissions.spawn_nondet {
            return Err(generated::types::Errno::Forbidden.into());
        }

        // Resolve the runner to execute and the child's custom-runner visibility
        // in this (deterministic parent) scope, so malformed inputs fail
        // deterministically at gl_call time (ADR-012 §4).
        let supervisor = self.context.data.supervisor.clone();
        let parent_runner_id = self.context.data.conf.execution.topmost_runner_id.clone();
        let child_topmost_id = match &runner {
            None => parent_runner_id.clone(),
            Some(r) => {
                rt::supervisor::actions::resolve_runner_id(&supervisor, &parent_runner_id, r)
                    .await
                    .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?
            }
        };
        let child_custom = rt::supervisor::actions::resolve_child_custom_runners(
            &self.context.loaded,
            custom_runners,
            &child_topmost_id,
        )
        .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

        let call_no = self
            .context
            .data
            .supervisor
            .nondet_call_no
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        if call_no >= public_abi::top_limits::NONDET_BLOCKS {
            return Err(generated::types::Error::trap(crate::anyhow_to_wasmtime(
                rt::errors::Error::vm(abi::consts::VmError::out_of().nondet_blocks()).into(),
            )));
        }

        let leaders_res_bytes = self
            .context
            .data
            .supervisor
            .get_leader_nondet_result(call_no);

        let leaders_res = match leaders_res_bytes {
            None if self.context.data.supervisor.is_leader() => None,
            None => {
                return Err(generated::types::Error::trap(crate::anyhow_to_wasmtime(
                    rt::errors::Error::vm(abi::consts::VmError::absent_leader_nondet_output())
                        .into(),
                )));
            }
            Some(data) if data.is_empty() => {
                // A zero-length leader result is malformed: it carries no result
                // code byte to dispatch on. It is contract-triggerable, so it must
                // not panic via out-of-bounds indexing. In `sync` mode there are no
                // validators to disagree, so surface a canonical VMError; otherwise
                // record a non-deterministic disagreement and return that VMError to
                // the validator's contract.
                if !self.context.data.supervisor.shared_data.is_sync {
                    self.context
                        .data
                        .supervisor
                        .mark_nondet_disagreement(call_no);
                }

                let result = rt::vm::RunOk::VMError(
                    abi::consts::VmError::absent_leader_nondet_output(),
                    None,
                );

                consume_nondet_output(
                    &self.context.data.supervisor.shared_data,
                    result.as_bytes().len() as u64,
                )
                .await?;

                return self.set_vm_run_result(result).map(|x| x.0);
            }
            Some(data) => {
                use crate::public_abi::ResultCode;
                let rest = &data[1..];
                let res = match data[0] {
                    x if x == ResultCode::Return as u8 => {
                        rt::vm::RunOk::Return(calldata::unparsed::Maybe::Checked(
                            calldata::unparsed::Raw(bytes::Bytes::copy_from_slice(rest)),
                        ))
                    }
                    x if x == ResultCode::UserError as u8 => {
                        let val = calldata::decode(rest).map_err(|e| {
                            generated::types::Error::trap(crate::anyhow_to_wasmtime(
                                anyhow::anyhow!(e),
                            ))
                        })?;
                        rt::vm::RunOk::UserError(calldata::unparsed::Maybe::Materialized(val))
                    }
                    x if x == ResultCode::VmError as u8 => {
                        let code = std::str::from_utf8(rest).map_err(|e| {
                            generated::types::Error::trap(crate::anyhow_to_wasmtime(
                                anyhow::anyhow!(e),
                            ))
                        })?;
                        rt::vm::RunOk::VMError(
                            public_abi::VmError(std::borrow::Cow::Owned(code.to_owned())),
                            None,
                        )
                    }
                    x => {
                        return Err(generated::types::Error::trap(crate::anyhow_to_wasmtime(
                            anyhow::anyhow!("invalid leader result code: {}", x),
                        )));
                    }
                };
                Some(res)
            }
        };

        let result_to_return = if self.context.data.supervisor.shared_data.is_sync {
            match leaders_res {
                None => {
                    return Err(generated::types::Error::trap(crate::anyhow_to_wasmtime(
                        anyhow::anyhow!("absent leader result in sync mode, call_no: {}", call_no),
                    )))
                }
                Some(v) => v,
            }
        } else {
            let storage_checkpoint = self.context.data.storage.clone();

            let message_data = match &leaders_res {
                None => self.context.data.message_data.fork_leader(
                    public_abi::EntryKind::ConsensusStage,
                    data_leader,
                    None,
                ),
                Some(leaders_res) => {
                    let dup = match leaders_res {
                        rt::vm::RunOk::Return(items) => rt::vm::RunOk::Return(items.clone()),
                        rt::vm::RunOk::UserError(msg) => rt::vm::RunOk::UserError(msg.clone()),
                        rt::vm::RunOk::VMError(msg, _) => rt::vm::RunOk::VMError(msg.clone(), None),
                    };
                    self.context.data.message_data.fork_leader(
                        public_abi::EntryKind::ConsensusStage,
                        data_validator,
                        Some(dup),
                    )
                }
            };

            let fake_accum = VMDataAccumulator {
                data_fees_limit: self.context.data.accumulator.data_fees_limit.clone(),
                messages_value_decremented: self
                    .context
                    .data
                    .accumulator
                    .messages_value_decremented,
                emissions: Vec::new(),
                message_fee_allocation: Vec::new(),
            };

            let vm_data = Box::new(SingleVMData {
                limiter: Default::default(),
                depth: self.context.data.depth + 1,
                spawn_kind: "run_nondet".to_owned(),
                // Permission model: docs/website/src/spec/03-vm/02-meta-properties.rst
                conf: base::Config {
                    needs_error_fingerprint: false,
                    permissions: base::Permissions {
                        deterministic: false,
                        write_storage: false,
                        send_messages: false,
                        call_others: false,
                        spawn_nondet: false,
                        register_runners: false,
                        can_use_balance_for_message_fees: false,
                    },
                    execution: base::Execution {
                        state_mode: public_abi::StorageType::Default,
                        topmost_runner_id: child_topmost_id,
                    },
                },
                message_data,
                supervisor: supervisor.clone(),
                storage: storage_checkpoint,
                accumulator: fake_accum,
                det_subvm_hashes: Default::default(), // won't be used
                // The nondet child is granted exactly the parent-computed set; the
                // pins keep the content alive until the (possibly queued) child
                // spawns and load-actions them against its own limiter (ADR-012 §4).
                granted_custom: child_custom,
            });

            let task_done = Arc::new(tokio::sync::Notify::new());
            let task = rt::supervisor::NonDetVMTask {
                task: vm_data,
                call_no,
                tasks_done: task_done.clone(),
            };

            match leaders_res {
                None => {
                    let res = task
                        .run_now(&self.context.data.supervisor)
                        .await
                        .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

                    self.context
                        .data
                        .supervisor
                        .push_nondet_result(call_no, bytes::Bytes::from(res.as_bytes()))
                        .await;

                    res
                }
                Some(leaders_res) => {
                    rt::supervisor::submit_nondet_vm_task(&self.context.data.supervisor, task)
                        .await;

                    leaders_res
                }
            }
        };

        consume_nondet_output(
            &self.context.data.supervisor.shared_data,
            result_to_return.as_bytes().len() as u64,
        )
        .await?;

        self.set_vm_run_result(result_to_return).map(|x| x.0)
    }

    pub(super) async fn sandbox(
        &mut self,
        data: bytes::Bytes,
        runner: String,
        allow_write_storage: bool,
        allow_send_messages: bool,
        allow_register_runners: bool,
        custom_runners: Option<Vec<String>>,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        let supervisor = self.context.data.supervisor.clone();

        let parent_runner_id = self.context.data.conf.execution.topmost_runner_id.clone();
        let topmost_runner_id =
            rt::supervisor::actions::resolve_runner_id(&supervisor, &parent_runner_id, &runner)
                .await
                .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

        // Grants are validated against, and drawn from, this VM's loaded set (§4).
        // No flow-back is possible by construction: the child's own loaded set (and
        // any runner it registers) dies with it, and the parent's loaded set is
        // never mutated here (§5).
        let child_custom = rt::supervisor::actions::resolve_child_custom_runners(
            &self.context.loaded,
            custom_runners,
            &topmost_runner_id,
        )
        .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

        let message_data = self
            .context
            .data
            .message_data
            .fork(public_abi::EntryKind::Sandbox, data);

        let zelf_conf = &self.context.data.conf;

        let storage_checkpoint = self.context.data.storage.clone();

        let mut fake_my_data = VMDataAccumulator {
            data_fees_limit: self.context.data.accumulator.data_fees_limit.clone(),
            messages_value_decremented: primitive_types::U256::max_value(),
            emissions: Vec::new(),
            message_fee_allocation: Vec::new(),
        };

        std::mem::swap(&mut self.context.data.accumulator, &mut fake_my_data);

        let stolen_data = fake_my_data;

        let vm_data = Box::new(SingleVMData {
            limiter: self.context.limiter.derived(),
            depth: self.context.data.depth + 1,
            spawn_kind: "sandbox".to_owned(),
            // Permission model: docs/website/src/spec/03-vm/02-meta-properties.rst
            conf: base::Config {
                needs_error_fingerprint: false,
                permissions: base::Permissions {
                    deterministic: zelf_conf.permissions.deterministic,
                    write_storage: zelf_conf.permissions.write_storage & allow_write_storage,
                    send_messages: zelf_conf.permissions.send_messages & allow_send_messages,
                    call_others: false,
                    spawn_nondet: false,
                    register_runners: zelf_conf.permissions.register_runners
                        & allow_register_runners,
                    can_use_balance_for_message_fees: zelf_conf
                        .permissions
                        .can_use_balance_for_message_fees
                        & allow_send_messages,
                },
                execution: base::Execution {
                    state_mode: zelf_conf.execution.state_mode,
                    topmost_runner_id,
                },
            },
            message_data,
            supervisor: supervisor.clone(),
            storage: storage_checkpoint,
            accumulator: stolen_data,
            det_subvm_hashes: Default::default(),
            granted_custom: child_custom,
        });

        let my_res = spawn_sub_vm(supervisor.clone(), vm_data)
            .await
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

        if self.context.data.conf.permissions.deterministic {
            let hash = my_res.small_hash();
            self.context.data.det_subvm_hashes.update(&hash);
        }

        self.context.data.accumulator = my_res.vm_data.accumulator;
        self.context.data.storage = my_res.vm_data.storage;

        let data: Vec<u8> = my_res.run_ok.as_bytes();
        self.place_content(vfs::FileContents::from(bytes::Bytes::from(data)))
    }
}
