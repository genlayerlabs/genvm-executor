use genlayer_sdk::abi;
use wiggle::error::Context as _;

use crate::rt;
use genvm_common::*;

pub(crate) const CONTRACT_WASM_FEATURES: wasmparser::WasmFeatures =
    wasmparser::WasmFeatures::WASM2.union(wasmparser::WasmFeatures::TAIL_CALL);

fn is_nondeterministic_float_operator(operator: &wasmparser::Operator<'_>) -> bool {
    use wasmparser::Operator::*;

    matches!(
        operator,
        F32Abs
            | F32Neg
            | F32Ceil
            | F32Floor
            | F32Trunc
            | F32Nearest
            | F32Sqrt
            | F32Add
            | F32Sub
            | F32Mul
            | F32Div
            | F32Min
            | F32Max
            | F32Copysign
            | F64Abs
            | F64Neg
            | F64Ceil
            | F64Floor
            | F64Trunc
            | F64Nearest
            | F64Sqrt
            | F64Add
            | F64Sub
            | F64Mul
            | F64Div
            | F64Min
            | F64Max
            | F64Copysign
            | F32Eq
            | F32Ne
            | F32Lt
            | F32Gt
            | F32Le
            | F32Ge
            | F64Eq
            | F64Ne
            | F64Lt
            | F64Gt
            | F64Le
            | F64Ge
            | I32TruncF32S
            | I32TruncF32U
            | I32TruncF64S
            | I32TruncF64U
            | I64TruncF32S
            | I64TruncF32U
            | I64TruncF64S
            | I64TruncF64U
            | F32ConvertI32S
            | F32ConvertI32U
            | F32ConvertI64S
            | F32ConvertI64U
            | F32DemoteF64
            | F64ConvertI32S
            | F64ConvertI32U
            | F64ConvertI64S
            | F64ConvertI64U
            | F64PromoteF32
            | I32TruncSatF32S
            | I32TruncSatF32U
            | I32TruncSatF64S
            | I32TruncSatF64U
            | I64TruncSatF32S
            | I64TruncSatF32U
            | I64TruncSatF64S
            | I64TruncSatF64U
            | F32x4Eq
            | F32x4Ne
            | F32x4Lt
            | F32x4Gt
            | F32x4Le
            | F32x4Ge
            | F64x2Eq
            | F64x2Ne
            | F64x2Lt
            | F64x2Gt
            | F64x2Le
            | F64x2Ge
            | F32x4Ceil
            | F32x4Floor
            | F32x4Trunc
            | F32x4Nearest
            | F32x4Abs
            | F32x4Neg
            | F32x4Sqrt
            | F32x4Add
            | F32x4Sub
            | F32x4Mul
            | F32x4Div
            | F32x4Min
            | F32x4Max
            | F32x4PMin
            | F32x4PMax
            | F64x2Ceil
            | F64x2Floor
            | F64x2Trunc
            | F64x2Nearest
            | F64x2Abs
            | F64x2Neg
            | F64x2Sqrt
            | F64x2Add
            | F64x2Sub
            | F64x2Mul
            | F64x2Div
            | F64x2Min
            | F64x2Max
            | F64x2PMin
            | F64x2PMax
            | I32x4TruncSatF32x4S
            | I32x4TruncSatF32x4U
            | F32x4ConvertI32x4S
            | F32x4ConvertI32x4U
            | I32x4TruncSatF64x2SZero
            | I32x4TruncSatF64x2UZero
            | F64x2ConvertLowI32x4S
            | F64x2ConvertLowI32x4U
            | F32x4DemoteF64x2Zero
            | F64x2PromoteLowF32x4
            | I32x4RelaxedTruncF32x4S
            | I32x4RelaxedTruncF32x4U
            | I32x4RelaxedTruncF64x2SZero
            | I32x4RelaxedTruncF64x2UZero
            | F32x4RelaxedMadd
            | F32x4RelaxedNmadd
            | F64x2RelaxedMadd
            | F64x2RelaxedNmadd
            | F32x4RelaxedMin
            | F32x4RelaxedMax
            | F64x2RelaxedMin
            | F64x2RelaxedMax
    )
}

pub(crate) fn validate_contract_wasm(wasm: &[u8]) -> wasmtime::Result<()> {
    let mut validator = wasmparser::Validator::new_with_features(CONTRACT_WASM_FEATURES);
    validator.validate_all(wasm).with_context(|| {
        format!(
            "validating {}",
            &String::from_utf8_lossy(&wasm[..10.min(wasm.len())])
        )
    })?;

    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        let wasmparser::Payload::CodeSectionEntry(body) = payload? else {
            continue;
        };
        let mut operators = body.get_operators_reader()?;
        while !operators.eof() {
            let offset = operators.original_position();
            let operator = operators.read()?;
            if is_nondeterministic_float_operator(&operator) {
                return Err(wasmtime::Error::msg(format!(
                    "nondeterministic floating-point operator {operator:?} at offset {offset}"
                )));
            }
        }
    }

    Ok(())
}

