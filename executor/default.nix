{
  pkgs,
  root-src,
  compile-rust,
  get-root-subtree,
  build-config,
  patch-yaml-schema,
  patch-rpath,
  host-system,
  host-system-as-genvm,
  compiled-libs,
  # Mount prefix of this executor checkout inside the manager full-src tree.
  # The executor is built by the genvm-manager umbrella, which mounts it at
  # executors/v0.3.x ; get-root-subtree paths must be prefixed accordingly.
  exec-prefix ? "executors/v0.3.x",
  ...
}@args:
let
  # Release manifests (latest.json/all.json) are produced by the umbrella's
  # runner machinery, which owns the accumulate-and-build logic. This executor
  # checkout is mounted at <umbrella>/executors/v0.3.x, so the umbrella root
  # (and its ./runners) is three levels up. `all.json` carries every runner
  # compatible up to our version; `latest.json` our own current runners.
  manifests-data = import ../../../runners/views/release.nix (
    args // { executorVersion = build-config.executor-version; }
  );

  lib = pkgs.lib;
  make-for-target =
    target:
    let
      exe = compile-rust rec {
        inherit target;

        pname = "genvm-executor";
        version = build-config.executor-version;

        cargoLock.lockFile = ./Cargo.lock;

        src = get-root-subtree [
          "${exec-prefix}/executor/src"
          # host<->executor interface and schemas live in the manager root
          "crates/modules-interfaces"
          "${exec-prefix}/executor/crates"
          "${exec-prefix}/executor/third-party"
          "${exec-prefix}/executor/Cargo.toml"
          "${exec-prefix}/executor/Cargo.lock"
          "docs/schemas"
        ];
        sourceRoot = "./source/${exec-prefix}/executor";

        extraLibs = compiled-libs.${target};

        GENVM_PROFILE = build-config.executor-version;
      };
    in
    pkgs.stdenvNoCC.mkDerivation rec {
      name = "genvm-executor-${target}";

      srcs = [
        exe
        ./install
      ];

      dontUnpack = true;
      dontConfigure = true;
      dontBuild = true;

      nativeBuildInputs = [
        pkgs.makeWrapper
        patch-yaml-schema
        patch-rpath
      ];

      installPhase = ''
        mkdir -p $out/executor/${build-config.executor-version}/bin

        mkdir -p $out/executor/${build-config.executor-version}/data
        cp "${manifests-data.latest}" "$out/executor/${build-config.executor-version}/data/latest.json"
        cp "${manifests-data.all}" "$out/executor/${build-config.executor-version}/data/all.json"

        cp "${exe}" "$out/executor/${build-config.executor-version}/bin/genvm"
        for src in $srcs; do
        if [[ "$src" != "${exe}" ]]
        then
        cp --no-preserve=ownership -r "$src/." "$out/executor/${build-config.executor-version}/."
        fi
        done

        chmod -R u+w "$out"
        patch-yaml-schema --tag ${build-config.executor-version} "$out"

        patch-rpath --codesign \
        --rpath '$ORIGIN/../lib' \
        --rpath '$ORIGIN/../../../lib' \
        "$out/executor/${build-config.executor-version}/bin/genvm"
      '';
    };
in
{
  executor = make-for-target host-system-as-genvm;

  executor-amd64-linux = make-for-target "amd64-linux";
  executor-arm64-linux = make-for-target "arm64-linux";
  executor-arm64-macos = make-for-target "arm64-macos";
}
