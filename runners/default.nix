# v0.2.x runners: this line does NOT build runners from source. v0.2.16 is a
# frozen legacy line, so we ship the runners exactly as they were released —
# fetch `genvm-universal.tar.xz` and hand each contained runner tar back to the
# umbrella unchanged.
#
# Contract (see ../../../runners/all.nix): this file is imported with
#   { host-system, deps, pkgs-overlays }
# and must return [ { id; uid; derivation; } ]. The umbrella tags each entry
# with this line's `executor-version` (v0.2.16) and lays the tars back out into
# its own `runners/<id>/<aa>/<rest>.tar` tree, so the only thing we owe it is the
# id, the `<id>:<hash32>` uid, and a path to the tar.
#
# `index.json` (committed) lists every runner's id + hash32; it is generated from
# the tarball so the manifest evaluates without import-from-derivation. The
# 200MB tarball itself is fetched at build time and is NOT committed.
{
  pkgs-overlays ? [ ],
  ...
}:
let
  pkgs = import (builtins.fetchGit {
    url = "https://github.com/NixOS/nixpkgs";
    rev = "2ff43b1d533641116f1740158d121013036a7f74";
    shallow = true;
  }) {
    system = "x86_64-linux";
    overlays = pkgs-overlays;
  };

  index = builtins.fromJSON (builtins.readFile ./index.json);

  tarball = pkgs.fetchurl {
    url = "https://github.com/genlayerlabs/genvm/releases/download/v0.2.16/genvm-universal.tar.xz";
    hash = "sha256-Tws1jsmOwUi+m5XN+w8OGmy+ZNoBlP36w//8b10dk+I=";
  };

  # Unpack once into a single store path holding the `runners/...` tree.
  unpacked = pkgs.stdenvNoCC.mkDerivation {
    name = "genvm-v0.2.16-universal-runners";
    src = tarball;
    sourceRoot = ".";
    dontConfigure = true;
    dontBuild = true;
    dontFixup = true;
    installPhase = ''
      mkdir -p "$out"
      cp -r runners "$out/"
    '';
  };
in
builtins.map (
  e:
  let
    aa = builtins.substring 0 2 e.hash32;
    rest = builtins.substring 2 50 e.hash32;
  in
  {
    id = e.id;
    uid = "${e.id}:${e.hash32}";
    derivation = "${unpacked}/runners/${e.id}/${aa}/${rest}.tar";
  }
) index
