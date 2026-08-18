use std::sync::Arc;

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum WasmMode {
    Det,
    Nondet,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub enum InitAction {
    MapFile {
        to: Arc<str>,
        file: Arc<str>,
    },
    AddEnv {
        name: String,
        val: String,
    },
    SetArgs(Vec<String>),
    Depends(String),
    LinkWasm(Arc<str>),
    StartWasm(Arc<str>),

    When {
        cond: WasmMode,
        action: Box<InitAction>,
    },
    Seq(Vec<InitAction>),

    With {
        runner: String,
        action: Box<InitAction>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runner_action_accepts_schema_annotation() {
        let parsed = serde_json::from_str::<InitAction>(
            r#"{
                "$schema": "https://raw.githubusercontent.com/genlayerlabs/genvm/refs/heads/main/doc/schemas/runner.json",
                "StartWasm": "file"
            }"#,
        );

        assert!(
            matches!(parsed, Ok(InitAction::StartWasm(ref path)) if path.as_ref() == "file"),
            "runner.json permits the optional `$schema` annotation; got {parsed:?}"
        );
    }

    #[test]
    fn runner_action_rejects_unknown_payload_fields() {
        let parsed = serde_json::from_str::<InitAction>(
            r#"{
                "MapFile": {
                    "file": "contract.py",
                    "to": "/contract.py",
                    "destination": "/silently-ignored.py"
                }
            }"#,
        );

        assert!(
            parsed.is_err(),
            "unknown runner action fields must be rejected instead of silently ignored; got {parsed:?}"
        );
    }
}
