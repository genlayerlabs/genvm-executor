{
  ##############################################################################
  #                                                                            #
  #   ⚠  DEV SHELL ONLY — NO PACKAGING HERE.  ⚠                                #
  #                                                                            #
  #   This flake exists ONLY to provide the developer shell and the            #
  #   git-hooks (pre-commit / commit-msg) when this executor submodule is      #
  #   checked out and worked on standalone.                                    #
  #                                                                            #
  #   The executor is BUILT by the umbrella manager flake, which imports this  #
  #   directory through `default.nix`. DO NOT add packages / build outputs to  #
  #   this flake — put nothing here that the release build depends on.         #
  #                                                                            #
  ##############################################################################
  description = "genvm-executor dev shell + commit hooks (DEV ONLY — not a build entry point)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    git-hooks = {
      url = "github:cachix/git-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      git-hooks,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        hooks-python = pkgs.python312;
        # Self-contained docs toolchain (sphinx + extensions) for this line's
        # standalone docs sub-site, built via `nix develop .#gen-docs`. The
        # manager's support/ci/pipelines/docs.py enters this shell and drops the
        # output under build/doc/html/executors/<line>/.
        docs-python = pkgs.python312.withPackages (
          ps: with ps; [
            sphinx
            myst-parser
            pydata-sphinx-theme
            sphinxcontrib-mermaid
            # genlayer_embeddings autodoc imports numpy (not mocked).
            numpy
          ]
        );
        # cargo fmt per crate: each Cargo.toml picks up its own edition.
        cargo-fmt-all = pkgs.writeShellScript "cargo-fmt-all" ''
          set -euo pipefail
          export PATH="${pkgs.cargo}/bin:${pkgs.rustfmt}/bin:$PATH"
          rc=0
          while IFS= read -r toml; do
            ( cd "$(dirname -- "$toml")" && cargo fmt ) || rc=1
          done < <(git ls-files '*Cargo.toml')
          exit $rc
        '';
        pre-commit-check = git-hooks.lib.${system}.run {
          src = ./.;
          excludes = [
            "^build/"
            "^\\.direnv/"
          ];
          hooks = {
            # --- generic checks (mirror the manager's) ----------------
            trim-trailing-whitespace = {
              enable = true;
              excludes = [
                "^\\.git-third-party"
                "/fuzz/"
              ];
            };
            end-of-file-fixer = {
              enable = true;
              excludes = [
                "^\\.git-third-party"
                "/fuzz/"
              ];
            };
            check-added-large-files.enable = true;
            check-json = {
              enable = true;
              excludes = [ "^\\.git-third-party" ];
            };
            check-yaml.enable = true;
            check-toml.enable = true;
            check-merge-conflicts.enable = true;
            taplo.enable = true;
            editorconfig-checker = {
              enable = true;
              # text-only (the built-in defaults to every file): keep binary
              # blobs (models/*.onnx, test zips, ...) out of it, mirroring the
              # old engine's `text` pseudo-type.
              types = [ "text" ];
              excludes = [
                "\\.git-third-party"
                "runners/py-libs"
                "runners/genlayer-py-std/src-emb/onnx"
                "/fuzz/"
              ];
            };
            # --- executor-owned languages: python, c/c++, rust, nix ---
            ruff-format.enable = true;
            clang-format = {
              enable = true;
              # v19 to match the executor's historical clang-format pin.
              package = pkgs.llvmPackages_19.clang-tools;
              # Restrict to C/C++ only (the built-in also covers json/js/...).
              types_or = [
                "c"
                "c++"
              ];
              excludes = [
                "runners/softfloat/berkeley-softfloat-3"
                "runners/py-libs"
              ];
            };
            cargo-fmt = {
              enable = true;
              name = "cargo-fmt";
              entry = toString cargo-fmt-all;
              files = "\\.rs$";
              pass_filenames = false;
            };
            nixfmt = {
              enable = true;
              files = "\\.nix$";
            };
            markdown-local-links = {
              enable = true;
              name = "markdown-local-links";
              entry = "${hooks-python}/bin/python3 support/scripts/md-local-links.py";
              types = [ "markdown" ];
            };
            check-source-text = {
              enable = true;
              name = "check-source-text";
              entry = "${hooks-python}/bin/python3 support/scripts/check-source-text.py";
              files = "\\.rs$";
            };
            # --- local guards -----------------------------------------
            # Refuse commits that leave dev-mode on or any hash as "test".
            no-commit-dev-mode = {
              enable = true;
              name = "no-commit-dev-mode";
              entry = "${hooks-python}/bin/python3 runners/support/versions/check-dev-mode.py";
              files = "^runners/support/versions/(dev-mode|current)\\.nix$";
              pass_filenames = false;
            };
            # --- commit-msg -------------------------------------------
            check-commit-message = {
              enable = true;
              name = "check-commit-message";
              entry = "${hooks-python}/bin/python3 support/scripts/check-commit-message.py --message-file";
              stages = [ "commit-msg" ];
            };
          };
        };
      in
      {
        checks.pre-commit-check = pre-commit-check;
        # `nix fmt` runs the same generated hook config as `nix flake check`,
        # in the working tree (so the executor's cross-repo cargo-fmt paths
        # resolve — the executor must be checked out under the manager).
        formatter = pkgs.writeShellScriptBin "pre-commit-run" ''
          exec ${pre-commit-check.config.package}/bin/pre-commit run --all-files --config ${pre-commit-check.config.configFile}
        '';
        devShells.default = pkgs.mkShell {
          # pre-commit + the hook tools, so `pre-commit run` works in-shell.
          packages = pre-commit-check.enabledPackages;
          # Standalone dev shell: installs the git-hooks stubs into .git/hooks.
          shellHook = pre-commit-check.shellHook;
        };
        devShells.gen-docs = pkgs.mkShell {
          # Standalone sphinx toolchain for this line's docs sub-site (mmdc
          # backs the sphinxcontrib.mermaid svg output). The text→api.txt merge
          # is plain python (docs/website/merge_txts.py), run by the manager
          # pipeline, so no ruby here.
          packages = [
            docs-python
            pkgs.mermaid-cli
          ];
        };
      }
    );
}
