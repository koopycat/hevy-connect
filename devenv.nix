{ pkgs, ... }:

{
  dotenv.disableHint = true;

  packages = with pkgs; [
    rustc
    cargo
    rustfmt
    clippy
    rust-analyzer
    just
  ];
}
