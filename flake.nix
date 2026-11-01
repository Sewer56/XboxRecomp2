{
  description = "XboxRecomp2: Rust port of the XboxRecomp x86-to-C++ recompiler";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, fenix }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          toolchains = fenix.packages.${system};

          rust = toolchains.combine [
            toolchains.stable.cargo
            toolchains.stable.rustc
            toolchains.stable.rust-src
            toolchains.stable.clippy
            toolchains.stable.rustfmt
            toolchains.stable.rust-analyzer

            # Windows std libraries let `cross` build the Windows targets.
            toolchains.targets.x86_64-pc-windows-gnu.stable.rust-std
            toolchains.targets.i686-pc-windows-gnu.stable.rust-std
          ];
        in
        {
          default = pkgs.mkShell {
            # cmake and gcc build the C sources of `zydis`.
            packages = [
              rust
              pkgs.cmake
              pkgs.gcc
            ];

            # `cross` expects a rustup toolchain.
            # These let it use the Nix toolchain above instead.
            CROSS_CUSTOM_TOOLCHAIN = "1";
            CROSS_CUSTOM_TOOLCHAIN_COMPAT = "stable-${pkgs.stdenv.hostPlatform.config}";
          };
        });
    };
}
