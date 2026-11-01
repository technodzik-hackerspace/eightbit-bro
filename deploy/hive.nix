# Colmena hive for push-based deploys from a workstation:
#   colmena apply -f deploy/hive.nix --on <host>
#
# Hosts are files in ./hosts/ (gitignored except example.nix). Flakes can't see untracked
# files, so this is a plain hive that pins nixpkgs from the repo's flake.lock.
let
  lock = (builtins.fromJSON (builtins.readFile ../flake.lock)).nodes.nixpkgs.locked;
  nixpkgs = builtins.fetchTarball {
    url = "https://github.com/${lock.owner}/${lock.repo}/archive/${lock.rev}.tar.gz";
    sha256 = lock.narHash;
  };
  lib = import "${nixpkgs}/lib";

  hostFiles = lib.filter (f: f != "example.nix" && lib.hasSuffix ".nix" f)
    (lib.attrNames (builtins.readDir ./hosts));
in
{
  meta.nixpkgs = import nixpkgs { system = "x86_64-linux"; };

  defaults = {
    imports = [ ../nix/appliance-rpi4.nix ];
    # Build on the deploying machine (needs aarch64 binfmt, e.g. fenek), not on the Pi.
    deployment.buildOnTarget = lib.mkDefault false;
  };
}
// lib.listToAttrs (map (f: {
  name = lib.removeSuffix ".nix" f;
  value = import (./hosts + "/${f}");
}) hostFiles)
