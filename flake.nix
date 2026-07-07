{
  description = "Athanor — the app. Rust core workspace + iOS shell devshell.";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        # Pinned stable toolchain with iOS targets added so `cargo build -p ffi
        # --target aarch64-apple-ios{,-sim}` links against a real core/std.
        # rustup is NOT used; the overlay toolchain is the single source of
        # cargo/rustc/clippy/rustfmt for both host and iOS targets.
        #
        # Toolchain version: goose's own `rust-toolchain.toml` states 1.92 as its
        # *minimum*, not a maximum. This flake pins stable 1.95.0, which builds
        # `-p ffi --features goose --target aarch64-apple-ios-sim` cleanly (Phase 4
        # D1 verified). 1.95 ≥ 1.92, so goose is satisfied — do NOT downgrade to
        # 1.92 to match the plan's note; that note is goose's floor, and churning
        # the toolchain down would only risk re-resolving the pinned lockfile.
        rustToolchain = pkgs.rust-bin.stable."1.95.0".default.override {
          extensions = [ "rust-src" "clippy" "rustfmt" ];
          targets = [
            "aarch64-apple-ios"
            "aarch64-apple-ios-sim"
            "x86_64-apple-ios"
          ];
        };
      in {
        devShells.default = pkgs.mkShell {
          packages = [ rustToolchain pkgs.rust-analyzer pkgs.cmake pkgs.clang pkgs.just ];
          # bindgen (whisper-rs-sys) needs libclang on its path for `--features whisper`:
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
        };
      });
}
