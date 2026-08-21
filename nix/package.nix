{ pkgs
, system ? pkgs.system
}:

let
  version = "0.0.88";
  releaseBase = "https://github.com/nevdelap/stay/releases/download/v${version}";
  releases = {
    x86_64-linux = {
      target = "x86_64-unknown-linux-gnu";
      hash = "sha256-VWCRPu/aTcy3s1fzTj/kEWDMbSehSadZhfUC5SyRoCI=";
    };
    aarch64-linux = {
      target = "aarch64-unknown-linux-gnu";
      hash = "sha256-tB1SYLYBY9rSRUZqSdRNR/CxmzpHJQeb0XvH5rWSECI=";
    };
    x86_64-darwin = {
      target = "x86_64-apple-darwin";
      hash = "sha256-lYPieuEiKPg9sV+nLzGbP69iY55ktW6ao4m+xY/sGPg=";
    };
    aarch64-darwin = {
      target = "aarch64-apple-darwin";
      hash = "sha256-agohUJekmmNOliDDNVMTKw8614vc0AfDDWtkkhrObqs=";
    };
  };
  release = releases.${system} or (throw "stay does not support ${system}");
in
assert pkgs.lib.versionAtLeast pkgs.tmux.version "3.6";
pkgs.stdenvNoCC.mkDerivation {
  pname = "stay";
  inherit version;

  src = pkgs.fetchurl {
    url = "${releaseBase}/stay-v${version}-${release.target}.tar.gz";
    hash = release.hash;
  };
  sourceRoot = ".";

  nativeBuildInputs = pkgs.lib.optionals pkgs.stdenv.isLinux [
    pkgs.autoPatchelfHook
  ];
  buildInputs = pkgs.lib.optionals pkgs.stdenv.isLinux [
    pkgs.glibc
    pkgs.libgcc
  ];
  propagatedBuildInputs = [ pkgs.tmux ];

  dontConfigure = true;
  dontBuild = true;

  installPhase = ''
    install -Dm755 "$(find . -type f -name stay -print -quit)" \
      "$out/bin/stay"
  '';

  meta = {
    description = "Persistent terminal sessions backed by tmux";
    homepage = "https://github.com/nevdelap/stay";
    license = pkgs.lib.licenses.mit;
    mainProgram = "stay";
    platforms = pkgs.lib.platforms.linux ++ pkgs.lib.platforms.darwin;
  };
}
