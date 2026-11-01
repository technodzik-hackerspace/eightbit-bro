# The eightbit-bro service, usable on any NixOS machine (the Pi appliance, or fenek for
# replays). Declarative settings become /etc/eightbit-bro/config.toml; secrets are entered
# in the dashboard and stay in the state directory.
{ config, lib, pkgs, ... }:

let
  cfg = config.services.eightbit-bro;
  toml = pkgs.formats.toml { };
  configFile = toml.generate "eightbit-bro.toml" cfg.settings;
in
{
  options.services.eightbit-bro = {
    enable = lib.mkEnableOption "eightbit-bro, the 8-bit console watcher";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix { };
      description = "The eightbit-bro package.";
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 8080;
      description = "Dashboard port.";
    };

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Open the dashboard port (keep the device off the public internet).";
    };

    settings = lib.mkOption {
      type = toml.type;
      default = { };
      example = lib.literalExpression ''
        { input.device = "/dev/v4l/by-id/usb-MACROSILICON_USB3._0_capture-video-index0"; }
      '';
      description = "Contents of config.toml (see crates/eightbit-bro/src/config.rs).";
    };
  };

  config = lib.mkIf cfg.enable {
    services.eightbit-bro.settings = {
      data_dir = lib.mkDefault "/var/lib/eightbit-bro";
      web.listen = lib.mkDefault "0.0.0.0:${toString cfg.port}";
    };

    environment.etc."eightbit-bro/config.toml".source = configFile;

    systemd.services.eightbit-bro = {
      description = "eightbit-bro: watches the console";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];
      path = [ pkgs.ffmpeg-headless ];
      restartTriggers = [ configFile ];
      serviceConfig = {
        ExecStart = "${lib.getExe cfg.package} /etc/eightbit-bro/config.toml";
        Restart = "always";
        RestartSec = 5;
        DynamicUser = true;
        SupplementaryGroups = [ "video" ];
        StateDirectory = "eightbit-bro";
        StateDirectoryMode = "0700";
        # Capture first (tech design §8).
        Nice = -5;
        MemoryMax = "600M";
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        NoNewPrivileges = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictAddressFamilies = [ "AF_INET" "AF_INET6" "AF_UNIX" ];
      };
    };

    networking.firewall.allowedTCPPorts = lib.mkIf cfg.openFirewall [ cfg.port ];
  };
}
