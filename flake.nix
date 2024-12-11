{
  description = "A basic Rust devshell for NixOS users developing Bimble";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    nixpkgs,
    rust-overlay,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        overlays = [(import rust-overlay)];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
      in
        with pkgs; {
          devShells.default = mkShell {
            buildInputs =
              [
                openssl
                pkg-config
                cacert
                clang
                cargo-make
                trunk
                # mold-wrapped # couldn't get mold to work
                # for tests
                glib
                gdk-pixbuf
                stdenv.cc
                atkmm
                pango
                gdk-pixbuf-xlib
                gtk3
                libsoup_3
                webkitgtk_4_1
                xdotool
                xdo
                (rust-bin.selectLatestNightlyWith (toolchain:
                  toolchain.default.override {
                    extensions = ["rust-src" "rust-analyzer"];
                    targets = ["wasm32-unknown-unknown"];
                  }))
              ]
              ++ pkgs.lib.optionals pkg.stdenv.isDarwin [
                darwin.apple_sdk.frameworks.SystemConfiguration
              ];

            shellHook = ''
            '';
          };
        }
    );
}
