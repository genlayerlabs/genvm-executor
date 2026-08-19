# importing this file ({} no args) results in:
# [{
#   id
#   hash
#   uid
#   derivation # zip file
# }]
# all args are optional
{
  host-system ? "x86_64-linux",
  deps ? null,
  pkgs-overlays ? [ ],
  ...
}:
let
  pkgs-pure = (
    builtins.fetchGit {
      url = "https://github.com/NixOS/nixpkgs";
      rev = "2ff43b1d533641116f1740158d121013036a7f74";
      shallow = true;
    }
  );
  pkgs = import pkgs-pure {
    system = "x86_64-linux";
    overlays = pkgs-overlays;
  };
  pkgs-host = import pkgs-pure {
    system = host-system;
    overlays = pkgs-overlays;
  };
  runnersLib = import ./support args;

  # The dep set is owned by the genvm-manager umbrella (see its flake.nix /
  # libs/deps) and passed down; the executor is intentionally not standalone.
  real-deps =
    if deps == null then
      throw "executor runners require `deps` from the genvm-manager umbrella (libs/deps); the executor is not standalone"
    else
      deps;

  args = {
    inherit pkgs pkgs-host runnersLib;
    inherit (pkgs) lib stdenvNoCC;

    deps = real-deps;
  };
in
(import ./py-libs args)
++ (import ./genlayer-py-std args)
++ (import ./softfloat args)
++ (import ./cpython args)
++ (import ./models args)
++ [ ]
