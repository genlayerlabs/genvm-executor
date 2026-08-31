use super::*;
use crate::rt::errors::ResultExt as _;
use genlayer_calldata::codec::Encode;

/// Named arguments for [`consume_message_fee_internal`].
struct ConsumeInternalArgs {
    is_deploy: bool,
    calldata_length: u64,
    code_length: u64,
    subtree_length: u64,
}

/// How an internal message's fee is funded.
enum FeeFunding<'a> {
    /// Sender-pool allocation: fee capped by `node.budget`; consumes the
    /// message-fee and receipt buckets.
    Allocation(&'a mut genvm_modules_interfaces::fees::MessageAllocationNode),
    /// Balance-funded (`useBalance`): the metered fee is the `declaredBudget`,
    /// reserved from the contract balance. On-chain such messages are excluded
    /// from the sender pool, so the message-fee bucket is skipped and only the
    /// receipt is charged. The balance is checked here, before the receipt is
    /// consumed, so nothing is charged unless funding is guaranteed.
    Balance {
        value: primitive_types::U256,
        messages_value_decremented: primitive_types::U256,
        my_balance: primitive_types::U256,
    },
}

fn convert_on_to_modules(on: gl_call::On) -> genvm_modules_interfaces::On {
    match on {
        gl_call::On::Finalized => genvm_modules_interfaces::On::Finalized,
        gl_call::On::Decided => genvm_modules_interfaces::On::Decided,
    }
}

fn convert_call_key_to_modules(call_key: abi::CallKey) -> genvm_modules_interfaces::CallKey {
    genvm_modules_interfaces::CallKey(call_key.0)
}

fn convert_internal_message_params_to_sdk(
    params: &genvm_modules_interfaces::fees::InternalMessageParams,
) -> abi::fees::InternalMessageParams {
    abi::fees::InternalMessageParams {
        leader_time_units_allocation: params.leader_timeunits_allocation,
        validator_time_units_allocation: params.validator_timeunits_allocation,
        execution_budget_per_round: params.execution_budget_per_round,
        rotations: params.rotations.clone(),
        max_price_gen_per_time_unit: params.max_price_gen_per_time_unit,
        storage_fee_max_gas_price: params.storage_fee_max_gas_price,
        receipt_fee_max_gas_price: params.receipt_fee_max_gas_price,
    }
}

fn convert_external_message_params_to_sdk(
    params: genvm_modules_interfaces::fees::ExternalMessageParams,
) -> abi::fees::ExternalMessageParams {
    abi::fees::ExternalMessageParams {
        gas_limit: params.gas_limit,
        max_gas_price: params.max_gas_price,
    }
}

async fn consume_message_fee_internal(
    shared_data: &rt::SharedData,
    funding: FeeFunding<'_>,
    fee_params: Arc<abi::fees::InternalMessageParams>,
    on: gl_call::On,
    args: ConsumeInternalArgs,
) -> Result<rt::fees::MessageFeeConsumption, generated::types::Error> {
    let balance_funded = matches!(funding, FeeFunding::Balance { .. });
    let fee_cost = shared_data
        .data_fees_limit
        .calculate_message_fee_internal(on, balance_funded, &fee_params)
        .map_err(internal_trap)?;
    let fee_total = fee_cost.reported_fee();

    let receipt_cost = shared_data
        .data_fees_limit
        .calculate_message_receipt(rt::fees::MessageReceiptParams {
            is_internal: true,
            is_deploy: args.is_deploy,
            calldata_length: args.calldata_length,
            code_length: args.code_length,
            subtree_length: args.subtree_length,
        })
        .ctx("calculate_message_receipt")
        .map_err(internal_trap)?;

    match funding {
        FeeFunding::Allocation(node) => {
            if fee_total > node.budget {
                log_warn!(
                    node:cd = *node,
                    fee_cost:cd = fee_total,
                    budget: cd = node.budget;
                    "message fee cost exceeds node budget"
                );
                return Err(internal_trap(rt::errors::Error::vm(
                    abi::consts::VmError::out_of()
                        .message_fee()
                        .allocation_budget()
                        .internal(),
                )));
            }

            if !shared_data
                .data_fees_limit
                .consume_message_fee(&fee_cost, &receipt_cost)
                .await
            {
                log_warn!(
                    node:cd = *node,
                    fee_cost:cd = fee_total,
                    buckets:? = shared_data.data_fees_limit;
                    "not enough remaining fee limit to consume message fee"
                );
                return Err(internal_trap(rt::errors::Error::vm(
                    abi::consts::VmError::out_of()
                        .message_fee()
                        .total()
                        .internal(),
                )));
            }

            node.budget -= fee_total;
        }
        FeeFunding::Balance {
            value,
            messages_value_decremented,
            my_balance,
        } => {
            // `value + fee_total` can itself overflow U256.
            let Some(total) = value.checked_add(fee_total) else {
                return Err(generated::types::Errno::InsufficientBalance.into());
            };
            if !checked_sum_le(total, messages_value_decremented, my_balance) {
                log_warn!(
                    fee_cost:cd = fee_total,
                    value:cd = value,
                    my_balance:cd = my_balance;
                    "insufficient balance to fund balance-funded message fee"
                );
                return Err(generated::types::Errno::InsufficientBalance.into());
            }

            if !shared_data
                .data_fees_limit
                .consume_message_receipt_only(&receipt_cost)
                .await
            {
                log_warn!(
                    receipt_cost:cd = receipt_cost.reported_fee(),
                    buckets:? = shared_data.data_fees_limit;
                    "not enough remaining receipt limit to consume message receipt"
                );
                return Err(internal_trap(rt::errors::Error::vm(
                    abi::consts::VmError::out_of()
                        .receipt()
                        .message()
                        .internal(),
                )));
            }
        }
    }

    Ok(rt::fees::MessageFeeConsumption {
        message_fee: fee_cost,
        receipt_fee: receipt_cost,
    })
}

