# A Raspberry Pi 4 that is an eightbit-bro appliance: boots from SD, captures from a USB
# HDMI card, and is managed remotely. Host-specific bits (hostname, keys) live in
# deploy/hosts/<name>.nix.
{ lib, pkgs, ... }:

{
  nixpkgs.hostPlatform = "aarch64-linux";

  boot = {
    # Mainline kernel: built and cached by Hydra (linux-rpi is deprecated and uncached).
    kernelPackages = pkgs.linuxPackages;
    initrd.availableKernelModules = [ "xhci_pci" "usbhid" "usb_storage" ];
    loader = {
      grub.enable = false;
      generic-extlinux-compatible = {
        enable = true;
        configurationLimit = 10;
      };
    };
    # Scratch space in RAM, not on the SD card.
    tmp = {
      useTmpfs = true;
      tmpfsSize = "256M";
    };
  };

  fileSystems."/" = {
    device = "/dev/disk/by-label/NIXOS_SD";
    fsType = "ext4";
    options = [ "noatime" ];
  };

  hardware.enableRedistributableFirmware = true;

  # 1.8 GB RAM and no swap partition: compressed swap in RAM.
  zramSwap = {
    enable = true;
    memoryPercent = 50;
  };

  # Keep SD card writes low.
  services.journald.extraConfig = ''
    SystemMaxUse=200M
  '';

  services.openssh = {
    enable = true;
    settings = {
      PasswordAuthentication = false;
      KbdInteractiveAuthentication = false;
      PermitRootLogin = "prohibit-password";
    };
  };

  services.tailscale.enable = lib.mkDefault true;
  networking.firewall = {
    enable = true;
    trustedInterfaces = [ "tailscale0" ];
  };

  users.mutableUsers = false;
  security.sudo.wheelNeedsPassword = false;

  nix = {
    settings = {
      experimental-features = [ "nix-command" "flakes" ];
      trusted-users = [ "root" "@wheel" ];
    };
    gc = {
      automatic = true;
      dates = "weekly";
      options = "--delete-older-than 14d";
    };
  };

  environment.systemPackages = with pkgs; [
    ffmpeg-headless
    htop
    usbutils
    v4l-utils
    vim
  ];

  system.stateVersion = lib.mkDefault "26.05";
}
