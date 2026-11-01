{ lib, rustPlatform, pkg-config, libjpeg_turbo }:

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

  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ libjpeg_turbo ];

  meta = {
    description = "He watches and knows all your 8-bit games";
    mainProgram = "eightbit-bro";
  };
}
