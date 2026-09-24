# Notes for coding agents

[CONTRIBUTING.md](CONTRIBUTING.md) applies. In addition:

- Prefer jj. Plan by change id, build by commit hash.
- When in doubt, ask rather than speculate. Be mindful of making accidental
  decisions. Be clear and explicit when a choice is arbitrary (e.g. choosing
  one approach over another because it's unclear which is better).
- Keep change descriptions up to date. Tag the subject of WIP changes with
  `[WIP]`, and track outstanding tasks in a checklist in the change
  description.
- Change sequences should make sense as a `git send-email` patch series, as if
  they are intended to be submitted to a mailing list such as LKML (but avoid
  trailer boilerplate).
- Be concise, precise, literal. Accommodate reviewers with ADHD and dyslexia.
  Avoid narrating, pointing out the obvious, mixed metaphors, flowery language.
- Do not introduce new jargon or terms.
- Prefer cross-referencing over restating, but avoid referring to ephemera.
  Everything must be reasonably intelligible to someone without shared
  context many years down the line.
- `nix flake check` builds everything. It's OK for checks to fail while
  iterating (WIP), but eventually every commit should pass every check.
- `nix build --no-link ".#checks.${system}.${check}"` can be used to run checks
  individually.
- Specify `?rev=` as in `nix build ".?rev=$commit_hash#checks..."` to
  authoritatively check a particular commit. This can be run in the background,
  especially when expected to pass, without interfering with the working copy.
  It never gets invalidated and doesn't pollute the local directory, so it is
  usually preferable over devshell usage.
- Aggregate checks include `quick`, `lint`, `tests`, `coverage` and `nightly`;
  see `nix flake show`.
