{
  imports = [
    ./actionlint.nix
    ./fetch-validation-history.nix
    ./aggregates.nix
    ./build.nix
    ./cargo-shear.nix
    ./cargo-sort.nix
    ./clippy.nix
    ./coverage
    ./doc.nix
    ./mutants.nix
    ./no-fixme-comments.nix
    ./registry.nix
    ./tests
  ];
}
