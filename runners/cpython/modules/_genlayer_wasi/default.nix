{
  pkgs,
  pythonObjs,
  stdenvNoCC,
  runnersLib,
  lib,
  pkgs-host,
  ...
}:
let
  genlayer_c = pkgs-host.writeText "genlayer.c" (builtins.readFile ./genlayer.c);
  extraObj = stdenvNoCC.mkDerivation {
    name = "genvm-cpython-mod-genlayer-objs";
    outputHashMode = "recursive";
    outputHash = "sha256-uJU7a6c7ocdIyJq37LROW/89XB42mzQuH28UwUwVaRY=";

    deps = [ genlayer_c ];

    src = pythonObjs;

    phases = [
      "unpackPhase"
      "buildPhase"
      "installPhase"
    ];

    nativeBuildInputs = [
      runnersLib.wasi-sdk.package
    ];

    postUnpack = ''
      cp "${genlayer_c}" ./genlayer.c
    '';

    buildPhase = ''
      ${runnersLib.wasi-sdk.env.CC} ${runnersLib.wasi-sdk.env.CFLAGS} -Wall -Wextra -Wpedantic -Werror -Wno-unused-parameter -I ./include/python3.13 -c -o genlayer.o ../genlayer.c
    '';

    installPhase = ''
      mkdir -p "$out/obj"
      cp ./genlayer.o "$out/obj/"
    '';
  };
in
{
  runners = [ ];
  extraObjs = [ extraObj ];

  setupLines = [
    "_genlayer_wasi"
  ];
}
