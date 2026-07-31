# Example probe

The reference shape for a probe: deploy A, deploy B, then call a write method on
B that reads from A. The jsonnet is written out by hand on top of
`templates/util.jsonnet` rather than `templates/simple.jsonnet`, because a probe
is usually more than one step.

Every step carries the three fields a probe needs —
`expected_semantics_components: []`, `modes: 'lvs'`, `stable_hash: false` — and
the top-level object carries `tags: ["stable", ...]`, without which the case
asks for LLM keys and a webdriver it does not need.

The procedure is the manager's `agentic-fuzzing` skill.
