# Wasmtime maintenance automation

The default executor branch coordinates Wasmtime maintenance for every line in
`wasmtime-maintenance.json`. GitHub schedules only execute from the repository's
default branch, so the workflow reads the configured `vX-dev` and `vX.x` refs
through the GitHub API instead of assuming its own checkout represents every
supported line.

## Ownership

Set the repository Actions variable `WASMTIME_REBASE_OWNER` to the GitHub login
of the person accountable for Wasmtime rebases. Advisory and monthly review
issues are assigned to that login. If the variable is absent, monitoring still
runs and creates issues, but emits an Actions warning and leaves them unassigned.

## Policies

- `monthly`: published-advisory monitoring plus one scheduled rebase review
  issue per calendar month.
- `security-only`: published-advisory monitoring without routine upgrade churn.

Both the development and release refs are checked. This distinguishes a line
that is vulnerable everywhere from one that is fixed on `vX-dev` but still
needs promotion to `vX.x`.

## Remediation flow

The automation never creates branches, pushes commits, or merges changes.
Remediation follows the existing manager-linked branch strategy:

1. Create a feature branch from the current manager `vX-dev`.
2. Rebase the affected executor line in its submodule.
3. Push the executor mirror as `pr/<executor-line>/<manager-feature>`.
4. Open the manager PR and its linked executor PR into `<executor-line>-dev`.
5. Run the full executor, upstream Wasmtime, and cross-repository checks.
6. Promote the fixed development line through the standing release gate into
   `<executor-line>.x`.

Do not patch a protected development or release branch directly.
