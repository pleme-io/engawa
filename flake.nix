{
  description = "Engawa (縁側) — typed render-graph IR for pleme-io GPU consumers";

  inputs = {
    nixpkgs.follows = "substrate/nixpkgs";
    crate2nix.url = "github:nix-community/crate2nix";
    flake-utils.url = "github:numtide/flake-utils";
    substrate = {
      url = "github:pleme-io/substrate";
    };
  };

  outputs = { self, nixpkgs, crate2nix, flake-utils, substrate, ... }:
    (import "${substrate}/lib/rust-library-flake.nix" {
      inherit nixpkgs crate2nix flake-utils;
    }) {
      libName = "engawa";
      src = self;
      repo = "pleme-io/engawa";
    };
}
