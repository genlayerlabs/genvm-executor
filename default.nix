# Package entry for this executor version line (v0.3.x).
#
# Replaces the old root flake.nix. The executor is intentionally NOT standalone:
# the genvm-manager umbrella imports this with its build arguments (compile-rust,
# get-root-subtree, compiled-libs, ...). See the manager's flake.nix, which maps
# `import "${exec-src}" (release-args // { inherit compiled-libs; })` per line.
#
# This line's manifest.json is the source of truth for its executor-version (and
# available-after); we read it here as `build-config` rather than having the
# umbrella thread one in, so the version always tracks the checkout.
#
# Returns the executor package set: { executor, executor-amd64-linux, ... }.
args:
import ./executor (
  args
  // {
    build-config = builtins.fromJSON (builtins.readFile ./manifest.json);
  }
)
