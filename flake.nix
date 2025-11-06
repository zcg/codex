{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        gitRevision =
          if self ? rev && self.rev != null then self.rev else "unknown";
        rustToolchain = pkgs.rust-bin.nightly.latest.default;
        rustPlatform = pkgs.makeRustPlatform {
          inherit (pkgs) stdenv;
          cargo = rustToolchain;
          rustc = rustToolchain;
        };
        rmcpSrc = pkgs.fetchgit {
          url = "https://github.com/modelcontextprotocol/rust-sdk";
          rev = "c0b777c7f784ba2d456b03c2ec3b98c9b28b5e10";
          hash = "sha256-uAEBai6Uzmpi5fcIn9v4MPE9DbzPvemkaaZ+alwM4PQ=";
        };
        ratatuiSrc = pkgs.fetchgit {
          url = "https://github.com/nornagon/ratatui";
          rev = "9b2ad1298408c45918ee9f8241a6f95498cdbed2";
          hash = "sha256-HBvT5c8GsiCxMffNjJGLmHnvG77A6cqEL+1ARurBXho=";
        };
        cargoLock = {
          lockFile = ./codex-rs/Cargo.lock;
          outputHashes = {
            "ratatui-0.29.0" = "sha256-HBvT5c8GsiCxMffNjJGLmHnvG77A6cqEL+1ARurBXho=";
            "crossterm-0.28.1" = "0vzgpvbri4m4qydkj50ch468az7myy04qh5z2n500p1f4dysv87a";
          };
        };
        cargoVendorSha = "sha256-NP94EW+XS1PrbFfMnGOCnwoNoT1S7txJ8bDD6xRb5hw=";
        cargoPatchConfig = pkgs.writeText "cargo-config.toml" ''
          [patch."https://github.com/modelcontextprotocol/rust-sdk"]
          rmcp = { path = "${rmcpSrc}/crates/rmcp" }
          rmcp-macros = { path = "${rmcpSrc}/crates/rmcp-macros" }

          [patch.crates-io]
          ratatui = { path = "${ratatuiSrc}" }
        '';
        commonRustPackageArgs = {
          version = "unstable";
          src = ./codex-rs;
          inherit cargoLock;
          cargoSha256 = cargoVendorSha;
          CODEX_BUILD_GIT_SHA = gitRevision;
          nativeBuildInputs = with pkgs; [ pkg-config ];
          buildInputs = with pkgs;
            [ openssl libgit2 curl zlib ]
            ++ lib.optionals stdenv.isDarwin [
              libiconv
              apple-sdk_11
            ];
          preBuild = ''
            export CARGO_HOME="$TMPDIR/cargo-home"
            mkdir -p "$CARGO_HOME"
            cp ${cargoPatchConfig} "$CARGO_HOME/config.toml"
          '';
          doCheck = false;
        };
        codex-tui = rustPlatform.buildRustPackage (commonRustPackageArgs // {
          pname = "codex-tui";
          cargoBuildFlags = [ "--package" "codex-tui" "--bin" "codex-tui" ];
          meta = with pkgs.lib; {
            description = "Codex TUI built from codex-rs";
            homepage = "https://github.com/sourcegraph/codex";
            license = licenses.asl20;
            mainProgram = "codex-tui";
            platforms = platforms.unix;
          };
        });
        codex-cli = rustPlatform.buildRustPackage (commonRustPackageArgs // {
          pname = "codex-cli";
          cargoBuildFlags = [ "--package" "codex-cli" "--bin" "codex" ];
          meta = with pkgs.lib; {
            description = "Codex CLI built from codex-rs";
            homepage = "https://github.com/sourcegraph/codex";
            license = licenses.asl20;
            mainProgram = "codex";
            platforms = platforms.unix;
          };
        });
      in {
        packages = {
          codex-cli = codex-cli;
          codex-tui = codex-tui;
          default = codex-cli;
        };
        apps =
          let
            codexCliApp = flake-utils.lib.mkApp { drv = codex-cli; };
            codexApp = flake-utils.lib.mkApp { drv = codex-tui; };
          in {
            codex = codexCliApp;
            codex-cli = codexCliApp;
            codex-tui = codexApp;
            default = codexCliApp;
          };
      }
    );
}
