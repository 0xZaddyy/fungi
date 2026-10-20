{ inputs, ... }:
{
  perSystem =
    { pkgs, ... }:
    {
      checks.no-fixme-comments = pkgs.runCommand "no-fixme-comments" { src = inputs.self; } ''
        if grep -Irn 'FIX[M]E' "$src" 2>/dev/null; then
          echo "FAIL: unresolved work-item markers found"
          exit 1
        fi
        mkdir -p "$out"
      '';
    };
}
