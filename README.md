# hevy-axi

`hevy-axi` is a single-binary CLI for all 22 operations in the Hevy Public API. It gives shell agents and humans compact, deterministic access to workouts, routines, exercise templates and history, routine folders, body measurements, and account metadata.

The interface follows the [AXI principles](https://axi.md/): minimize schema and token overhead, return structured errors and explicit empty states, and include useful next commands. Output is [TOON](https://toonformat.dev/) by default because AXI recommends it as a token-efficient alternative to JSON; JSON remains available for conventional tooling.

## API status

Hevy describes its [Public API](https://api.hevyapp.com/docs/) as an early rollout whose structure may change or be discontinued. Access currently requires **Hevy Pro** and an API key from [Hevy developer settings](https://hevy.com/settings?developer). The CLI tolerates a few known response variations, but it cannot make this experimental API stable.

## Install and run

`hevy-axi` is one self-contained native binary (macOS and Linux, about 2 MB) with no runtime to install. With Homebrew (macOS and Linux, arm64 and amd64):

```bash
brew install koopycat/tap/hevy-axi
hevy-axi --version
```

Or build it from a checkout with a Rust toolchain (1.96 or newer) and [`just`](https://just.systems):

```bash
just install          # builds target/release/hevy-axi and copies it to ~/.local/bin
hevy-axi --version
```

Or run it from source while developing:

```bash
just run --help
just run workout count
```

Releases are built by `.github/workflows/release.yml` (see AGENTS.md). To build a binary by hand, use `just build` and copy `target/release/hevy-axi`. Build it outside `devenv shell`: inside it, macOS binaries link a library from `/nix/store` that other machines lack. Outside it, the binary depends only on system libraries; TLS is built in (rustls), so no OpenSSL is needed.

## Secure API-key setup

Never put a key on the command line. The CLI reads the key from one of two sources, in precedence order:

1. `HEVY_API_KEY` in the process environment.
2. `~/.config/hevy-axi/credentials.env`, written by `setup key`.

A project `.env` is never read. For global storage, read the key without echoing it and send it over stdin:

```bash
read -rsp "Hevy API key: " HEVY_API_KEY; printf '\n'
printf '%s\n' "$HEVY_API_KEY" | hevy-axi setup key --confirm
unset HEVY_API_KEY
hevy-axi setup status
```

For one process, export `HEVY_API_KEY` through your shell or secret manager.

The stored file must be a regular file (never a symlink) owned by you with no group or world permissions (mode `0600`); otherwise the CLI refuses it. Setup secures the directory as `0700`, writes the file atomically as `0600`, and never returns the key. `setup remove-key --confirm` removes only the stored file, not an environment key.

## Commands and API coverage

Every resource supports `--help`. These actions map to the complete documented API:

| Resource      | Actions                                                         | API operations                                                    |
| ------------- | --------------------------------------------------------------- | ----------------------------------------------------------------- |
| `user`        | `info`                                                          | `GET /v1/user/info`                                               |
| `workout`     | `list`, `create`, `count`, `events`, `view <id>`, `update <id>` | 6 workout operations                                              |
| `routine`     | `list`, `create`, `view <id>`, `update <id>`                    | 4 routine operations                                              |
| `exercise`    | `list`, `create`, `view <id>`, `history <id>`                   | 3 exercise-template operations plus exercise history              |
| `folder`      | `list`, `create`, `view <id>`                                   | 3 routine-folder operations                                       |
| `measurement` | `list`, `create`, `view <YYYY-MM-DD>`, `update <YYYY-MM-DD>`    | 4 body-measurement operations                                     |
| `setup`       | `status`, `key`, `remove-key`                                   | Local credential setup; no Hevy API operation                     |

Running `hevy-axi` with no arguments shows the home view: whether a key is configured, its credential source, and next commands. It makes no API call and shows no account or workout data, so it is safe to run at the start of every agent session.

## Output, fields, and pagination

- TOON is the default. Use `--json` or `--format json` for JSON.
- Default views normalize inconsistent Hevy envelopes and select compact fields. `--fields id,title` projects comma-separated paths from that compact output.
- `--full` returns the untruncated wire-oriented payload and cannot be combined with `--fields`.
- `workout view` and `routine view` list each exercise's sets (type, weight, reps, distance, duration, and custom metric) without `--full`. Workout sets add RPE; routine sets add the planned rep range as `repRangeStart` and `repRangeEnd`.
- Long strings are truncated explicitly in compact output; use `--full` to bypass truncation.
- Lists accept `--page`, `--page-size`, `--limit`, and `--all`. `--all` starts at page 1 and cannot be combined with `--page`.
- When `--limit` stops inside a page, `hasMore` is true and `resume: {page, skip}` names that page and how many of its leading items were already returned; the continuation command re-reads that page. `totalCount` appears only when it is exact.
- API page sizes are at most 10 except exercise templates, which allow 100. Automatic pagination is capped at 500 pages and 5,000 items.
- Workout events accept `--since <ISO-8601>`. If omitted, the CLI still sends the Unix epoch explicitly.
- Exercise history is not paginated; constrain it with `--start` and `--end` when possible.

## Examples

Read compact data and select only needed fields:

```bash
hevy-axi workout count
hevy-axi workout list --page-size 10 --fields id,title,startTime
hevy-axi exercise list --all --limit 200 --json
hevy-axi workout events --since 2024-01-01T00:00:00Z
```

Preview a mutation without loading credentials or contacting Hevy:

```bash
payload="$(mktemp)"
printf '%s\n' '{"title":"CLI test folder"}' > "$payload"
hevy-axi folder create --file "$payload" --dry-run --full
rm -f "$payload"
```

To execute the same mutation, inspect the preview first and then opt in explicitly. Folder creation inserts at index 0 and reorders existing folders.

```bash
payload="$(mktemp)"
printf '%s\n' '{"title":"CLI test folder"}' > "$payload"
hevy-axi folder create --file "$payload" --dry-run
hevy-axi folder create --file "$payload" --confirm
rm -f "$payload"
```

Mutation input may also come from stdin with `--file -`. It must be a JSON object no larger than 1 MiB; regular files must not be symlinks. The CLI accepts either a bare resource object or the documented envelope where applicable.

## Mutation and retry safety

- Every POST or PUT requires `--file` and exactly one of `--dry-run` or `--confirm`. There are no prompts and no implicit writes.
- POST creates are non-idempotent and are **never retried**. If a response is lost, inspect Hevy before trying again; a retry may create a duplicate.
- PUT requests are also **never retried**. Workout updates use the complete workout write shape, and callers should treat routine updates as complete intended state because Hevy offers no revisions or conditional writes.
- Hevy defines measurement PUT as full replacement, where omitted values become null. The CLI protects partial updates by reading the existing date, merging supplied fields, and sending the resulting complete measurement; an explicit `null` clears a metric. Concurrent external edits can still be overwritten.
- Folder creation changes folder order by inserting the new folder first.
- The public API exposes no delete commands. Delete in Hevy itself where supported.
- The CLI keeps no response, health, or synchronization cache. Each command reads live state.
- GET requests alone may retry selected network, rate-limit, and transient gateway failures, at most twice. POST and PUT never retry.

## Errors and exit codes

Results and errors are structured as TOON or JSON on stdout so callers can parse both paths. Error objects contain a stable `code`, safe message, optional redacted details, and suggestions. The API key is centrally redacted from upstream error bodies. HTTP failures append Hevy's `{error}` text to the message; network failures report only the system error code, such as `ECONNREFUSED`, as `details.cause`.

| Exit | Meaning                                                                                                                                                                                                     |
| ---: | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
|  `0` | Success, including a closed downstream pipe (`EPIPE`)                                                                                                                                                       |
|  `1` | Configuration, network, timeout, protocol, or API failure: `BAD_REQUEST` (400), `AUTH_ERROR` (401), `FORBIDDEN` (403, including account limits), `NOT_FOUND`, `CONFLICT` (409), `RATE_LIMITED`, `API_ERROR` |
|  `2` | Invalid command, flag, argument, input, or unsafe local configuration (`CONFIG_INSECURE`)                                                                                                                   |

Unknown commands and flags fail loudly. Requests time out after 20 seconds, successful bodies are bounded to 10 MiB, and error bodies to 8 KiB.

## Ambient agent context

To show agents whether Hevy is configured at the start of a session, run `hevy-axi` with no arguments from your own SessionStart hook. The home view makes no API call and contains only configuration status and next commands, never the API key, account, or workout data. `hevy-axi` does not install hooks itself.

## Configuration

| Setting             | Purpose                                                         |
| ------------------- | --------------------------------------------------------------- |
| `HEVY_API_KEY`      | Preferred process-level API key                                 |
| `HEVY_API_BASE_URL` | Explicit API origin override; default `https://api.hevyapp.com` |

`HEVY_API_BASE_URL` is never discovered from Swagger or a response. Set it only to an endpoint you explicitly trust, because the API key is sent there. It must be HTTPS, contain no user information, query, or fragment, and trailing slashes are removed. Plain HTTP is accepted only for loopback test servers. It is read only from the environment, never from a file, so no file can send the key elsewhere.

TLS certificates are checked against a built-in set of public roots, not the system trust store, so a TLS-intercepting corporate proxy is not supported. Proxy environment variables (`HTTPS_PROXY`, `ALL_PROXY`, ...) are ignored on purpose: the API key is only ever sent directly to the base URL.

## Development

```bash
just test
just build
just check
```

`just check` runs format verification, Clippy with warnings denied, all unit and integration tests, and a release build. Integration tests run the real binary against a local mock server, so they need no API key or network.

`just api-sync-check` compares the committed OpenAPI capture with Hevy's live docs (network), and `just api-sync` adopts a change.

## Limitations and observed live quirks

- The API is experimental, has no declared server URL in OpenAPI, no stability or rate-limit contract, no idempotency keys, no conditional writes, and no deletes.
- The documented events envelope is `events`, but a live request that omits `since` can return `workouts`. The CLI always sends an explicit default `since` and tolerates either envelope.
- The documented folder-list envelope is `routine_folders`, but live responses may use the legacy key `routines`; the CLI accepts both.
- The API has no documented operation for Hevy's AI Trainer. This CLI does not claim that Trainer plans are available.
- Exercise history has no documented page or maximum range. Large unbounded histories can produce large responses.
- No persistent cache or sync cursor is maintained. Use explicit timestamps and reconcile important state directly with Hevy.

For the contract inventory, schema inconsistencies, documented behavior, live deviations, and design decisions, see [`docs/hevy-api-analysis.md`](docs/hevy-api-analysis.md).