/// Named arguments for [`consume_message_fee_external`].
struct ConsumeExternalArgs {
    is_deploy: bool,
    calldata_length: u64,
}

async fn consume_message_fee_external(
    shared_data: &rt::SharedData,
    node: &mut genvm_modules_interfaces::fees::MessageAllocationNode,
    params: abi::fees::ExternalMessageParams,
    // External messages are always emitted on finalization; carried for signature
    // symmetry with the internal path.
    _on: gl_call::On,
    args: ConsumeExternalArgs,
) -> Result<rt::fees::MessageFeeConsumption, generated::types::Error> {
    let fee_cost = shared_data
        .data_fees_limit
        .calculate_message_fee_external(&params)
        .map_err(internal_trap)?;

    let fee_total = fee_cost.reported_fee();
    if fee_total > node.budget {
        return Err(internal_trap(rt::errors::Error::vm(
            abi::consts::VmError::out_of()
                .message_fee()
                .allocation_budget()
                .external(),
        )));
    }

    let receipt_cost = shared_data
        .data_fees_limit
        .calculate_message_receipt(rt::fees::MessageReceiptParams {
            is_internal: false,
            is_deploy: args.is_deploy,
            calldata_length: args.calldata_length,
            code_length: 0,
            subtree_length: 0,
        })
        .map_err(internal_trap)?;

    if !shared_data
        .data_fees_limit
        .consume_message_fee(&fee_cost, &receipt_cost)
        .await
    {
        return Err(internal_trap(rt::errors::Error::vm(
            abi::consts::VmError::out_of()
                .message_fee()
                .total()
                .external(),
        )));
    }

    node.budget -= fee_total;

    Ok(rt::fees::MessageFeeConsumption {
        message_fee: fee_cost,
        receipt_fee: receipt_cost,
    })
}

/// Magnitude bounds (in significant bits; a larger field is rejected) on
/// guest-supplied fee params. Invariant the code cannot express: the worst-case
/// `messageFeeFloor` product must stay within U256, since the fee evaluator's
/// `rational_to_u256` treats overflow as an internal abort. Three guest fields
/// multiply into one floor term (`maxPrice × rotations entry × validatorTU`),
/// so with the default 18-round validator table (counts ≤ 1537 < 2^11) the
/// accepted-lifecycle worst case is
///   lifecycle(<2^4) × [price(<2^96) × rounds(<2^5) × rot(<2^33)
///     × (leaderTU + vpr × validatorTU)(<2^44)
///     + price(<2^96) × leaderRounds(<2^36)]
///   < 2^183 ≪ 2^256.
/// Economically generous: 2^96 atto-GEN ≈ 8e10 GEN for prices/budgets; 2^32
/// for counts (time units per phase, rotations per round).
pub(super) const FEE_PARAM_PRICE_BITS: usize = 96;
pub(super) const FEE_PARAM_COUNT_BITS: usize = 32;

