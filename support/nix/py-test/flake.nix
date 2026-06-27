{
  description = "genlayer-py-std unit-test Python environment (pinned, independent flake)";

  inputs = {
    # A pinned *stable* channel (not nixos-unstable like the sibling precommit
    # flake): Hydra fully builds and binary-caches stable, so torch / transformers
    # and the rest are fetched, not compiled from source.
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        python = pkgs.python312;
        # The test/dev dependencies that used to live in
        # genlayer-py-std/pyproject.toml's [tool.poetry.group.dev.dependencies].
        # The genlayer / genlayer_embeddings / onnx packages under test are NOT
        # installed here: the pytest plugin imports them straight from src/ +
        # src-emb/ via PYTHONPATH (see tests/runner/genvm_tool_plugins/pytest.py),
        # mirroring the editable install poetry used to provide.
        pyEnv = python.withPackages (ps: [
          ps.pytest
          ps.pytest-xdist
          ps.pytest-cov
          ps.numpy
          ps.eth-abi
          # The embeddings tests compare genlayer_embeddings against the HF
          # reference via transformers' AutoTokenizer/AutoModel (+ torch). The
          # poetry pin listed `sentence-transformers`, but only the `transformers`
          # API and the model-name string are actually used, so we depend on
          # transformers + torch directly and skip sentence-transformers' heavy
          # datasets/evaluate tail.
          ps.transformers
          # CPU torch from the pinned stable channel (below): Hydra builds and
          # caches the CPU (unfree-free) torch for stable, so this is a plain
          # fetch — no multi-hour source compile, no unfree CUDA wheel.
          ps.torch
          # NB: python-afl (the old `py_fuzz` plugin's `py-afl-fuzz`) is not
          # packaged in this nixpkgs and the fuzz collector is disabled, so it
          # is intentionally omitted. Add it back here if fuzzing is revived.
        ]);
      in
      {
        # `nix build path:<dir>` (the pytest plugin / CI) realises `default`;
        # its bin/ carries `pytest` and `python3`.
        packages.default = pyEnv;
        packages.python = pyEnv;
      }
    );
}
