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
  release-src = get-root-subtree [
    "${exec-prefix}/executor/install"
    "${exec-prefix}/executor/registry"
  ];

  # v0.2.x is a frozen legacy line, so its runner registry never changes: rather
  # than run it through the umbrella's accumulate-and-filter machinery (which is
  # built for forward-rolling lines and whose `latest` would pick the wrong hash
  # per id — it has no per-runner version metadata to order them), we ship the
  # exact `all.json`/`latest.json` from the v0.2.16 release, committed under
  # `./registry`. They list precisely the runners in `genvm-universal.tar.xz`
  # (see ../runners/index.json), which is what this line fetches. `all.json` is
  # consulted to accept a requested runner hash; `latest.json` resolves an id to
  # its newest runner in debug mode (used by e.g. `Depends: py-genlayer:test`).
  manifests-data = {
    all = release-src + "/${exec-prefix}/executor/registry/all.json";
    latest = release-src + "/${exec-prefix}/executor/registry/latest.json";
  };

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
          # host<->executor interface and shared calldata live in the manager root
          "crates/modules-interfaces"
          "crates/calldata"
          "crates/calldata-derive"
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
        (release-src + "/${exec-prefix}/executor/install")
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
