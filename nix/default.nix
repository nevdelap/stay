{ pkgs ? import <nixpkgs> { }, system ? builtins.currentSystem }:

let
  stay = pkgs.callPackage ./package.nix {
    inherit system;
  };
in
{
  inherit stay;
  nixosModule = import ./nixos-module.nix { inherit pkgs stay; };
  homeManagerModule = import ./home-manager-module.nix { inherit pkgs stay; };
}
