# NixOS module: `programs.dshell.enable = true;`
{ config, lib, pkgs, ... }:

let
  cfg = config.programs.dshell;
in
{
  options.programs.dshell = {
    enable = lib.mkEnableOption "DarkShadow's Shell (Rust)";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix { };
      defaultText = lib.literalExpression "pkgs.callPackage ./package.nix { }";
      description = "The dshell package.";
    };
    services = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Turn on what the shell reads: NetworkManager, Bluetooth, UPower and PipeWire.";
    };
    nightLight = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Install hyprsunset and wlsunset for the night light switch.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages =
      [ cfg.package ]
      ++ lib.optionals cfg.nightLight [ pkgs.wlsunset ]
      ++ lib.optionals (cfg.nightLight && pkgs ? hyprsunset) [ pkgs.hyprsunset ];

    networking.networkmanager.enable = lib.mkIf cfg.services (lib.mkDefault true);
    hardware.bluetooth.enable = lib.mkIf cfg.services (lib.mkDefault true);
    services.upower.enable = lib.mkIf cfg.services (lib.mkDefault true);
    services.pipewire = lib.mkIf cfg.services {
      enable = lib.mkDefault true;
      pulse.enable = lib.mkDefault true;
      wireplumber.enable = lib.mkDefault true;
    };
    # brightness without root goes through logind (SetBrightness); nothing else needed
  };
}
