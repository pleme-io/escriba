{
  description = "escriba — Rust + tatara-lisp editor, built on the pleme-io GPU stack";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    # Pleme-io siblings — Cargo.toml has ../<sibling> path deps; the flake
    # composes them into a hermetic workspace tree. Local dev still uses
    # the in-tree paths; the flake just shadows them during nix builds.
    tatara   = { url = "git+ssh://git@github.com/pleme-io/tatara"; flake = false; };
    shikumi  = { url = "github:pleme-io/shikumi";                  flake = false; };
    garasu   = { url = "github:pleme-io/garasu";                   flake = false; };
    madori   = { url = "github:pleme-io/madori";                   flake = false; };
    awase    = { url = "github:pleme-io/awase";                    flake = false; };
    hayai    = { url = "github:pleme-io/hayai";                    flake = false; };
    irodori  = { url = "github:pleme-io/irodori";                  flake = false; };
    mojiban  = { url = "github:pleme-io/mojiban";                  flake = false; };

    substrate = { url = "github:pleme-io/substrate"; inputs.nixpkgs.follows = "nixpkgs"; };
  };

  outputs = inputs @ { self, nixpkgs, flake-utils, substrate, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        cleanEscriba = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: type:
            let rel = pkgs.lib.removePrefix (toString ./.) (toString path);
            in !(builtins.match "^/target(/.*)?$" rel != null
                 || builtins.match "^/result.*$" rel != null
                 || builtins.match ".*/\\.direnv(/.*)?$" rel != null);
        };

        # Compose a workspace tree that mirrors the Cargo.toml layout:
        #   escriba/    ← this repo
        #   tatara/     shikumi/    garasu/    madori/    ← path siblings
        #   awase/      hayai/      irodori/   mojiban/
        composedSrc = pkgs.runCommand "escriba-composed-src" {} ''
          mkdir -p $out/escriba
          cp -r ${cleanEscriba}/. $out/escriba/
          chmod -R +w $out/escriba
          for sib in tatara shikumi garasu madori awase hayai irodori mojiban; do
            case "$sib" in
              tatara)  src="${inputs.tatara}";;
              shikumi) src="${inputs.shikumi}";;
              garasu)  src="${inputs.garasu}";;
              madori)  src="${inputs.madori}";;
              awase)   src="${inputs.awase}";;
              hayai)   src="${inputs.hayai}";;
              irodori) src="${inputs.irodori}";;
              mojiban) src="${inputs.mojiban}";;
            esac
            cp -r "$src" "$out/$sib"
            chmod -R +w "$out/$sib"
          done
        '';

        mkBin = { pname, package }: pkgs.rustPlatform.buildRustPackage {
          inherit pname;
          version = "0.1.0";
          src = composedSrc;
          sourceRoot = "escriba-composed-src/escriba";
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "-p" package ];
          cargoTestFlags  = [ "-p" package ];
          doCheck = false;
          buildInputs = with pkgs; (
            if stdenv.isDarwin
            then [ fontconfig freetype ]
            else [
              libxkbcommon wayland vulkan-loader
              libGL mesa xorg.libX11 xorg.libXcursor xorg.libXi xorg.libXrandr
              fontconfig freetype
            ]
          );
          nativeBuildInputs = with pkgs; [ pkg-config ];
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
            src = cleanEscriba;
            nativeBuildInputs = [ pkgs.rustfmt pkgs.cargo ];
          } "cd $src && cargo fmt --all -- --check && touch $out";
        };
      }) // {
        overlays.default = final: prev: {
          escriba = self.packages.${final.system}.escriba;
        };
        homeManagerModules.default = import ./module { inherit self; };
      };
}
