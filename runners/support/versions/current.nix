let
  dev-mode = import ./dev-mode.nix;

  # gvm32 (Crockford Base32) — the encoding the executor uses for runner hash
  # ids. The embedded `uid` (consumed by `Depends`) MUST use this, matching the
  # on-disk runner paths and `hashToIDHash` in ../default.nix.
  gvm32 = import ../gvm32.nix;

  # A pin may also set `available-after` (RFC 3339 timestamp such as
  # `2026-12-01T00:00:00Z`, transactions before it cannot resolve the runner)
  # and `supersedes` (gvm32 hash of the same id it replaces); both default to
  # null. hash-updater.py rewrites only `hash`, so they survive a re-pin.
  src = rec {
    __prefix = "";

    top = {
      hash = fakeHash; # set to null/test to batch-set all others. set to fake-hash to proceed
    };

    models = {
      __prefix = "models-";

      all-MiniLM-L6-v2 = {
        hash = "sha256-2vGWIlrrjV11GirBD3tEQeZGNwOAwy8Spjvp7zkfUqo=";

        depends = [
          top
        ];
      };
    };

    pyLibs = {
      __prefix = "py-lib-";

      cloudpickle = {
        hash = "sha256-TN1BHLvPXx1J5rkT2XAFD5bd/pamqZaUSl+ECARlTm8=";
        depends = [
          top
        ];
      };
      protobuf = {
        hash = "sha256-gYRnaWpTTqH+7bH5dp2lmD3G2jKbezTiCFLj0UIU2hw=";
        depends = [
          top
        ];
      };

      word_piece_tokenizer = {
        hash = "sha256-SGfP6emJTAB1CqCE5hofj5k6RjKDHdLKM4SRR/BQL9o=";
        depends = [
          top
        ];
      };

      genlayer-std = {
        hash = "sha256-PksnxuFxyJ5pMoisrXko6z9xeIeeHTRMxPxoJMiO0HY=";
        available-after = "2026-12-01T00:00:00Z";
        supersedes = "z7fv8bqpetj3wnjd8sn5s5xfqve3rz8nmtq4npwf2qdvns57zjgg";
        depends = [
          top
        ];
      };

      genlayer-embeddings = {
        hash = "sha256-je2HODoZDgfVTBKc5wfFYIQCzEJcJhslGYIoPIatp1c=";

        depends = [
          models.all-MiniLM-L6-v2
          pyLibs.word_piece_tokenizer
          pyLibs.protobuf
        ];
      };
    };

    cpython = {
      hash = "sha256-Hokchd7GUYWC7yjQxbBCPLM7um5Nn+yDan6VbaQ9qnY=";
      depends = [
        softfloat
      ];
    };

    softfloat = {
      hash = "sha256-2bJDpPcTRdQAFYrxxdM9NsfXEg/7G3mPScjbPdrbjAQ=";
      depends = [
        top
      ];
    };

    wrappers = {
      __prefix = "";
      py-genlayer = {
        hash = "sha256-GIcaWExUo/cmpIKNIsSZKq7YeEe/GfcCxrRn9kCr7lk=";
        available-after = "2026-12-01T00:00:00Z";
        supersedes = "babwp9t37jw9g4j2bccgn14g6x7x9s1f7dsmh7emvz735pz4zxa0";
        depends = [
          cpython
          pyLibs.cloudpickle
          pyLibs.genlayer-std
        ];
      };
      py-genlayer-multi = {
        hash = "sha256-zkeQjXwF16fI+1JyAA56sv/wh2IvGoZZqXcrG1HJ5rQ=";
        available-after = "2026-12-01T00:00:00Z";
        supersedes = "2f6ynxqmzzg1dmcsaj5m2k1b1bgpe1y3850y7vhm8m2eewcvg130";
        depends = [
          cpython
          pyLibs.cloudpickle
          pyLibs.genlayer-std
        ];
      };
    };
  };

  hashHasSpecial = hsh: val: if val.hash == hsh then true else hashHasSpecialDeps hsh val;

  hashHasSpecialDeps =
    hsh: val:
    builtins.any (hashHasSpecial hsh) (if builtins.hasAttr "depends" val then val.depends else [ ]);

  deduceHash =
    val:
    if hashHasSpecial null val then
      null
    else if hashHasSpecial "test" val then
      "test"
    else
      val.hash;

  fakeHash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

  checkHashes = (
    pref: name: val:
    if builtins.hasAttr "__prefix" val then
      builtins.foldl' (acc: item: acc + item) "" (
        builtins.map (name: checkHashes (pref + val.__prefix) name val.${name}) (
          builtins.filter (name: name != "__prefix") (builtins.attrNames val)
        )
      )
    else if val.hash == null then
      ""
    else if val.hash == "test" then
      (if dev-mode then "" else "set ${pref + name} hash to 'null'\n")
    else if hashHasSpecialDeps null val then
      "set ${pref + name} hash to null\n"
    else if hashHasSpecialDeps null val then
      "set ${pref + name} hash to 'test'\n"
    else
      ""
  );

  transform = (
    pref: name: val:
    if builtins.hasAttr "__prefix" val then
      builtins.listToAttrs (
        builtins.map (name: {
          inherit name;
          value = transform (pref + val.__prefix) name val.${name};
        }) (builtins.filter (name: name != "__prefix") (builtins.attrNames val))
      )
    else
      let
        deducedHashBase = deduceHash val;
        deducedHash =
          if deducedHashBase == "error" then
            builtins.throw "set ${pref + name} hash to null"
          else
            deducedHashBase;
        hashSRI = if deducedHash == null then fakeHash else deducedHash;
        hash32 =
          if deducedHash == "test" then
            "test"
          else
            gvm32.encodeHex (
              builtins.convertHash {
                hash = hashSRI;
                toHashFormat = "base16";
              }
            );
      in
      rec {
        id = pref + name;

        hash = hashSRI;

        uid = "${id}:${hash32}";

        excludeFromBuild = hashHasSpecialDeps null val;

        available-after = val.available-after or null;
        supersedes = val.supersedes or null;
      }
  );
in
builtins.seq (
  let
    errs = checkHashes "" "" src;
  in
  if errs != "" then builtins.throw errs else null
) (transform "" "" src)
