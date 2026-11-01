{ lib, rustPlatform }:

rustPlatform.buildRustPackage {
  pname = "eightbit-bro";
  version = "0.1.0";

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      ../fixtures
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  meta = {
    description = "He watches and knows all your 8-bit games";
    mainProgram = "eightbit-bro";
  };
}
