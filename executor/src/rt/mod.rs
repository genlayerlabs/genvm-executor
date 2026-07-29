pub mod errors;
pub mod fees;
pub mod memlimiter;
pub mod supervisor;
pub mod vm;

use std::sync::Arc;

enum SpawnErrorState {
    Spawned(vm::VMBase),
    Unspawned(Box<wasi::genlayer_sdk::SingleVMData>),
}

pub struct SpawnError {
    error: anyhow::Error,
    state: Box<SpawnErrorState>,
}

#[derive(Default, Debug, serde::Serialize, genlayer_calldata::Encode)]
pub struct Metrics {
    precompile_hits: genvm_common::stats::metric::Count,
    compiled_modules: genvm_common::stats::metric::Count,
    compilation_time: genvm_common::stats::metric::Time,
}

pub struct DetNondet<T> {
    pub det: T,
    pub non_det: T,
}

impl<T> DetNondet<T> {
    pub fn get(&self, is_det: bool) -> &T {
        if is_det {
            &self.det
        } else {
            &self.non_det
        }
    }

    pub fn get_mut(&mut self, is_det: bool) -> &mut T {
        if is_det {
            &mut self.det
        } else {
            &mut self.non_det
        }
    }
}

pub struct DetFuelBudget(tokio::sync::Mutex<Option<primitive_types::U256>>);

impl DetFuelBudget {
    pub fn new(initial: Option<primitive_types::U256>) -> Self {
        Self(tokio::sync::Mutex::new(initial))
    }

    async fn remaining(&self, host_remaining: primitive_types::U256) -> primitive_types::U256 {
        match *self.0.lock().await {
            Some(imported) => imported.min(host_remaining),
            None => host_remaining,
        }
    }

    async fn consume(&self, consumed: primitive_types::U256) {
        let mut budget = self.0.lock().await;
        if let Some(remaining) = budget.as_mut() {
            *remaining = remaining.saturating_sub(consumed);
        }
    }
}

/// basic data that is shared across all VMs
pub struct SharedData {
    pub is_sync: bool,
    pub genvm_id: genvm_modules_interfaces::GenVMId,
    pub debug_mode: genvm_common::DebugMode,
    pub metrics: crate::Metrics,
    pub data_fees_limit: fees::DataLimit,
    pub det_fuel_budget: DetFuelBudget,
    pub llm_consumption: tokio::sync::Mutex<primitive_types::U256>,
}

impl SharedData {
    pub async fn remaining_det_fuel(
        &self,
        host_remaining: primitive_types::U256,
    ) -> primitive_types::U256 {
        self.det_fuel_budget.remaining(host_remaining).await
    }

    pub async fn consume_det_fuel(&self, consumed: primitive_types::U256) {
        self.det_fuel_budget.consume(consumed).await;
    }
}

pub fn parse_host_data(
    zelf: &genvm_modules_interfaces::ExecutionData,
) -> anyhow::Result<genvm_modules_interfaces::HostData> {
    serde_json::from_str(&zelf.host_data)
        .with_context(|| "parsing host_data from execution context")
}

pub async fn spawn_apply_run(
    supervisor: &Arc<supervisor::Supervisor>,
    vm: Box<wasi::genlayer_sdk::SingleVMData>,
) -> std::result::Result<vm::RunResult, anyhow::Error> {
    match spawn_apply_run_inner(supervisor, vm).await {
        Ok(res) => Ok(res),
        Err(SpawnError { error, state }) => {
            let (wasm_store_hashes, vm_data) = match *state {
                SpawnErrorState::Spawned(mut vm_base) => (
                    vm_base.wasm_store_hashes(),
                    Box::new(vm_base.store.into_data().genlayer_ctx.genlayer_sdk.data),
                ),
                SpawnErrorState::Unspawned(vm_data) => (Default::default(), vm_data),
            };

            log_debug!(error:ah = error; "spawn_apply_run failed");

            match errors::unwrap_vm_errors_backtrace(errors::UnwrapDynError::from(error)) {
                Ok((run_ok, backtrace)) => Ok(vm::RunResult {
                    run_ok,
                    backtrace,
                    wasm_store_hashes,
                    vm_data,
                }),
                Err(e) => Err(e),
            }
        }
    }
}

async fn spawn_apply_run_inner(
    supervisor: &Arc<supervisor::Supervisor>,
    vm: Box<wasi::genlayer_sdk::SingleVMData>,
) -> std::result::Result<vm::RunResult, SpawnError> {
    let limiter = supervisor.limiter.get(vm.conf.is_deterministic).derived();

    let vm = supervisor::spawn(supervisor, vm, limiter).await?;

    let vm = supervisor::apply_contract_actions(supervisor, vm).await?;

    vm.run().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn imported_deterministic_fuel_is_the_initial_budget() {
        let budget = DetFuelBudget::new(Some(primitive_types::U256::from(10)));
        assert_eq!(
            budget.remaining(primitive_types::U256::from(20)).await,
            primitive_types::U256::from(10)
        );
        budget.consume(primitive_types::U256::from(3)).await;
        assert_eq!(
            budget.remaining(primitive_types::U256::from(20)).await,
            primitive_types::U256::from(7)
        );
    }
}

use anyhow::Context;
use genvm_common::log_debug;

use crate::wasi;
