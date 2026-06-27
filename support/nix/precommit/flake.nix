{
  description = "genvm-executor pre-commit lint tools (pinned, independent flake)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
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
        # One named output per tool: hooks.toml `nix = "<name>"` references
        # these, and they are what `default` aggregates.
        tools = {
          inherit (pkgs)
            ruff
            editorconfig-checker
            rustfmt
            nixfmt
            cargo
            ;
          pre-commit-hooks = pkgs.python3Packages.pre-commit-hooks;
          # v19 to match the executor's historical clang-format pin.
          clang-tools = pkgs.llvmPackages_19.clang-tools;
        };
      in
      {
        packages = tools // {
          default = pkgs.buildEnv {
            name = "genvm-precommit-tools-executor";
            ignoreCollisions = true;
            paths = builtins.attrValues tools;
          };
        };
      }
    );
}
