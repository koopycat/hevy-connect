# Hevy Public API analysis

> Scope: analysis of the captured contract plus separately identified live observations. **Fact** comes from the captured official contract or cited AXI source. **Observed live deviation** records behavior seen from the live API but not promised by the contract. **Decision** records implemented `hevy-axi` behavior, not a Hevy promise.

## Provenance, version, and confidence

**Facts**

- The API source is Hevy's official Swagger UI at [https://api.hevyapp.com/docs/](https://api.hevyapp.com/docs/). The local research capture is dated **2026-10-07** (from `.research/20261007-105219`; this is the capture date, not an asserted publication date).
- `.research/20261007-105219/hevy-openapi.json` is an OpenAPI **3.0.0** document titled “Hevy Public API Docs,” with API version **0.0.1**. It contains 14 paths, 22 operations, and 28 named schemas.
- `.research/20261007-105219/hevy-swagger-ui-init.js` embeds the same OpenAPI object byte-for-byte after JSON serialization. It adds only Swagger UI initialization; it does not reveal another API version, server URL, auth scheme, or rate limit.
- The AXI source is the official [https://axi.md/](https://axi.md/) page captured as `.research/20261007-105219/pages/axi.md`.
- The Hevy description explicitly says the public API is just beginning rollout, may change structure completely, and may be abandoned. It says access is currently limited to Hevy Pro users.
- The contract has no `servers` entry and no changelog or stability policy. A `/v1` path prefix does not override the explicit `0.0.1` and instability warning.

**Decision**

Treat this as an unstable, version-pinned adapter boundary. Default to the official HTTPS host and honor `HEVY_API_BASE_URL` only as an explicit, trusted override because the key is sent there. Retain tolerant wire decoders, keep Hevy-specific shapes behind internal domain types, and re-check the live contract before release.

## Complete operation inventory (22)

All paths below are relative because the OpenAPI document does not declare `servers`.

### Workouts (6)

| Operation                      | Purpose                                                      | Success shape                                                                                        | Documented failures                   |
| ------------------------------ | ------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------- | ------------------------------------- |
| `GET /v1/workouts`             | Page through workouts                                        | `200 {page, page_count, workouts: Workout[]}`                                                        | `400` invalid page size               |
| `POST /v1/workouts`            | Create workout                                               | `201 {workout: Workout[]}`; despite the singular key, the value is a documented single-element array | `400 {error}`                         |
| `GET /v1/workouts/count`       | Count all workouts                                           | `200 {workout_count}`                                                                                | none                                  |
| `GET /v1/workouts/events`      | Page through update/delete events since a time, newest first | `200 {page, page_count, events}`                                                                     | `500`                                 |
| `GET /v1/workouts/{workoutId}` | Fetch complete workout                                       | `200 Workout` (unwrapped)                                                                            | `404`                                 |
| `PUT /v1/workouts/{workoutId}` | Replace/update workout from the create request shape         | `200 Workout` (unwrapped)                                                                            | `400 {error}`; no `404` is documented |

### User (1)

| Operation           | Purpose                                       | Success shape          | Documented failures |
| ------------------- | --------------------------------------------- | ---------------------- | ------------------- |
| `GET /v1/user/info` | Fetch authenticated user and unit preferences | `200 {data: UserInfo}` | `404`               |

### Routines (4)

| Operation                      | Purpose               | Success shape                                 | Documented failures                                     |
| ------------------------------ | --------------------- | --------------------------------------------- | ------------------------------------------------------- |
| `GET /v1/routines`             | Page through routines | `200 {page, page_count, routines: Routine[]}` | `400` invalid page size                                 |
| `POST /v1/routines`            | Create routine        | `201 {routine: Routine}`                      | `400 {error}`, `403 {error}` routine limit              |
| `GET /v1/routines/{routineId}` | Fetch routine         | `200 {routine: Routine}`                      | `400 {error}` (oddly described as invalid request body) |
| `PUT /v1/routines/{routineId}` | Update routine        | `200 {routine: Routine}`                      | `400 {error}`, `404 {error}`                            |

### Exercise templates (3)

| Operation                                         | Purpose                          | Success shape                                                    | Documented failures                                |
| ------------------------------------------------- | -------------------------------- | ---------------------------------------------------------------- | -------------------------------------------------- |
| `GET /v1/exercise_templates`                      | Page through available templates | `200 {page, page_count, exercise_templates: ExerciseTemplate[]}` | `400` invalid page size                            |
| `POST /v1/exercise_templates`                     | Create custom template           | `200 {id: integer}` (not `201`, and not a template object)       | `400 {error}`, `403 {error}` custom-exercise limit |
| `GET /v1/exercise_templates/{exerciseTemplateId}` | Fetch template                   | `200 ExerciseTemplate` (unwrapped)                               | `404`                                              |

### Routine folders (3)

| Operation                            | Purpose                                       | Success shape                                              | Documented failures     |
| ------------------------------------ | --------------------------------------------- | ---------------------------------------------------------- | ----------------------- |
| `GET /v1/routine_folders`            | Page through folders                          | `200 {page, page_count, routine_folders: RoutineFolder[]}` | `400` invalid page size |
| `POST /v1/routine_folders`           | Create folder at index 0, shifting all others | `201 RoutineFolder` (unwrapped)                            | `400 {error}`           |
| `GET /v1/routine_folders/{folderId}` | Fetch folder                                  | `200 RoutineFolder` (unwrapped)                            | `404`                   |

### Exercise history (1)

| Operation                                       | Purpose                                                                    | Success shape                                                   | Documented failures                  |
| ----------------------------------------------- | -------------------------------------------------------------------------- | --------------------------------------------------------------- | ------------------------------------ |
| `GET /v1/exercise_history/{exerciseTemplateId}` | History filtered by optional `start_date` / `end_date` ISO-8601 date-times | `200 {exercise_history: ExerciseHistoryEntry[]}`; not paginated | `400` invalid parameters/date format |

### Body measurements (4)

| Operation                          | Purpose                                       | Success shape                                                  | Documented failures                              |
| ---------------------------------- | --------------------------------------------- | -------------------------------------------------------------- | ------------------------------------------------ |
| `GET /v1/body_measurements`        | Page through measurements                     | `200 {page, page_count, body_measurements: BodyMeasurement[]}` | `400`, `404` page not found                      |
| `POST /v1/body_measurements`       | Create one date-keyed measurement             | `201` with an explicitly empty body                            | `400 {error}`, `409 {error}` date already exists |
| `GET /v1/body_measurements/{date}` | Fetch by `YYYY-MM-DD`                         | `200 BodyMeasurement` (unwrapped)                              | `404 {error}`                                    |
| `PUT /v1/body_measurements/{date}` | Fully overwrite measurement values for a date | `200` with no response schema                                  | `400 {error}`, `404 {error}`                     |

No delete operation is exposed. Workout deletion can nevertheless appear in the event feed, presumably when deletion occurs elsewhere in Hevy.

## Authentication, transport, pagination, and rate etiquette

### Contract facts

- Every operation requires an `api-key` request header. Each operation repeats an inline string schema with `format: uuid`; there is no OpenAPI `securitySchemes` declaration and no global `security` requirement.
- The description directs Pro users to obtain a key in Hevy's web settings. The spec does not document key scopes, rotation, expiry, auth failure status/body, or whether access can be delegated.
- JSON request bodies use `application/json`. The spec does not describe other media types.
- The five ordinary paged collections and workout events use one-based `page` (default 1) plus camel-case `pageSize`. Maximum/default size is 10 for workouts, events, routines, routine folders, and body measurements. Exercise templates default to and allow 100.
- Paged successes use snake-case `page_count`; they do not include item totals, next links, cursors, or snapshot tokens. Only workouts have a separate total-count operation.
- Exercise history is date-filterable but unpaginated. `GET /workouts/events` has a `since` string defaulting to the Unix epoch, is newest-first, and is intended for incremental cache maintenance.
- There is no numeric rate limit, `429` response, rate-limit header, or `Retry-After` behavior documented. The only explicit etiquette is: scheduled hourly/daily updates should **not** run exactly at `xx:00`; choose a random minute.

### Client decisions

- Load the key only from a secret-capable environment/config provider. Never accept it as a positional CLI argument, print it, persist it in logs/fixtures, place it in URLs, or include it in structured errors. Redact the `api-key` header centrally.
- Require HTTPS and make the base URL explicit/configurable because `servers` is absent. Do not follow cross-origin redirects while retaining the key.
- Validate `page >= 1`; clamp/reject `pageSize` above the resource maximum rather than relying on the server. Stop when `page >= page_count`, not when a page happens to be short. Represent an empty result explicitly.
- Bound all automatic pagination by both pages and records. Return truncation metadata and a continuation command instead of silently returning a partial collection.
- A changing, newest-first page feed can shift while pages are read. For event sync, overlap the previous watermark, deduplicate by stable event content (event type, entity ID, and `updated_at`/`deleted_at` where available), finish all reported pages before advancing the watermark, and periodically reconcile against workout count/full reads. The contract does not define whether `since` is inclusive, so never assume an exact boundary is lossless.
- Add jitter to scheduled syncs well away from the top of the hour. Keep concurrency low, cache templates/folders, coalesce duplicate reads, and honor `Retry-After` if the server supplies it even though it is undocumented.

## Exact wire-shape quirks

### Request envelopes (facts)

- Workout create and update both require the outer shape `{workout: {...}}`. Within `workout`, only `title`, `start_time`, `end_time`, and `exercises` are marked required. Optional `description` is nullable; optional `is_private` defaults to `false`.
- Workout exercise/set members are not marked required. Set metrics are nullable and include weight, reps, distance, duration, custom metric, and RPE. Set `type` is one of `warmup`, `normal`, `failure`, `dropset`; RPE is limited to 6–10 in half-point steps except 6.5 is absent.
- Routine create is `{routine: {...}}`; update has the same outer envelope but a distinct schema. Neither outer nor inner routine fields are marked required. `folder_id: null` selects the default “My Routines” folder. Routine sets have `rep_range`, not RPE.
- Folder create is `{routine_folder: {title}}`, but neither object nor title is marked required.
- Custom-template create is `{exercise: {title, exercise_type, equipment_category, muscle_group, other_muscles}}`; malformed `required` metadata means the document does not validly declare those properties required.
- Body-measurement create is the **raw** `BodyMeasurement`, not an envelope, and requires `date`. Update is the raw `PutBodyMeasurement` without `date`; the date is only in the path.

### Response envelopes (facts)

There is no uniform data envelope:

- Lists: `{page,page_count,<plural-array>}`.
- Workout create: `{workout:[...]}`; workout get/update: raw `Workout`.
- Routine create/get/update: `{routine:{...}}`.
- Template create: `{id}`; template get: raw object.
- Folder create/get: raw object.
- User: `{data:{...}}`; history: `{exercise_history:[...]}`; count: `{workout_count}`.
- Measurement create is explicitly bodyless; measurement update has no response schema. A successful client must not require JSON for either.
- Errors are inconsistently modeled. Some 4xx responses specify `{error:string}`; several 400/404/500 responses specify no content at all. Auth errors, 429, and generic 5xx are absent from most operations.
- Workout events are `{page,page_count,events}`. An updated event is `{type,workout}` with a full `Workout`; a deleted event is `{type,id,deleted_at}`. The two variants use `oneOf`, but `type` is only illustrated, not constrained by an enum or OpenAPI discriminator.

**Decision**

Decode each operation at its documented boundary, then normalize internally. Treat empty bodies as valid where documented; elsewhere, parse JSON only when the content type/body permits. Preserve unknown fields, tolerate newly added enum values as `unknown(raw)`, and produce a typed protocol error when a required runtime value is absent. Do not “fix” envelopes globally (for example, never assume every success is under `data`).

## Schema oddities and inconsistencies

**Facts**

1. `CustomExerciseType`, `MuscleGroup`, and `EquipmentCategory` say `type: enum`; OpenAPI 3.0 requires a primitive type such as `string` plus `enum`. Strict generators may reject or misgenerate them.
2. `CreateCustomExerciseRequestBody.exercise` contains `required: true`; for an object schema, `required` must be an array on the containing object. Most request and response properties are consequently optional according to the literal schema.
3. `workoutId`, `routineId`, `exerciseTemplateId`, and `folderId` path parameters omit `schema` entirely. The body-measurement `date` path parameter correctly supplies one. This is invalid/incomplete parameter modeling.
4. Date/time strings are inconsistent: filter parameters use `format: date-time` and measurement dates use `format: date`, while workout/routine/folder timestamps, event timestamps, `since`, and workout write timestamps are plain strings despite ISO-8601 descriptions/examples.
5. IDs vary by resource and even by operation. Workout/routine/template IDs are generally strings; folder IDs and `folder_id` are `number`; custom-template creation returns an integer `id` while `ExerciseTemplate.id` is a string. Superset IDs and indexes are also `number`, often where integer would be expected.
6. Metric types differ between writes and reads. Workout request reps/distance/duration are integers, while `Workout`/`Set` responses model them as numbers; exercise history returns those fields as integers. Indexes are numbers. Clients should not infer integrality from one representation.
7. The write-only/read-only model is asymmetric. Workout input accepts `is_private`, but `Workout` output does not expose it. Routine input accepts top-level `notes`, but `Routine` output has no top-level notes property. Routine sets support `rep_range`; workout sets support RPE instead.
8. Routine-create `notes` is non-nullable while routine-update `notes` is nullable. Almost no object has meaningful `required` lists, and none closes unknown properties with `additionalProperties: false`.
9. `BodyMeasurement` suffixes weight and several dimensions with units (`_kg`, `_cm`), but `abdomen`, `waist`, `hips`, thighs, and calves have no unit suffix or description. Their unit must not be guessed from the field name alone.
10. Measurement `PUT` explicitly says **all fields are overwritten and omitted fields become null**. This is replacement behavior, not patch behavior.
11. The four set types are actual enums on write schemas but only prose on response schemas. Event `type` likewise lacks an enum. Future values can therefore appear without violating the response schema.
12. Routine-folder creation changes the index of every existing folder. Treat folder index as mutable presentation order, never identity.
13. `GET /routines/{routineId}` documents a `400 “Invalid request body”` despite having no body and does not document `404`; workout update omits `404`; status coverage is not systematic.
14. Exercise history has no pagination or maximum range documented, making an unbounded all-time request potentially large.
15. The spec defines no ETags, revision numbers, conditional headers, idempotency keys, bulk operations, deletes, webhooks, or optimistic-concurrency mechanism.

## Safe client behavior

### Reads (decisions)

- Default to read-only commands. Validate IDs and dates locally, URL-encode path components, send UTC RFC 3339 date-times, and retain the original date semantics for `YYYY-MM-DD` measurements.
- Page lazily and deterministically; expose page metadata. Use the 100-item maximum for templates and at most 10 elsewhere, but allow lower caller limits.
- Do not cache API responses by default because workouts and body measurements are sensitive. Consumers that explicitly persist data should use workout events only as a delta hint and periodically reconcile fully because the feed has no cursor/snapshot guarantee.
- Exercise history is unpaginated. Compact output is capped and reports omissions; use `--start`/`--end` to bound the server response and `--full` only when the complete history is intentionally required.
- Distinguish `not_found`, `validation`, `auth`, `forbidden_or_limit`, `conflict`, `rate_limited`, `server`, `network`, and `protocol` failures even when the upstream body is empty. Preserve status and a redacted request ID/header if available.

### Writes and concurrency (decisions)

- Put all writes behind explicit intent (`--execute` or `--confirm`), with non-interactive `--dry-run` showing a redacted normalized payload and effects. Never let “no arguments” mutate.
- Before workout/routine/measurement updates, read current state, calculate a field-level diff, and send a complete validated replacement. For measurement `PUT`, require callers to acknowledge that omitted metrics become null; preferably merge desired changes with the current object client-side, then show the resulting full replacement.
- There is no server-side compare-and-swap. Warn when source state changed between read and write where a detectable timestamp exists; otherwise document last-write-wins risk. Do not claim transactional safety.
- POST workout, routine, folder, and custom-template operations are not idempotent and offer no idempotency key. After an ambiguous timeout, do not blindly retry: reconcile by a narrowly matching read and ask for confirmation if uniqueness cannot be proved.
- Measurement create has a natural date uniqueness constraint. On timeout or `409`, `GET` that date and compare normalized values; report “already applied” only on an exact match.
- PUT is repeatable only with the identical complete body and target, but concurrent edits may still be overwritten. After timeout, read back and compare before retrying. Folder creation additionally reorders other folders.

### Retry policy (decisions)

- Retry GET on transport failures, `408`, `429`, and transient `5xx` with capped exponential backoff and full jitter; honor `Retry-After`; cap attempts and total elapsed time.
- Do not retry ordinary `4xx`. A `403` on create can mean an account limit, not transient authorization. Surface body text only after sanitization.
- Never automatically retry POST after bytes may have reached the server. Retry PUT only under the read-back rule above. Do not retry schema/protocol errors.
- Keep retries observable through attempt counts and final structured errors, but never log headers or sensitive response content.

## Mapping AXI principles to this API

The first clause in each row is an **AXI fact** from the official source; the second is **our design decision** for a Hevy-facing CLI.

| AXI principle                       | Hevy client mapping                                                                                                                                                                                      |
| ----------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1. Token-efficient output           | Default to compact TOON; offer JSON for machines. Never dump raw full workouts by default.                                                                                                               |
| 2. Minimal default schemas          | Lists show 3–4 useful fields plus stable ID; `--fields`/`--full` opts into metrics, notes, or nested sets.                                                                                               |
| 3. Content truncation               | Truncate notes/descriptions and large exercise/set lists with exact omitted counts and a `--full` escape hatch.                                                                                          |
| 4. Pre-computed aggregates          | Include `shown`, `page`, `pageCount`, and available total. Use `/workouts/count`; compute concise set/exercise summaries locally without extra API calls.                                                |
| 5. Definitive empty states          | Emit typed arrays with `count: 0` and the applied filters, never blank stdout.                                                                                                                           |
| 6. Structured errors and exit codes | Emit redacted structured errors on stdout; logs on stderr; exit 0/1 and 2 for usage. Reconcile ambiguous writes rather than pretending unsafe POSTs are idempotent.                                      |
| 7. Ambient context                  | Make setup opt-in and directory/account-profile scoped. Surface only connection/read-only status and never inject keys or health/workout details into session context.                                   |
| 8. Content first                    | No-args shows a privacy-safe home view: executable, profile alias, auth availability, mode, and suggested read commands—not help alone and not personal records.                                         |
| 9. Contextual disclosure            | Append concrete next commands carrying safe filters/profile flags, while leaving IDs as placeholders unless returned by the current result. Suggest `view`, pagination, sync, or dry-run as appropriate. |
| 10. Consistent help                 | Every command supports concise `--help`; unknown commands/flags fail loudly rather than silently widening a query.                                                                                       |

Additional design consequences: combine action and observation (`create` returns normalized created identity, `update` returns a verified diff), support shell filtering without leaking secrets, and keep default output bounded. Because Hevy data can be sensitive, privacy and write safety take priority where a literal “live data” home view would conflict with AXI's content-first guidance.

## Bottom line

**Fact:** the captured API is useful but explicitly experimental: 22 operations cover reads plus a small set of creates/updates, with low page limits, inconsistent envelopes, incomplete OpenAPI typing, no formal rate contract, no concurrency controls, and no general idempotency support.

**Decision:** build a conservative adapter: read-first, bounded, jittered, secret-safe, tolerant on input, strict before writes, explicit about replacement semantics, and reconciliation-driven after ambiguous failures. The AXI layer should hide wire inconsistency without hiding uncertainty or destructive consequences.
