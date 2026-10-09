# hevy-axi agent guide

`hevy-axi` is an agent-ergonomic CLI over all 22 documented Hevy Public API operations, shipped as one native Rust binary. Keep the interface safe for unattended reads and deliberately gated mutations.

## Commands

```bash
devenv shell -- just --list
devenv shell -- just check
```

`just check` is the only gate: format, Clippy with warnings denied, all tests, release build. Tests run the real binary against a local mock server; never call the real Hevy API from a test. `just api-sync` and `just api-sync-check` need the network and stay out of `check`. Build binaries you distribute (`just install`, releases) outside `devenv shell`: inside it, macOS binaries link a `/nix/store` libiconv that other machines lack.

## Releases

Bump `version` in `Cargo.toml` (the lockfile follows), merge to `main`, then tag that commit `vX.Y.Z` and push the tag. `release.yml` refuses a tag whose core version differs from `Cargo.toml`, runs `just check`, builds darwin and linux archives for arm64 and amd64 on native runners, publishes the GitHub release with `checksums.txt`, and writes `Formula/hevy-axi.rb` in `koopycat/homebrew-tap` through the `HOMEBREW_APP_ID` and `HOMEBREW_APP_PRIVATE_KEY` secrets. A tag with a prerelease suffix (`v0.2.0-rc.1`) publishes a prerelease and leaves the tap alone. The binary's `--version` output is the bare version and the formula test asserts it.

## Invariants

- Never commit or print API keys, real Hevy IDs, or personal health/workout data; credential files remain local and mode `0600`. The CLI reads the key only from `HEVY_API_KEY` or the stored `~/.config/hevy-axi/credentials.env`, and the base URL only from `HEVY_API_BASE_URL`; never add a project `.env` or other file source. Gitleaks (`.gitleaks.toml`, pre-commit hooks, `.github/workflows/security.yml`) scans every commit; a finding is fixed, never allowlisted to get past it.
- Never add response, health-data, or synchronization caches. Commands read live state and redact credentials from failures.
- Never add Node, pnpm, TypeScript, or other runtime dependencies: the binary stays self-contained. This repository contains no JavaScript. macOS and Linux only.
- Treat the Hevy API as unstable: responses are read as plain JSON with tolerant envelope handling, not rigid types. Re-check the official contract before release; see `docs/hevy-api-analysis.md`.
- `docs/hevy-openapi.json` is the verbatim official OpenAPI document embedded in `https://api.hevyapp.com/docs/swagger-ui-init.js`, captured 2026-10-07. Never edit it by hand; replace it only with `just api-sync`. Unit tests in `resource.rs`, `compact.rs` and `mutate.rs` compare the hand-written tables with it, so a contract change fails `just check` until a person has reviewed those tables. After a sync that changes content, update the SHA and capture date here and in `docs/hevy-api-analysis.md`, and the analysis wherever the contract changed. The weekly workflow `.github/workflows/api-sync.yml` has two jobs. `refresh` runs `scripts/api-sync-refresh.sh` on the homelab runners (`runs-on: [self-hosted, linux, x64, homelab]`, which needs the repository topic `homelab-runner`), with no write credential, and uploads plain files. `publish` runs `scripts/api-sync-publish.sh` with the write token, from the default branch only. It builds on `automated/hevy-api-sync` (never force-pushes), opens or updates a PR from that branch, and closes the PR without deleting the branch when the difference is gone. A failing check opens the PR as a draft. Opening the PR needs the repository setting that lets Actions create pull requests.
- Keep POST and PUT free of automatic retries. Mutations require explicit `--confirm` or offline `--dry-run`; measurement update must retain its read-merge-write protection. `exercise create` checks every template title on `--confirm` (never on `--dry-run`) and refuses a match unless `--allow-duplicate` is given; if that scan cannot complete, nothing is created.
- Never write to stderr. Results and errors are TOON or JSON on stdout; exit codes are 0, 1 (runtime failure) and 2 (usage or unsafe local configuration). Field names, error codes and exit codes are a contract for agents.
- Dependency updates come from Renovate through the shared preset `github>koopycat/renovate-config` (`renovate.json`): one weekly PR for minor and patch updates, GitHub Actions in their own PR pinned to commit SHAs with a version comment (keep the version as the last text of the comment), majors only when ticked on the Dependency Dashboard. Do not add Dependabot version or security updates; the two bots would open the same PR.
- Keep `PROJECT.md` outcome and unchecked experiment decision current when a change alters the tested hypothesis or lifecycle.
