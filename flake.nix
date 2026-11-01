{
  description = "XboxRecomp2: Rust port of the XboxRecomp x86-to-C++ recompiler";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          rust = pkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" ];
          };
        in
        {
          default = pkgs.mkShell {
            # Native build tools required by the `zydis` dependency, which
            # compiles its vendored C sources through cmake during
            # `cargo build`.
            packages = [
              rust
              pkgs.rust-analyzer
              pkgs.cmake
              pkgs.gcc
            ];
          };
        });
    };
}
