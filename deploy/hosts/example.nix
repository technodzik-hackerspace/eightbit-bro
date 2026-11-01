# Copy to deploy/hosts/<hostname>.nix (gitignored) and fill in.
# Telegram and dashboard secrets are entered in the dashboard, not here.
{ ... }:

{
  deployment = {
    targetHost = "192.0.2.10";
    targetUser = "root";
  };

  networking.hostName = "eightbit-bro";

  users.users.root.openssh.authorizedKeys.keys = [
    "ssh-ed25519 AAAA... you@laptop"
  ];

  users.users.admin = {
    isNormalUser = true;
    extraGroups = [ "wheel" "video" ];
    openssh.authorizedKeys.keys = [
      "ssh-ed25519 AAAA... you@laptop"
    ];
  };

  # The NixOS release this host was first installed with; never change it.
  system.stateVersion = "26.05";
}
