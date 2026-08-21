{
  description = "Prebuilt Stay packages for NixOS and Home Manager";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/9f78f44a87948854445dae0b6bf82b2e87e4efb5";
    home-manager = {
      url = "github:nix-community/home-manager/d4fd24667c8cbef124bb70a20380cab75ec8474d";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, home-manager }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forEachSystem = nixpkgs.lib.genAttrs systems;
      pkgsFor = system: import nixpkgs { inherit system; };
      stayFor = system: (pkgsFor system).callPackage ./nix/package.nix {
        inherit system;
      };

      moduleConfig = system: module: modulePkgs: settings: base:
        modulePkgs.lib.evalModules {
          modules = [
            base
            (module { pkgs = modulePkgs; stay = stayFor system; })
            settings
          ];
        };
    in
    {
      packages = forEachSystem (system: {
        stay = stayFor system;
        default = stayFor system;
      });

      nixosModules.stay = { pkgs, stay }:
        import ./nix/nixos-module.nix { inherit pkgs stay; };
      homeManagerModules.stay = { pkgs, stay }:
        import ./nix/home-manager-module.nix { inherit pkgs stay; };

      checks = forEachSystem (system:
        let
          pkgs = pkgsFor system;
          stay = stayFor system;
          legacy = let
            result = import ./nix/default.nix { inherit pkgs system; };
          in
            assert builtins.attrNames result == [
              "homeManagerModule"
              "nixosModule"
              "stay"
            ];
            result;
          manifest = pkgs.fetchurl {
            url = "https://github.com/nevdelap/stay/releases/download/v0.0.88/SHA256SUMS";
            hash = "sha256-0C6Q2WdMFkKi5iwfOvTm7EKuYzliM54zEnc5K2v1TyI=";
          };
          packageCheck = assert builtins.elem pkgs.tmux stay.propagatedBuildInputs;
            pkgs.runCommand "stay-package-check" {
            nativeBuildInputs = [ pkgs.file ];
          } ''
            test "${self.packages.${system}.stay}" = \
              "${self.packages.${system}.default}"
            test -x ${stay}/bin/stay
            test "$(find ${stay}/bin -type f | sort)" = "${stay}/bin/stay"
            test "$(file -b ${stay}/bin/stay)" != "empty"
            test "$(${stay}/bin/stay --version)" = "stay 0.0.88"
            ! find ${stay} -type f \( -iname '*cargo*' -o -iname '*rust*' \
              -o -iname '*source*' \) -print -quit | grep -q .
            touch "$out"
          '';
          hashCheck = pkgs.runCommand "stay-release-hashes-check" { } ''
            test "$(wc -l < ${manifest})" -eq 4
            test "$(sha256sum ${manifest} | cut -d' ' -f1)" = \
              d02e90d9674c1642a2e62c1f3af4e6ec42ae633962339e331277392b6bf54f22
            grep -Fqx \
              '5560913eefda4dccb7b357f34e3fe41160cc6d27a149a75985f502e52c91a022  stay-v0.0.88-x86_64-unknown-linux-gnu.tar.gz' \
              ${manifest}
            grep -Fqx \
              'b41d5260b60163dad245466a49d44d47f0b19b3a4725079bd17bc7e6b5921022  stay-v0.0.88-aarch64-unknown-linux-gnu.tar.gz' \
              ${manifest}
            grep -Fqx \
              '9583e27ae12228f83db15fa72f319b3faf62639e64b56e9aa389bec58fec18f8  stay-v0.0.88-x86_64-apple-darwin.tar.gz' \
              ${manifest}
            grep -Fqx \
              '6a0a215097a49a634e9620c33553132b0f3ad78bdcd007c30d6b64921ace6eab  stay-v0.0.88-aarch64-apple-darwin.tar.gz' \
              ${manifest}
            touch "$out"
          '';
          nixosBase = {
            options.environment.systemPackages = pkgs.lib.mkOption {
              type = pkgs.lib.types.listOf pkgs.lib.types.package;
              default = [ ];
            };
          };
          homeBase = {
            options.home.packages = pkgs.lib.mkOption {
              type = pkgs.lib.types.listOf pkgs.lib.types.package;
              default = [ ];
            };
          };
          nixosConfig = module: settings:
            moduleConfig system module pkgs settings nixosBase;
          homeConfig = module: settings:
            moduleConfig system module pkgs settings homeBase;
          nixosLegacyModule = _args: legacy.nixosModule;
          homeLegacyModule = _args: legacy.homeManagerModule;
          homeManagerConfig = module: settings:
            home-manager.lib.homeManagerConfiguration {
              inherit pkgs;
              modules = [
                (module { inherit pkgs stay; })
                {
                  home.username = "stay";
                  home.homeDirectory = "/tmp/stay";
                  home.stateVersion = "26.05";
                }
                settings
              ];
            };
          embeddedConfig = settings:
            nixpkgs.lib.nixosSystem {
              inherit system;
              specialArgs = { inherit self; };
              modules = [
                home-manager.nixosModules.home-manager
                {
                  system.stateVersion = "26.05";
                  home-manager.users.stay = {
                    imports = [
                      (self.homeManagerModules.stay { inherit pkgs stay; })
                    ];
                    home.username = "stay";
                    home.homeDirectory = "/tmp/stay";
                    home.stateVersion = "26.05";
                  } // settings;
                }
              ];
            };
          nixosFlake = nixosConfig self.nixosModules.stay { };
          nixosLegacy = nixosConfig nixosLegacyModule { };
          nixosFlakeOverride = nixosConfig self.nixosModules.stay {
            programs.stay.package = pkgs.hello;
          };
          nixosLegacyOverride = nixosConfig nixosLegacyModule {
            programs.stay.package = pkgs.hello;
          };
          homeFlake = homeConfig self.homeManagerModules.stay { };
          homeLegacy = homeConfig homeLegacyModule { };
          homeFlakeOverride = homeConfig self.homeManagerModules.stay {
            programs.stay.package = pkgs.hello;
          };
          homeLegacyOverride = homeConfig homeLegacyModule {
            programs.stay.package = pkgs.hello;
          };
          homeFlakeActivation = homeManagerConfig
            self.homeManagerModules.stay { };
          homeFlakeDisabledActivation = homeManagerConfig
            self.homeManagerModules.stay { programs.stay.enable = false; };
          homeFlakeNoTmuxActivation = homeManagerConfig
            self.homeManagerModules.stay { programs.stay.enableTmux = false; };
          homeFlakeOverrideActivation = homeManagerConfig
            self.homeManagerModules.stay { programs.stay.package = pkgs.hello; };
          homeLegacyActivation = homeManagerConfig homeLegacyModule { };
          homeLegacyDisabledActivation = homeManagerConfig homeLegacyModule {
            programs.stay.enable = false;
          };
          homeLegacyNoTmuxActivation = homeManagerConfig homeLegacyModule {
            programs.stay.enableTmux = false;
          };
          homeLegacyOverrideActivation = homeManagerConfig homeLegacyModule {
            programs.stay.package = pkgs.hello;
          };
          embeddedDefault = embeddedConfig { };
          embeddedDisabled = embeddedConfig {
            programs.stay.enable = false;
          };
          embeddedNoTmux = embeddedConfig {
            programs.stay.enableTmux = false;
          };
          embeddedOverride = embeddedConfig {
            programs.stay.package = pkgs.hello;
          };
          nixosDisabled = nixosConfig self.nixosModules.stay {
            programs.stay.enable = false;
          };
          nixosNoTmux = nixosConfig self.nixosModules.stay {
            programs.stay.enableTmux = false;
          };
          homeDisabled = homeConfig self.homeManagerModules.stay {
            programs.stay.enable = false;
          };
          homeNoTmux = homeConfig self.homeManagerModules.stay {
            programs.stay.enableTmux = false;
          };
          packageNames = packages: map pkgs.lib.getName packages;
          relevantPackages = packages:
            builtins.filter
              (package: builtins.elem (pkgs.lib.getName package)
                [ "stay" "tmux" "hello" ])
              packages;
          moduleCheck = name: defaultPackages: disabledPackages: noTmuxPackages: overridePackages: activations:
            pkgs.runCommand name { } ''
              ${pkgs.lib.concatMapStringsSep "\n" (activation: "test -e ${activation}") activations}
              test "${builtins.toJSON (packageNames defaultPackages)}" = \
                '[stay,tmux]'
              test "${builtins.toJSON (packageNames disabledPackages)}" = '[]'
              test "${builtins.toJSON (packageNames noTmuxPackages)}" = '[stay]'
              test "${builtins.toJSON (packageNames overridePackages)}" = '[hello,tmux]'
              touch "$out"
            '';
        in {
          stay-package = packageCheck;
          release-hashes = hashCheck;
          nixos-flake = moduleCheck "stay-nixos-flake-check"
            nixosFlake.config.environment.systemPackages
            nixosDisabled.config.environment.systemPackages
            nixosNoTmux.config.environment.systemPackages
            nixosFlakeOverride.config.environment.systemPackages
            [ ];
          nixos-legacy = moduleCheck "stay-nixos-legacy-check"
            nixosLegacy.config.environment.systemPackages
            nixosDisabled.config.environment.systemPackages
            nixosNoTmux.config.environment.systemPackages
            nixosLegacyOverride.config.environment.systemPackages
            [ ];
          home-manager-flake = moduleCheck "stay-home-manager-flake-check"
            homeFlake.config.home.packages
            homeDisabled.config.home.packages
            homeNoTmux.config.home.packages
            homeFlakeOverride.config.home.packages
            [
              homeFlakeActivation.activationPackage
              homeFlakeDisabledActivation.activationPackage
              homeFlakeNoTmuxActivation.activationPackage
              homeFlakeOverrideActivation.activationPackage
            ];
          home-manager-legacy = moduleCheck "stay-home-manager-legacy-check"
            homeLegacy.config.home.packages
            homeDisabled.config.home.packages
            homeNoTmux.config.home.packages
            homeLegacyOverride.config.home.packages
            [
              homeLegacyActivation.activationPackage
              homeLegacyDisabledActivation.activationPackage
              homeLegacyNoTmuxActivation.activationPackage
              homeLegacyOverrideActivation.activationPackage
            ];
          home-manager-embedded = moduleCheck "stay-home-manager-embedded-check"
            (relevantPackages embeddedDefault.config.home-manager.users.stay.home.packages)
            (relevantPackages embeddedDisabled.config.home-manager.users.stay.home.packages)
            (relevantPackages embeddedNoTmux.config.home-manager.users.stay.home.packages)
            (relevantPackages embeddedOverride.config.home-manager.users.stay.home.packages)
            [ ];
        });
    };
}
