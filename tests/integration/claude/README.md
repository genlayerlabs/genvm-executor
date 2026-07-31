# Agentic fuzzing

What survives between fuzzing sessions:

- `intelligence/` — what has been explored and what was ruled out
- `example/` — the reference shape for a probe

Probes themselves are throwaway and live in `_scratch/`, which is gitignored.
The procedure is the manager's `agentic-fuzzing` skill
(`.claude/skills/agentic-fuzzing/SKILL.md`).
