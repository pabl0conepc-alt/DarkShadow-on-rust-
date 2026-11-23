{
  description = "DarkShadow's Shell rewritten in Rust (Hyprland, driftwm and VEYRON WM)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (s: f nixpkgs.legacyPackages.${s});
    in
    {
      packages = forAll (pkgs: rec {
        dshell = pkgs.callPackage ./nix/package.nix { };
        default = dshell;
      });

      overlays.default = final: _prev: { dshell = final.callPackage ./nix/package.nix { }; };

      nixosModules.default = { pkgs, ... }: {
        imports = [ ./nix/module.nix ];
        programs.dshell.package = nixpkgs.lib.mkDefault self.packages.${pkgs.stdenv.hostPlatform.system}.dshell;
      };

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.dshell ];
          packages = [ pkgs.rust-analyzer pkgs.clippy pkgs.rustfmt ];
        };
      });
    };
}
