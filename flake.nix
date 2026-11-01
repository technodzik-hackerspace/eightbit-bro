{
  description = "eightbit-bro: he watches and knows all your 8-bit games";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs = { self, nixpkgs }:
    let
      lib = nixpkgs.lib;
      forAll = f: lib.genAttrs [ "x86_64-linux" "aarch64-linux" ]
        (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = lib.recursiveUpdate
        (forAll (pkgs: { default = pkgs.callPackage ./nix/package.nix { }; }))
        {
          aarch64-linux.sd-image-rpi4 =
            self.nixosConfigurations.sd-image-rpi4.config.system.build.sdImage;
        };

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.default ];
          packages = with pkgs; [ clippy rustfmt rust-analyzer ffmpeg-headless sqlite ];
        };
      });

      nixosModules = {
        # Everything that turns a Raspberry Pi 4 into an eightbit-bro box.
        appliance-rpi4 = ./nix/appliance-rpi4.nix;
      };

      # Fresh SD card: flash it, plug in the capture card, boot, open the dashboard.
      nixosConfigurations.sd-image-rpi4 = nixpkgs.lib.nixosSystem {
        modules = [
          "${nixpkgs}/nixos/modules/installer/sd-card/sd-image-aarch64.nix"
          self.nixosModules.appliance-rpi4
          { networking.hostName = "eightbit-bro"; }
        ];
      };
    };
}
