{ pkgs, stay }:

{ config, lib, ... }:

let
  cfg = config.programs.stay;
in
{
  options.programs.stay = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Install Stay.";
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = stay;
      description = "The Stay package to install.";
    };

    enableTmux = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Install tmux as Stay's runtime dependency.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ]
      ++ lib.optional cfg.enableTmux pkgs.tmux;
  };
}
