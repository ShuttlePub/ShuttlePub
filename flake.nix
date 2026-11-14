{
  description = "TAKT + AI coding agents + Rust development environment for NixOS-WSL";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    takt = {
      url = "github:nrslib/takt";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    codex-cli-nix = {
      url = "github:sadjow/codex-cli-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    claude-code-nix = {
      url = "github:sadjow/claude-code-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, takt, codex-cli-nix, claude-code-nix }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          config.allowUnfree = true;
        };
      in {
        devShells.default = pkgs.mkShell {
          buildInputs = [
            takt.packages.${system}.default

            codex-cli-nix.packages.${system}.default
            claude-code-nix.packages.${system}.default

            pkgs.opencode
            pkgs.nodejs_22

            pkgs.bashInteractive
            pkgs.git
            pkgs.gh
            pkgs.gnugrep
            pkgs.ripgrep

            pkgs.rustup
            pkgs.sccache
            pkgs.cargo-release
            pkgs.cargo-nextest
            pkgs.mold-wrapped

            pkgs.pkg-config
            pkgs.openssl
            pkgs.zlib
            pkgs.gcc

            pkgs.python3
          ];

          # sccache
          RUSTC_WRAPPER = "${pkgs.sccache}/bin/sccache";
          SCCACHE_CACHE_SIZE = "50G";

          PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
            pkgs.openssl
            pkgs.zlib
            pkgs.gcc.cc.lib
          ];

          shellHook = ''
            export CARGO_HOME="''${CARGO_HOME:-$HOME/.cargo}"
            export RUSTUP_HOME="''${RUSTUP_HOME:-$HOME/.rustup}"
            export SCCACHE_DIR="''${XDG_CACHE_HOME:-$HOME/.cache}/takt-sccache"
            export PATH="$CARGO_HOME/bin:$PATH"

            mkdir -p "$SCCACHE_DIR"

            if ! rustup toolchain list | grep -q '^stable'; then
              echo "Installing Rust stable toolchain..."
              rustup toolchain install stable --profile minimal
            fi

            rustup default stable
            rustup component add clippy rustfmt rust-analyzer rust-src

            if ! command -v cargo-dylint >/dev/null 2>&1; then
              echo "Installing cargo-dylint..."
              cargo install --locked cargo-dylint dylint-link
            fi

            echo "takt     : $(takt --version 2>/dev/null || echo 'installing...')"
            echo "claude   : $(claude --version 2>/dev/null || echo 'installing...')"
            echo "codex    : $(codex --version 2>/dev/null || echo 'not found')"
            echo "rustc    : $(rustc --version)"
            echo "cargo    : $(cargo --version)"
            echo "dylint   : $(cargo dylint --version 2>/dev/null || echo 'not installed')"
            echo "opencode : $(opencode --version 2>/dev/null || echo 'not found')"
            echo "gh       : $(gh --version | head -1)"
          '';
        };
      }
    );
}