/// Validates the balance-funded fee fields (`use_balance` / `fee_params`) shared
/// by `EmitInternalMessage` and `EmitInternalDeployMessage`. Returns the fee params
/// to meter against the contract balance when `use_balance` is set, or `None`
/// for the ordinary allocation-matched path.
///
/// Guest params are structurally validated so no guest input can drive the fee
/// evaluator into an internal abort or emit a message the chain would reject at
/// reveal: `rotations` must be non-empty (the yaml floor derives
/// `appealRounds = len - 1` and would go negative otherwise), the three
/// price caps must be non-zero (the chain reverts `FeeValueMustBeNonZero(4/5/6)`
/// at reveal), and every numeric field -- rotations entries included -- is
/// bounded by [`FEE_PARAM_PRICE_BITS`] / [`FEE_PARAM_COUNT_BITS`] so the
/// metered floor fits in U256. The rotations-length upper bound is
/// node-config-dependent (validator table length) and is enforced in the yaml
/// fee expression instead.
pub(super) fn validate_balance_fee(
    can_use_balance: bool,
    use_balance: bool,
    fee_params: Option<abi::fees::InternalMessageParams>,
) -> Result<Option<abi::fees::InternalMessageParams>, generated::types::Error> {
    match (use_balance, fee_params) {
        (true, _) if !can_use_balance => {
            log_debug!("balance-funded message rejected: permission absent (Forbidden)");
            Err(generated::types::Errno::Forbidden.into())
        }
        (true, None) => {
            // this is a feature for future
            log_debug!("balance-funded message rejected: fee_params absent (Inval)");
            Err(generated::types::Errno::Inval.into())
        }
        (true, Some(params)) => {
            if params.rotations.is_empty() {
                log_debug!("balance-funded message rejected: rotations empty (Inval)");
                return Err(generated::types::Errno::Inval.into());
            }
            if params.max_price_gen_per_time_unit.is_zero()
                || params.storage_fee_max_gas_price.is_zero()
                || params.receipt_fee_max_gas_price.is_zero()
            {
                log_debug!("balance-funded message rejected: zero price cap (Inval)");
                return Err(generated::types::Errno::Inval.into());
            }
            let too_large = params.max_price_gen_per_time_unit.bits() > FEE_PARAM_PRICE_BITS
                || params.storage_fee_max_gas_price.bits() > FEE_PARAM_PRICE_BITS
                || params.receipt_fee_max_gas_price.bits() > FEE_PARAM_PRICE_BITS
                || params.execution_budget_per_round.bits() > FEE_PARAM_PRICE_BITS
                || params.leader_time_units_allocation.bits() > FEE_PARAM_COUNT_BITS
                || params.validator_time_units_allocation.bits() > FEE_PARAM_COUNT_BITS
                || params
                    .rotations
                    .iter()
                    .any(|r| r.bits() > FEE_PARAM_COUNT_BITS);
            if too_large {
                log_debug!("balance-funded message rejected: fee param too large (Inval)");
                return Err(generated::types::Errno::Inval.into());
            }
            Ok(Some(params))
        }
        (false, Some(_)) => {
            log_debug!("message rejected: fee_params without use_balance (Inval)");
            Err(generated::types::Errno::Inval.into())
        }
        (false, None) => Ok(None),
    }
}

pub(super) struct EmitInternalDeployMessageArgs {
    pub code: bytes::Bytes,
    pub value: primitive_types::U256,
    pub salt_nonce: primitive_types::U256,
    pub use_balance: bool,
}

