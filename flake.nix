{
  description = "Rust + tatara-lisp editor, built on the pleme-io GPU stack";

  nixConfig.allow-import-from-derivation = true;

  inputs = {
    nixpkgs = {
      url = "github:nixos/nixpkgs?ref=nixos-unstable";
    };
    flake-utils = {
      url = "github:numtide/flake-utils";
    };
    substrate = {
      url = "github:pleme-io/substrate";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    tatara = {
      url = "git+ssh://git@github.com/pleme-io/tatara";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    shikumi = {
      url = "git+ssh://git@github.com/pleme-io/shikumi";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    garasu = {
      url = "git+ssh://git@github.com/pleme-io/garasu";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    madori = {
      url = "git+ssh://git@github.com/pleme-io/madori";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    awase = {
      url = "git+ssh://git@github.com/pleme-io/awase";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    hayai = {
      url = "git+ssh://git@github.com/pleme-io/hayai";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    irodori = {
      url = "git+ssh://git@github.com/pleme-io/irodori";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    mojiban = {
      url = "git+ssh://git@github.com/pleme-io/mojiban";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs @ { self, nixpkgs, flake-utils, substrate, tatara, shikumi, garasu, madori, awase, hayai, irodori, mojiban, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        cleanSelf = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: _:
            let rel = pkgs.lib.removePrefix (toString ./.) (toString path);
            in !(builtins.match "^/target(/.*)?$" rel != null
                 || builtins.match "^/result.*$" rel != null
                 || builtins.match ".*/\\.direnv(/.*)?$" rel != null);
        };

        composedSrc = pkgs.runCommand "escriba-composed-src" {} ''
          mkdir -p $out/escriba
          cp -r ${cleanSelf}/. $out/escriba/
          chmod -R +w $out/escriba
          cp -r ${tatara} $out/tatara
          chmod -R +w $out/tatara
          cp -r ${shikumi} $out/shikumi
          chmod -R +w $out/shikumi
          cp -r ${garasu} $out/garasu
          chmod -R +w $out/garasu
          cp -r ${madori} $out/madori
          chmod -R +w $out/madori
          cp -r ${awase} $out/awase
          chmod -R +w $out/awase
          cp -r ${hayai} $out/hayai
          chmod -R +w $out/hayai
          cp -r ${irodori} $out/irodori
          chmod -R +w $out/irodori
          cp -r ${mojiban} $out/mojiban
          chmod -R +w $out/mojiban
        '';

        escriba = pkgs.rustPlatform.buildRustPackage {
          pname = "escriba";
          version = "0.1.0";
          src = composedSrc;
          sourceRoot = "escriba-composed-src/escriba";
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "-p" "escriba" ];
          cargoTestFlags  = [ "-p" "escriba" ];
          doCheck = false;
          nativeBuildInputs = with pkgs; [ pkg-config ];
          buildInputs = with pkgs; [ openssl ];
        };
      in {
        packages = { inherit escriba; default = escriba; };
        apps.default = { type = "app"; program = "${escriba}/bin/escriba"; };
        devShells.default = pkgs.mkShell {
          name = "escriba-dev";
          packages = with pkgs; [
            rustc cargo rustfmt clippy rust-analyzer
            pkg-config openssl git jq yq-go
          ];
        };
      }) // {
        overlays.default = final: _prev: { escriba = self.packages.${final.system}.escriba; };
        homeManagerModules.default = import ./module { self = self; };
      };
    };
}
