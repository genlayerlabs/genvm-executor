# Runners this line no longer builds, kept so contracts that pinned them still
# resolve. Each `<executor-version>.tar.xz` holds `<id>/<aa>/<rest>.zip` files;
# `hash` pins the bytes extracted from it, as `current.nix` does for built ones.
{
  pkgs-host,
  runnersLib,
  ...
}:
let
  archives = {
    "v0.3.0-rc10" = [
      {
        id = "py-lib-genlayer-std";
        hash = "sha256-+d+0LvZ2pD5WTUZqXJevvtw8fRWmrkrbjxXbuuSn/KE=";
      }
      {
        id = "py-genlayer";
        hash = "sha256-WpfLJ0M8uJgSQlsZCoSQN0/U5C87c0id1N/OMtvk/1Q=";
      }
      {
        id = "py-genlayer-multi";
        hash = "sha256-E83q9vT/4BbRmVSLQUwrCuFnB8NBQePuNEUE53GbgEY=";
      }
    ];
  };

  extract =
    archive:
    { id, hash }:
    let
      hash32 = runnersLib.hashToIDHash hash;
      member = "${id}/${builtins.substring 0 2 hash32}/${builtins.substring 2 50 hash32}.zip";
    in
    {
      inherit id hash;
      uid = "${id}:${hash32}";
      archived = true;
      available-after = null;
      supersedes = null;

      derivation = pkgs-host.runCommand "genvm_archived_runner_${id}_${hash32}" {
        nativeBuildInputs = [ pkgs-host.xz ];
        outputHashMode = "flat";
        outputHash = hash;
      } "tar -xJOf ${./. + "/${archive}.tar.xz"} ${member} > $out";
    };
in
builtins.concatLists (
  builtins.attrValues (builtins.mapAttrs (archive: builtins.map (extract archive)) archives)
)