impl ContextVFS<'_> {
    pub(super) async fn gl_call_emit_external_message(
        &mut self,
        address: calldata::Address,
        calldata: bytes::Bytes,
        value: primitive_types::U256,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        if !self.context.data.conf.permissions.deterministic {
            return Err(generated::types::Errno::Forbidden.into());
        }
        if !self.context.data.conf.permissions.send_messages {
            return Err(generated::types::Errno::Forbidden.into());
        }

        if !value.is_zero() {
            let my_balance = self
                .context
                .get_balance_impl(self.context.data.message_data.message.contract_address)
                .await?;

            if !checked_sum_le(
                value,
                self.context.data.accumulator.messages_value_decremented,
                my_balance,
            ) {
                return Err(generated::types::Errno::InsufficientBalance.into());
            }
        }

        let mut call_key = abi::CallKey([0u8; 32]);
        if calldata.len() < 4 {
            log_warn!(len = calldata.len(); "calldata too short for method selector, using unnamed call key");
        } else {
            call_key.0[..4].copy_from_slice(&calldata[..4]);
        }

        let Some((matched_node, matched_params)) = self
            .context
            .data
            .accumulator
            .message_fee_allocation
            .iter_mut()
            .find_map(|node| {
                node.matches_external(address, convert_call_key_to_modules(call_key))
                    .map(|params| (node, params))
            })
        else {
            log_warn!(
                recipient = address,
                call_key:? = call_key;
                "no matching node for message fee allocation"
            );

            return Err(internal_trap(rt::errors::Error::vm(
                abi::consts::VmError::fee()
                    .no_matching_allocation()
                    .external(),
            )));
        };

        let calldata_length = calldata.len().into_int_comptime();
        let matched_params = convert_external_message_params_to_sdk(matched_params);
        let allocation = reserve_permanent(
            &self.context.limiter,
            emission_allocation_size(&[calldata_length]),
            "external message",
        )?;

        let fees = consume_message_fee_external(
            &self.context.data.supervisor.shared_data,
            matched_node,
            matched_params,
            gl_call::On::Finalized,
            ConsumeExternalArgs {
                is_deploy: false,
                calldata_length,
            },
        )
        .await?;

        self.context
            .data
            .accumulator
            .emissions
            .push(domain::ExecutionEmission::ExternalMessage {
                address,
                calldata,
                value,
                message_fee: fees.message_fee.reported_fee(),
                receipt_fee: fees.receipt_fee.reported_fee(),
                fee_params: matched_params,
            });
        allocation.commit();

        self.context.data.accumulator.messages_value_decremented = self
            .context
            .data
            .accumulator
            .messages_value_decremented
            .saturating_add(value);
        Ok(file_fd_none())
    }
    pub(super) async fn gl_call_emit_event(
        &mut self,
        topics: Vec<bytes::Bytes>,
        blob: calldata::unparsed::Maybe<calldata::Map<calldata::Value>>,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        if !self.context.data.conf.permissions.deterministic {
            log_warn!("EmitEvent requires deterministic mode");

            return Err(generated::types::Errno::Forbidden.into());
        }
        // Events are state-mutating log emissions, so they require the
        // same write capability as storage. A read-only sub-VM (e.g. a
        // `CallContract` child) must not emit events; otherwise the
        // emission is charged but later discarded with the child.
        if !self.context.data.conf.permissions.write_storage {
            log_warn!("EmitEvent requires write_storage permission");

            return Err(generated::types::Errno::Forbidden.into());
        }

        if topics.len() > public_abi::EVENT_MAX_TOPICS.into_int_comptime() {
            log_warn!(cnt = topics.len(), max = public_abi::EVENT_MAX_TOPICS; "too many topics");
            return Err(generated::types::Errno::Inval.into());
        }

        let mut real_topics: Vec<bytes::Bytes> =
            Vec::with_capacity(public_abi::EVENT_MAX_TOPICS.into_int_comptime() + 1);

        for t in topics.iter() {
            if t.len() != 32 {
                log_warn!(len = t.len(); "invalid topic length");

                return Err(generated::types::Errno::Inval.into());
            }

            real_topics.push(t.clone());
        }

        struct CountingWriter(usize);
        impl calldata::Writer for CountingWriter {
            type Error = std::convert::Infallible;

            fn write_all(&mut self, data: &[u8]) -> Result<(), Self::Error> {
                self.0 += data.len();
                Ok(())
            }
        }

        let mut enc = calldata::Encoder::new(CountingWriter(0));
        blob.encode(&mut enc).unwrap_or_else(|e| match e {});
        let blob_size = enc.into_inner().0.into_int_comptime();

        let supervisor = self.context.data.supervisor.clone();
        let topics_count = topics.len().into_int_comptime();
        let topics_size = topics
            .iter()
            .map(|topic| usize_into_u64(topic.len()))
            .sum::<u64>();
        let allocation = reserve_permanent(
            &self.context.limiter,
            emission_allocation_size(&[blob_size, topics_size]),
            "event",
        )?;

        let storage_fee = supervisor
            .shared_data
            .data_fees_limit
            .consume_event(blob_size, topics_count)
            .await
            .map_err(internal_trap)?
            .ok_or_else(|| {
                internal_trap(rt::errors::Error::vm(
                    abi::consts::VmError::out_of().receipt().event(),
                ))
            })?;

        self.context
            .data
            .accumulator
            .emissions
            .push(domain::ExecutionEmission::Event {
                topics: real_topics,
                blob,
                storage_fee,
            });
        allocation.commit();

        Ok(file_fd_none())
    }
    pub(super) async fn gl_call_emit_internal_message(
        &mut self,
        address: calldata::Address,
        calldata: abi::entry::MainCallData,
        value: primitive_types::U256,
        on: gl_call::On,
        use_balance: bool,
        fee_params: Option<abi::fees::InternalMessageParams>,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        log_debug!(
            recipient = address,
            on:? = on,
            use_balance = use_balance;
            "EmitInternalMessage dispatched"
        );
        if !self.context.data.conf.permissions.deterministic {
            log_debug!("EmitInternalMessage rejected: non-deterministic context (Forbidden)");
            return Err(generated::types::Errno::Forbidden.into());
        }
        if !self.context.data.conf.permissions.send_messages {
            log_debug!("EmitInternalMessage rejected: can_send_messages=false (Forbidden)");
            return Err(generated::types::Errno::Forbidden.into());
        }

        let balance_params = validate_balance_fee(
            self.context
                .data
                .conf
                .permissions
                .can_use_balance_for_message_fees,
            use_balance,
            fee_params,
        )?;

        let call_key = if let Some(method_name) = &calldata.name {
            abi::CallKey::for_method(method_name)
        } else {
            abi::CallKey::UNNAMED
        };

        if let Some(params) = balance_params {
            let mut enc = calldata::Encoder::new(calldata::CounterWriter(0));
            calldata::codec::Encode::encode(&calldata, &mut enc).unwrap_or_else(|e| match e {});
            let calldata_length = enc.into_inner().0;
            let rotations_size = usize_into_u64(params.rotations.len())
                .saturating_mul(memory_limiter_consts::MESSAGE_FEE_ROTATION_ELEMENT_SIZE.into());
            let allocation = reserve_permanent(
                &self.context.limiter,
                emission_allocation_size(&[calldata_length, rotations_size]),
                "internal message",
            )?;

            let my_balance = self
                .context
                .get_balance_impl(self.context.data.message_data.message.contract_address)
                .await?;
            let messages_value_decremented =
                self.context.data.accumulator.messages_value_decremented;

            let fees = consume_message_fee_internal(
                &self.context.data.supervisor.shared_data,
                FeeFunding::Balance {
                    value,
                    messages_value_decremented,
                    my_balance,
                },
                Arc::new(params.clone()),
                on,
                ConsumeInternalArgs {
                    is_deploy: false,
                    calldata_length,
                    code_length: 0,
                    subtree_length: 0,
                },
            )
            .await?;

            let metered_fee = fees.message_fee.reported_fee();

            // Empty subtree: use_balance nesting is fail-closed on-chain.
            self.context.data.accumulator.emissions.push(
                domain::ExecutionEmission::InternalMessage {
                    call_key,
                    address,
                    calldata,
                    value,
                    on,
                    message_fee: metered_fee,
                    receipt_fee: fees.receipt_fee.reported_fee(),
                    fee_params: params,
                    subtree: bytes::Bytes::new(),
                    use_balance: true,
                },
            );
            allocation.commit();

            self.context.data.accumulator.messages_value_decremented = messages_value_decremented
                .saturating_add(value)
                .saturating_add(metered_fee);

            return Ok(file_fd_none());
        }

        if !value.is_zero() {
            let my_balance = self
                .context
                .get_balance_impl(self.context.data.message_data.message.contract_address)
                .await?;

            if !checked_sum_le(
                value,
                self.context.data.accumulator.messages_value_decremented,
                my_balance,
            ) {
                return Err(generated::types::Errno::InsufficientBalance.into());
            }
        }

        let Some((matched_node, matched_params)) = self
            .context
            .data
            .accumulator
            .message_fee_allocation
            .iter_mut()
            .find_map(|node| {
                node.matches_internal(
                    convert_on_to_modules(on),
                    address,
                    convert_call_key_to_modules(call_key),
                )
                .map(|params| (node, params))
            })
        else {
            log_warn!(
                recipient = address,
                call_key:? = call_key,
                on:? = on;
                "no matching node for message fee allocation"
            );

            return Err(internal_trap(rt::errors::Error::vm(
                abi::consts::VmError::fee()
                    .no_matching_allocation()
                    .internal(),
            )));
        };

        log_debug!(
            recipient = address,
            call_key:? = call_key,
            on:? = on;
            "EmitInternalMessage matched fee allocation node"
        );

        let mut enc = calldata::Encoder::new(calldata::CounterWriter(0));
        calldata::codec::Encode::encode(&calldata, &mut enc).unwrap_or_else(|e| match e {});
        let calldata_length = enc.into_inner().0;

        let fee_params = convert_internal_message_params_to_sdk(matched_params.as_ref());
        let subtree = bytes::Bytes::from(
            genvm_modules_interfaces::fees::MessageAllocationNode::abi_encode(
                &matched_node.children,
            ),
        );
        let rotations_size = usize_into_u64(fee_params.rotations.len())
            .saturating_mul(memory_limiter_consts::MESSAGE_FEE_ROTATION_ELEMENT_SIZE.into());
        let allocation = reserve_permanent(
            &self.context.limiter,
            emission_allocation_size(&[
                calldata_length,
                subtree.len().into_int_comptime(),
                rotations_size,
            ]),
            "internal message",
        )?;

        let fees = consume_message_fee_internal(
            &self.context.data.supervisor.shared_data,
            FeeFunding::Allocation(matched_node),
            Arc::new(fee_params.clone()),
            on,
            ConsumeInternalArgs {
                is_deploy: false,
                calldata_length,
                code_length: 0,
                subtree_length: subtree.len().into_int_comptime(),
            },
        )
        .await?;

        self.context
            .data
            .accumulator
            .emissions
            .push(domain::ExecutionEmission::InternalMessage {
                call_key,
                address,
                calldata,
                value,
                on,
                message_fee: fees.message_fee.reported_fee(),
                receipt_fee: fees.receipt_fee.reported_fee(),
                fee_params,
                subtree,
                use_balance: false,
            });
        allocation.commit();

        log_debug!(
            depth = self.context.data.depth(),
            emissions_total = self.context.data.accumulator.emissions.len();
            "EmitInternalMessage emission pushed to accumulator"
        );

        self.context.data.accumulator.messages_value_decremented = self
            .context
            .data
            .accumulator
            .messages_value_decremented
            .saturating_add(value);

        Ok(file_fd_none())
    }
    pub(super) async fn gl_call_emit_internal_deploy_message(
        &mut self,
        calldata: abi::entry::MainDeployData,
        on: gl_call::On,
        fee_params: Option<abi::fees::InternalMessageParams>,
        args: EmitInternalDeployMessageArgs,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        let EmitInternalDeployMessageArgs {
            code,
            value,
            salt_nonce,
            use_balance,
        } = args;

        if !self.context.data.conf.permissions.deterministic {
            return Err(generated::types::Errno::Forbidden.into());
        }
        if !self.context.data.conf.permissions.send_messages {
            return Err(generated::types::Errno::Forbidden.into());
        }

        let balance_params = validate_balance_fee(
            self.context
                .data
                .conf
                .permissions
                .can_use_balance_for_message_fees,
            use_balance,
            fee_params,
        )?;

        if let Some(params) = balance_params {
            let code_length = code.len().into_int_comptime();
            let mut enc = calldata::Encoder::new(calldata::CounterWriter(0));
            calldata::codec::Encode::encode(&calldata, &mut enc).unwrap_or_else(|e| match e {});
            let calldata_length = enc.into_inner().0;
            let rotations_size = usize_into_u64(params.rotations.len())
                .saturating_mul(memory_limiter_consts::MESSAGE_FEE_ROTATION_ELEMENT_SIZE.into());
            let allocation = reserve_permanent(
                &self.context.limiter,
                emission_allocation_size(&[calldata_length, code_length, rotations_size]),
                "internal deploy message",
            )?;

            let my_balance = self
                .context
                .get_balance_impl(self.context.data.message_data.message.contract_address)
                .await?;
            let messages_value_decremented =
                self.context.data.accumulator.messages_value_decremented;

            let fees = consume_message_fee_internal(
                &self.context.data.supervisor.shared_data,
                FeeFunding::Balance {
                    value,
                    messages_value_decremented,
                    my_balance,
                },
                Arc::new(params.clone()),
                on,
                ConsumeInternalArgs {
                    is_deploy: true,
                    calldata_length,
                    code_length,
                    subtree_length: 0,
                },
            )
            .await?;

            let metered_fee = fees.message_fee.reported_fee();

            self.context.data.accumulator.emissions.push(
                domain::ExecutionEmission::InternalDeployMessage {
                    calldata,
                    code,
                    value,
                    on,
                    salt_nonce,
                    receipt_fee: fees.receipt_fee.reported_fee(),
                    message_fee: metered_fee,
                    fee_params: params,
                    subtree: bytes::Bytes::new(),
                    use_balance: true,
                },
            );
            allocation.commit();

            self.context.data.accumulator.messages_value_decremented = messages_value_decremented
                .saturating_add(value)
                .saturating_add(metered_fee);

            return Ok(file_fd_none());
        }

        if !value.is_zero() {
            let my_balance = self
                .context
                .get_balance_impl(self.context.data.message_data.message.contract_address)
                .await?;

            if !checked_sum_le(
                value,
                self.context.data.accumulator.messages_value_decremented,
                my_balance,
            ) {
                return Err(generated::types::Errno::InsufficientBalance.into());
            }
        }

        let Some((matched_node, matched_params)) = self
            .context
            .data
            .accumulator
            .message_fee_allocation
            .iter_mut()
            .find_map(|node| {
                node.matches_internal(
                    convert_on_to_modules(on),
                    calldata::Address::zero(),
                    convert_call_key_to_modules(abi::CallKey::DEPLOY),
                )
                .map(|params| (node, params))
            })
        else {
            log_warn!(
                recipient = calldata::Address::zero(),
                call_key:? = abi::CallKey::DEPLOY,
                on:? = on;
                "no matching node for message fee allocation"
            );

            return Err(internal_trap(rt::errors::Error::vm(
                abi::consts::VmError::fee()
                    .no_matching_allocation()
                    .internal(),
            )));
        };

        let code_length = code.len().into_int_comptime();
        let mut enc = calldata::Encoder::new(calldata::CounterWriter(0));
        calldata::codec::Encode::encode(&calldata, &mut enc).unwrap_or_else(|e| match e {});
        let calldata_length = enc.into_inner().0;

        let fee_params = convert_internal_message_params_to_sdk(matched_params.as_ref());
        let subtree = bytes::Bytes::from(
            genvm_modules_interfaces::fees::MessageAllocationNode::abi_encode(
                &matched_node.children,
            ),
        );
        let rotations_size = usize_into_u64(fee_params.rotations.len())
            .saturating_mul(memory_limiter_consts::MESSAGE_FEE_ROTATION_ELEMENT_SIZE.into());
        let allocation = reserve_permanent(
            &self.context.limiter,
            emission_allocation_size(&[
                calldata_length,
                code_length,
                subtree.len().into_int_comptime(),
                rotations_size,
            ]),
            "internal deploy message",
        )?;

        let fees = consume_message_fee_internal(
            &self.context.data.supervisor.shared_data,
            FeeFunding::Allocation(matched_node),
            Arc::new(fee_params.clone()),
            on,
            ConsumeInternalArgs {
                is_deploy: true,
                calldata_length,
                code_length,
                subtree_length: subtree.len().into_int_comptime(),
            },
        )
        .await?;

        self.context.data.accumulator.emissions.push(
            domain::ExecutionEmission::InternalDeployMessage {
                calldata,
                code,
                value,
                on,
                salt_nonce,
                receipt_fee: fees.receipt_fee.reported_fee(),
                message_fee: fees.message_fee.reported_fee(),
                fee_params,
                subtree,
                use_balance: false,
            },
        );
        allocation.commit();

        self.context.data.accumulator.messages_value_decremented = self
            .context
            .data
            .accumulator
            .messages_value_decremented
            .saturating_add(value);

        Ok(file_fd_none())
    }
}
