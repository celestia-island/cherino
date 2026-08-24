# Contributing to cherino

Thanks for your interest in improving `cherino`.

## Ground rules

- This repository's code is substantially AI-generated and licensed under
  [SySL-1.0](LICENSE). Derivatives must keep the AI-generation disclosure.
- Never commit real credentials, tokens, or internal network addresses.
  Use placeholders (`CHANGE_ME`, `test-password`, RFC 5737 `192.0.2.x`
  addresses) in examples and tests.

## Workflow

1. Branch off `master` (`feat/<name>`, `fix/<name>`, `chore/<name>`,
   `refactor/<name>`). The `dev` branch is deprecated.
2. Keep commits in the format `<gitmoji> <Capitalized English sentence
   ending with a period.>` — no `type:` prefixes, no CJK. PR titles follow
   the same rule.
3. Squash merges only; no merge commits.
4. Before opening a PR, verify locally:

   ```console
   just check        # cargo check --workspace --all-features
   just test         # cargo test --workspace
   just lint         # cargo fmt --all --check + clippy -D warnings
   ```

5. Do not maintain a CHANGELOG file — merged PRs are the changelog. Release
   notes live on git tags / GitHub Releases only.

## Code style

- Rust edition 2024, MSRV 1.91, `rustfmt` with `max_width = 100`.
- Clippy must pass with `-D warnings` across `--all-features --tests`.
- Brand-sensitive identifiers (AppArmor profile names, env overrides) carry
  legacy compatibility paths — extend, never silently rename them.