impl super::Supervisor {
    pub fn validate_wasm(&self, wasm: &[u8]) -> wasmtime::Result<()> {
        validate_contract_wasm(wasm)
    }

    pub async fn compile_wasm(
        &self,
        wasm: &[u8],
        debug_path: &str,
    ) -> wasmtime::Result<rt::DetNondet<wasmtime::Module>> {
        log_debug!(path = debug_path; "compilation");
        self.shared_data
            .metrics
            .supervisor
            .compiled_modules
            .increment();
        let tok = stats::tracker::Time::new(
            self.shared_data
                .gep(|x| &x.metrics.supervisor.compilation_time),
        );

        if let Err(validate) = self.validate_wasm(wasm) {
            return Err(rt::errors::Error::wrap(
                abi::consts::VmError::invalid_contract().wasm().validating(),
                validate,
            )
            .into());
        }

        let start_time = std::time::Instant::now();
        let module_det = wasmtime::CodeBuilder::new(&self.engines.det)
            .wasm_binary(
                std::borrow::Cow::Borrowed(wasm),
                Some(std::path::Path::new(debug_path)),
            )?
            .compile_module();

        let module_det = match module_det {
            Ok(v) => v,
            Err(e) => {
                return Err(rt::errors::Error::wrap(
                    abi::consts::VmError::invalid_contract().wasm().validating(),
                    e,
                )
                .into());
            }
        };

        let module_non_det = wasmtime::CodeBuilder::new(&self.engines.non_det)
            .wasm_binary(
                std::borrow::Cow::Borrowed(wasm),
                Some(std::path::Path::new(debug_path)),
            )?
            .compile_module();

        let module_non_det = match module_non_det {
            Ok(v) => v,
            Err(e) => {
                return Err(rt::errors::Error::wrap(
                    abi::consts::VmError::invalid_contract().wasm().validating(),
                    e,
                )
                .into());
            }
        };

        log_info!(status = "done", duration:? = start_time.elapsed(), path = debug_path; "cache compiling");

        std::mem::drop(tok);
        Ok(rt::DetNondet {
            det: module_det,
            non_det: module_non_det,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::validate_contract_wasm;

    fn validate(wat: &str) -> wasmtime::Result<()> {
        let wasm = wat::parse_str(wat).map_err(|error| wasmtime::Error::msg(error.to_string()))?;
        validate_contract_wasm(&wasm)
    }

    #[test]
    fn deterministic_policy_allows_float_bit_transport() {
        validate(
            r#"
                (module
                    (memory 1)
                    (func (param i32) (result i32)
                        local.get 0
                        f32.load
                        i32.reinterpret_f32
                    )
                    (func (param i32) (result i64)
                        local.get 0
                        f64.load
                        i64.reinterpret_f64
                    )
                    (func (param i32 f32)
                        local.get 0
                        local.get 1
                        f32.store
                    )
                    (func (param f32) (result v128)
                        local.get 0
                        f32x4.splat
                    )
                    (func (param v128) (result f32)
                        local.get 0
                        f32x4.extract_lane 0
                    )
                    (func (param v128 f32) (result v128)
                        local.get 0
                        local.get 1
                        f32x4.replace_lane 0
                    )
                    (func (result f32)
                        f32.const 1.0
                    )
                    (func (result f64)
                        f64.const 1.0
                    )
                    (func (param f64) (result v128)
                        local.get 0
                        f64x2.splat
                    )
                    (func (param v128 f64) (result v128)
                        local.get 0
                        local.get 1
                        f64x2.replace_lane 0
                    )
                    (func (param v128) (result f64)
                        local.get 0
                        f64x2.extract_lane 0
                    )
                )
            "#,
        )
        .unwrap();
    }

    #[test]
    fn deterministic_policy_rejects_scalar_float_computation() {
        let error = validate(
            r#"
                (module
                    (func (param f32 f32) (result f32)
                        local.get 0
                        local.get 1
                        f32.add
                    )
                )
            "#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("F32Add"), "{error:#}");
    }

    #[test]
    fn deterministic_policy_rejects_simd_float_computation() {
        let error = validate(
            r#"
                (module
                    (func (param v128 v128) (result v128)
                        local.get 0
                        local.get 1
                        f64x2.mul
                    )
                )
            "#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("F64x2Mul"), "{error:#}");
    }

    #[test]
    fn contract_feature_set_rejects_memory64() {
        let error = validate("(module (memory i64 1))").unwrap_err();
        assert!(format!("{error:#}").contains("memory64"), "{error:#}");
    }
}
