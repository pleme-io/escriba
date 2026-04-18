{
  description = "escriba — Rust + tatara-lisp editor, built on the pleme-io GPU stack";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: type:
            let rel = pkgs.lib.removePrefix (toString ./.) (toString path);
            in !(builtins.match "^/target(/.*)?$" rel != null
                 || builtins.match "^/result.*$" rel != null
                 || builtins.match ".*/\\.direnv(/.*)?$" rel != null);
        };
        mkBin = { pname, package }: pkgs.rustPlatform.buildRustPackage {
          inherit pname src;
          version = "0.1.0";
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "-p" package ];
          buildInputs = with pkgs; [
            libxkbcommon wayland vulkan-loader
            libGL mesa xorg.libX11 xorg.libXcursor xorg.libXi xorg.libXrandr
            fontconfig freetype
          ];
          nativeBuildInputs = with pkgs; [ pkg-config ];
          doCheck = false;
        };
        escriba = mkBin { pname = "escriba"; package = "escriba"; };
      in {
        packages = {
          inherit escriba;
          default = escriba;
        };
        apps.default = { type = "app"; program = "${escriba}/bin/escriba"; };
        devShells.default = pkgs.mkShell {
          name = "escriba-dev";
          packages = with pkgs; [
            rustc cargo rustfmt clippy rust-analyzer
            pkg-config libxkbcommon wayland vulkan-loader
            libGL fontconfig freetype
            git jq yq-go
          ];
        };
        checks = {
          cargo-fmt = pkgs.runCommand "fmt" {
            inherit src;
            nativeBuildInputs = [ pkgs.rustfmt pkgs.cargo ];
          } "cd $src && cargo fmt --all -- --check && touch $out";
        };
      });
}
