{
  description = "dagger: code review for smooth brains";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { nixpkgs, rust-overlay, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        # What the window around the page needs: a webview and the plumbing under it. Only
        # the window needs these — the tool itself is Rust and nothing else.
        window = with pkgs; [
          webkitgtk_4_1
          gtk3
          libsoup_3
          glib
          cairo
          pango
          gdk-pixbuf
          atk
          librsvg
          pkg-config
        ];
      in
      {
        devShells.default = pkgs.mkShell {
          packages = [ rust pkgs.cargo-insta pkgs.nodejs_22 ] ++ window;
        };
      });
}
