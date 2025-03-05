{ pkgs ? import <nixpkgs> {} }:

let
  lib = pkgs.lib;
  isDarwin = pkgs.stdenv.isDarwin;
  merged-openssl = pkgs.symlinkJoin {
    name = "merged-openssl";
    paths = [ pkgs.openssl.out pkgs.openssl.dev ];
  };
in pkgs.stdenv.mkDerivation rec {
  name = "spark-middleware";
  env = pkgs.buildEnv { name = name; paths = buildInputs; };

  buildInputs = [
    pkgs.clippy
    pkgs.rustc
    pkgs.cargo
    pkgs.openssl
    pkgs.lld
  ] ++ lib.optional isDarwin pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
    ++ lib.optional isDarwin pkgs.darwin.apple_sdk.frameworks.CoreFoundation
    ++ lib.optional isDarwin pkgs.darwin.apple_sdk.frameworks.Security;

  shellHook = ''
    export OPENSSL_DIR="${merged-openssl}"

    export RUST_LOG=debug
    FILE_LOG_LEVEL="debug"
    CONSOLE_LOG_LEVEL="debug"
    echo "RUST_LOG is set to $RUST_LOG"

    echo "middleware environment is ready."
  '';

  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [ pkgs.openssl pkgs.libiconv ];
}

