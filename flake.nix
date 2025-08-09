{
  description = "Battery monitor Rust workspace";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nmattia/naersk";
  };

  outputs = { self, nixpkgs, flake-utils, naersk }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        naersk-lib = naersk.lib.${system};
      in {
        packages = {
          daemon = naersk-lib.buildPackage {
            pname = "battery-monitor-daemon";
            src = ./.;
            cargoBuildOptions = [ "--bin" "battery-monitor-daemon" ];
          };
          viewer = naersk-lib.buildPackage {
            pname = "battery-monitor-viewer";
            src = ./.;
            cargoBuildOptions = [ "--bin" "battery-monitor-viewer" ];
          };
        };
        apps.daemon = flake-utils.lib.mkApp { drv = self.packages.${system}.daemon; };
        apps.viewer = flake-utils.lib.mkApp { drv = self.packages.${system}.viewer; };
        defaultPackage = self.packages.${system}.daemon;
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [ rustc cargo ];
        };
      });
}
