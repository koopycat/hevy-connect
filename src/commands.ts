import { constants as fsConstants } from "node:fs";
import { lstat, open, type FileHandle } from "node:fs/promises";
import { homedir } from "node:os";
import { resolve } from "node:path";

import {
  installSessionStartHooks,
  sessionStartHookStatus,
  uninstallSessionStartHooks,
} from "axi-sdk-js";

import {
  COMMON_OUTPUT_FLAGS,
  booleanFlag,
  combineFlags,
  outputFormat,
  parseArgs,
  positiveSafeInteger,
  requirePositionalCount,
  stringFlag,
  type FlagDefinition,
  type OutputFormat,
  type ParsedArgs,
} from "./args.js";
import type { HevyClient } from "./client.js";
import {
  removeStoredApiKey,
  requireApiKey,
  resolveConfig,
  writeStoredApiKey,
  type ResolvedConfig,
} from "./config.js";
import { HevyCliError, validationError } from "./errors.js";
import { commandHelp, TOP_LEVEL_HELP } from "./help.js";
import {
  finalizeOutput,
  formatCliError,
  formatResult,
  parseFields,
  projectFields,
} from "./output.js";
import type { JsonObject, JsonValue } from "./types.js";
import { VERSION } from "./version.js";

export { commandHelp, formatCliError, TOP_LEVEL_HELP };
export type { OutputFormat } from "./args.js";

const MAX_INPUT_BYTES = 1024 * 1024;
const MAX_AUTO_PAGES = 500;
const MAX_AUTO_ITEMS = 5000;
const HOOK_MARKER = "hevy-axi";
const BINARY_NAMES = ["hevy-axi"];
const DEFAULT_EVENTS_SINCE = "1970-01-01T00:00:00Z";

const AVAILABLE_COMPACT_FIELDS: Readonly<
  Record<CompactKind, readonly string[]>
> = {
  workout: ["id", "title", "startTime", "durationMinutes", "exerciseCount"],
  routine: ["id", "title", "folderId", "exerciseCount"],
  exercise: ["id", "title", "type", "primaryMuscle", "equipment", "isCustom"],
  folder: ["id", "index", "title"],
  measurement: ["date", "weightKg", "fatPercent", "waist"],
  event: ["type", "id", "time", "title"],
  history: [
    "workoutId",
    "workoutTitle",
    "workoutStartTime",
    "setType",
    "weightKg",
    "reps",
    "distanceMeters",
    "durationSeconds",
    "rpe",
    "customMetric",
  ],
};
const DEFAULT_COMPACT_FIELDS: Readonly<Record<CompactKind, readonly string[]>> =
  {
    workout: ["id", "title", "startTime", "exerciseCount"],
    routine: ["id", "title", "folderId", "exerciseCount"],
    exercise: ["id", "title", "primaryMuscle", "equipment"],
    folder: ["id", "index", "title"],
    measurement: ["date", "weightKg", "fatPercent", "waist"],
    event: ["type", "id", "time", "title"],
    history: ["workoutId", "workoutStartTime", "weightKg", "reps"],
  };
const MEASUREMENT_FIELDS = [
  "weight_kg",
  "lean_mass_kg",
  "fat_percent",
  "neck_cm",
  "shoulder_cm",
  "chest_cm",
  "left_bicep_cm",
  "right_bicep_cm",
  "left_forearm_cm",
  "right_forearm_cm",
  "abdomen",
  "waist",
  "hips",
  "left_thigh",
  "right_thigh",
  "left_calf",
  "right_calf",
] as const;
const MEASUREMENT_FIELD_SET = new Set<string>(MEASUREMENT_FIELDS);

const USER_FIELDS = [
  "id",
  "username",
  "name",
  "url",
  "weight_unit",
  "distance_unit",
] as const;

interface ClientLike {
  get<T>(
    path: string,
    options?: {
      query?: Readonly<
        Record<string, string | number | boolean | null | undefined>
      >;
    },
  ): Promise<T>;
  post<T>(path: string, options?: { body?: JsonValue }): Promise<T>;
  put<T>(path: string, options?: { body?: JsonValue }): Promise<T>;
}

export interface CommandDependencies {
  readonly clientFactory: (config: ResolvedConfig) => ClientLike | HevyClient;
  readonly resolveConfig?: () => Promise<ResolvedConfig>;
  readonly writeStoredApiKey?: (
    apiKey: string,
  ) => Promise<{ status: "created" | "updated"; path: string }>;
  readonly removeStoredApiKey?: () => Promise<{
    status: "removed" | "not_found";
    path: string;
  }>;
  readonly stdin?: NodeJS.ReadableStream;
  readonly cwd?: string;
  readonly homeDir?: string;
  readonly execPath?: string;
  readonly hooks?: {
    readonly install: typeof installSessionStartHooks;
    readonly status: typeof sessionStartHookStatus;
    readonly uninstall: typeof uninstallSessionStartHooks;
  };
}

export type CommandHandler = (
  args: string[],
  context?: unknown,
) => Promise<Record<string, unknown> | string>;

export type CommandMap = Record<string, CommandHandler>;

type CompactKind =
  | "workout"
  | "routine"
  | "exercise"
  | "folder"
  | "measurement"
  | "event"
  | "history";

interface ListSpec {
  readonly path: string;
  readonly arrayKey: string;
  readonly maximumPageSize: number;
  readonly defaultPageSize?: number;
  readonly kind: CompactKind;
  readonly helpName: string;
  readonly actionName?: string;
  readonly supportsView?: boolean;
  readonly extraQuery?: Readonly<Record<string, string>>;
  readonly legacyArrayKey?: string;
  readonly exactCount?: (client: ClientLike) => Promise<number>;
}

const HELP_FLAG: Readonly<Record<string, FlagDefinition>> = {
  help: { kind: "boolean" },
};
const LIST_FLAGS = combineFlags(COMMON_OUTPUT_FLAGS, HELP_FLAG, {
  page: { kind: "string" },
  "page-size": { kind: "string" },
  limit: { kind: "string" },
  all: { kind: "boolean" },
});
const MUTATION_FLAGS = combineFlags(COMMON_OUTPUT_FLAGS, HELP_FLAG, {
  file: { kind: "string" },
  confirm: { kind: "boolean" },
  "dry-run": { kind: "boolean" },
});
const READ_FLAGS = combineFlags(COMMON_OUTPUT_FLAGS, HELP_FLAG);
const SETUP_FLAGS = combineFlags(COMMON_OUTPUT_FLAGS, HELP_FLAG, {
  confirm: { kind: "boolean" },
});
const UPDATE_FLAGS: Readonly<Record<string, FlagDefinition>> = {
  check: { kind: "boolean" },
  json: { kind: "boolean" },
  help: { kind: "boolean" },
};

function objectValue(value: JsonValue, description: string): JsonObject {
  if (value === null || Array.isArray(value) || typeof value !== "object") {
    throw new HevyCliError(
      "PROTOCOL_ERROR",
      `Hevy returned an invalid ${description}.`,
    );
  }
  return value;
}

function arrayValue(
  value: JsonValue | undefined,
  description: string,
): JsonValue[] {
  if (!Array.isArray(value)) {
    throw new HevyCliError(
      "PROTOCOL_ERROR",
      `Hevy returned an invalid ${description}.`,
    );
  }
  return value;
}

function scalar(value: JsonValue | undefined): JsonValue {
  return value === undefined || (value !== null && typeof value === "object")
    ? null
    : value;
}

function stringValue(value: JsonValue | undefined): string | null {
  return typeof value === "string" ? value : null;
}

function arrayLength(value: JsonValue | undefined): number {
  return Array.isArray(value) ? value.length : 0;
}

function compactWorkout(value: JsonValue, detail = false): JsonObject {
  const workout = objectValue(value, "workout");
  const exercises = Array.isArray(workout.exercises) ? workout.exercises : [];
  const start = stringValue(workout.start_time);
  const end = stringValue(workout.end_time);
  const duration =
    start !== null && end !== null
      ? (Date.parse(end) - Date.parse(start)) / 60000
      : Number.NaN;
  const compact: JsonObject = {
    id: scalar(workout.id),
    title: scalar(workout.title),
    startTime: scalar(workout.start_time),
    durationMinutes: Number.isFinite(duration)
      ? Math.max(0, Math.round(duration * 10) / 10)
      : null,
    exerciseCount: exercises.length,
  };
  if (detail) {
    compact.endTime = scalar(workout.end_time);
    compact.description = scalar(workout.description);
    compact.setCount = exercises.reduce<number>((count, exercise) => {
      if (
        exercise === null ||
        Array.isArray(exercise) ||
        typeof exercise !== "object"
      ) {
        return count;
      }
      return count + arrayLength(exercise.sets);
    }, 0);
    compact.exercises = exercises.map((exercise) => {
      const item = objectValue(exercise, "workout exercise");
      return {
        title: scalar(item.title),
        exerciseTemplateId: scalar(item.exercise_template_id),
        setCount: arrayLength(item.sets),
      };
    });
  }
  return compact;
}

function compactRoutine(value: JsonValue, detail = false): JsonObject {
  const routine = objectValue(value, "routine");
  const compact: JsonObject = {
    id: scalar(routine.id),
    title: scalar(routine.title),
    folderId: scalar(routine.folder_id),
    exerciseCount: arrayLength(routine.exercises),
  };
  if (detail) {
    compact.exercises = Array.isArray(routine.exercises)
      ? routine.exercises.map((entry) => {
          const exercise = objectValue(entry, "routine exercise");
          return {
            title: scalar(exercise.title),
            exerciseTemplateId: scalar(exercise.exercise_template_id),
            setCount: arrayLength(exercise.sets),
          };
        })
      : [];
  }
  return compact;
}

function compactExercise(value: JsonValue): JsonObject {
  const exercise = objectValue(value, "exercise template");
  return {
    id: scalar(exercise.id),
    title: scalar(exercise.title),
    type: scalar(exercise.type),
    primaryMuscle: scalar(exercise.primary_muscle_group),
    equipment: scalar(exercise.equipment),
    isCustom: scalar(exercise.is_custom),
  };
}

function compactFolder(value: JsonValue): JsonObject {
  const folder = objectValue(value, "routine folder");
  return {
    id: scalar(folder.id),
    index: scalar(folder.index),
    title: scalar(folder.title),
  };
}

function compactMeasurement(value: JsonValue): JsonObject {
  const measurement = objectValue(value, "body measurement");
  return {
    date: scalar(measurement.date),
    weightKg: scalar(measurement.weight_kg),
    fatPercent: scalar(measurement.fat_percent),
    waist: scalar(measurement.waist),
  };
}

function compactEvent(value: JsonValue): JsonObject {
  const event = objectValue(value, "workout event");
  const workout =
    event.workout === undefined
      ? undefined
      : objectValue(event.workout, "event workout");
  return {
    type: scalar(event.type),
    id: scalar(workout?.id ?? event.id),
    time: scalar(workout?.updated_at ?? event.deleted_at),
    title: scalar(workout?.title),
  };
}

function compactHistory(value: JsonValue): JsonObject {
  const history = objectValue(value, "exercise history entry");
  return {
    workoutId: scalar(history.workout_id),
    workoutTitle: scalar(history.workout_title),
    workoutStartTime: scalar(history.workout_start_time),
    setType: scalar(history.set_type),
    weightKg: scalar(history.weight_kg),
    reps: scalar(history.reps),
    distanceMeters: scalar(history.distance_meters),
    durationSeconds: scalar(history.duration_seconds),
    rpe: scalar(history.rpe),
    customMetric: scalar(history.custom_metric),
  };
}

function compact(
  value: JsonValue,
  kind: CompactKind,
  detail = false,
): JsonObject {
  switch (kind) {
    case "workout":
      return compactWorkout(value, detail);
    case "routine":
      return compactRoutine(value, detail);
    case "exercise":
      return compactExercise(value);
    case "folder":
      return compactFolder(value);
    case "measurement":
      return compactMeasurement(value);
    case "event":
      return compactEvent(value);
    case "history":
      return compactHistory(value);
  }
}

function validateIdentifier(value: string, name: string): string {
  if (value.trim() === "" || /[\0\r\n]/u.test(value)) {
    throw validationError(`${name} is invalid.`);
  }
  return encodeURIComponent(value);
}

function validateDate(value: string): string {
  if (!/^\d{4}-\d{2}-\d{2}$/u.test(value)) {
    throw validationError("Date must use YYYY-MM-DD format.");
  }
  const date = new Date(`${value}T00:00:00Z`);
  if (
    Number.isNaN(date.valueOf()) ||
    date.toISOString().slice(0, 10) !== value
  ) {
    throw validationError(
      "Date must be a valid calendar date in YYYY-MM-DD format.",
    );
  }
  return value;
}

function validateIso(
  value: string | undefined,
  name: string,
): string | undefined {
  if (value !== undefined && Number.isNaN(Date.parse(value))) {
    throw validationError(
      `${name} must be a valid ISO-8601 date or timestamp.`,
    );
  }
  return value;
}

function ensureAction(args: readonly string[], command: string): string {
  const action = args[0];
  if (action === undefined) {
    throw validationError(
      `An action is required. Run "hevy-axi ${command} --help".`,
    );
  }
  return action;
}

function commonOptions(parsed: ParsedArgs): {
  format: OutputFormat;
  full: boolean;
  fields?: string[];
} {
  const format = outputFormat(parsed);
  const full = booleanFlag(parsed, "full");
  const fields = parseFields(stringFlag(parsed, "fields"));
  if (full && fields !== undefined) {
    throw validationError("--full cannot be combined with --fields.");
  }
  return { format, full, ...(fields === undefined ? {} : { fields }) };
}

async function configuredClient(
  deps: CommandDependencies,
): Promise<ClientLike> {
  const config = await (deps.resolveConfig ?? resolveConfig)();
  requireApiKey(config);
  return deps.clientFactory(config);
}

function helpResult(command: string): string {
  return commandHelp(command) ?? TOP_LEVEL_HELP;
}

async function listResource(
  client: ClientLike,
  parsed: ParsedArgs,
  spec: ListSpec,
): Promise<JsonValue> {
  const page = positiveSafeInteger(stringFlag(parsed, "page"), "--page") ?? 1;
  const pageSize =
    positiveSafeInteger(stringFlag(parsed, "page-size"), "--page-size") ??
    spec.defaultPageSize ??
    spec.maximumPageSize;
  const limit = positiveSafeInteger(stringFlag(parsed, "limit"), "--limit");
  const all = booleanFlag(parsed, "all");
  if (pageSize > spec.maximumPageSize) {
    throw validationError(
      `--page-size must not exceed ${spec.maximumPageSize}.`,
    );
  }
  if (all && stringFlag(parsed, "page") !== undefined) {
    throw validationError("--all cannot be combined with --page.");
  }
  if (limit !== undefined && limit > MAX_AUTO_ITEMS) {
    throw validationError(`--limit must not exceed ${MAX_AUTO_ITEMS}.`);
  }

  const startPage = all ? 1 : page;
  let currentPage = startPage;
  let pageCount = startPage;
  let fetchedPages = 0;
  const items: JsonValue[] = [];
  let fetching = true;
  while (fetching) {
    if (fetchedPages >= MAX_AUTO_PAGES) {
      throw validationError(
        `Automatic pagination exceeds the safety cap of ${MAX_AUTO_PAGES} pages.`,
      );
    }
    const response = await client.get<JsonValue>(spec.path, {
      query: { page: currentPage, pageSize, ...(spec.extraQuery ?? {}) },
    });
    const wire = objectValue(response, `${spec.helpName} list response`);
    const wirePage = wire.page;
    const wirePageCount = wire.page_count;
    const validEmptyPage = wirePage === 1 && wirePageCount === 0;
    if (
      !Number.isSafeInteger(wirePage) ||
      Number(wirePage) !== currentPage ||
      !Number.isSafeInteger(wirePageCount) ||
      Number(wirePageCount) < 0 ||
      (!validEmptyPage && Number(wirePageCount) < Number(wirePage))
    ) {
      throw new HevyCliError(
        "PROTOCOL_ERROR",
        "Hevy returned invalid pagination metadata.",
      );
    }
    pageCount = Number(wirePageCount);
    let pageItems: JsonValue[];
    if (
      spec.kind === "event" &&
      !Array.isArray(wire[spec.arrayKey]) &&
      Array.isArray(wire.workouts)
    ) {
      pageItems = wire.workouts.map((item) => {
        const legacy = objectValue(item, "legacy workout event");
        return typeof legacy.type === "string"
          ? legacy
          : { type: "updated", workout: legacy };
      });
    } else {
      const documentedItems = wire[spec.arrayKey];
      const listItems =
        documentedItems !== undefined || spec.legacyArrayKey === undefined
          ? documentedItems
          : wire[spec.legacyArrayKey];
      pageItems = arrayValue(listItems, `${spec.helpName} array`);
    }
    if (validEmptyPage && pageItems.length !== 0) {
      throw new HevyCliError(
        "PROTOCOL_ERROR",
        "Hevy returned invalid pagination metadata.",
      );
    }
    for (const item of pageItems) {
      if (items.length >= MAX_AUTO_ITEMS) {
        throw validationError(
          `Automatic pagination exceeds the safety cap of ${MAX_AUTO_ITEMS} items.`,
        );
      }
      if (limit === undefined || items.length < limit) {
        items.push(item);
      }
    }
    fetchedPages += 1;
    if (
      !all ||
      currentPage >= pageCount ||
      (limit !== undefined && items.length >= limit)
    ) {
      fetching = false;
    } else {
      // Progress by the requested page, independently of potentially clamped or
      // stale response metadata. The page/item caps remain the final guard.
      currentPage += 1;
    }
  }

  const limitReached =
    limit !== undefined && items.length >= limit && currentPage < pageCount;
  const hasMore = currentPage < pageCount;
  const options = commonOptions(parsed);
  if (options.full) {
    return {
      page: startPage,
      pageCount,
      resultCount: items.length,
      empty: items.length === 0,
      hasMore,
      [spec.arrayKey]: items,
    };
  }
  const compactResults = items.map((item) => compact(item, spec.kind));
  const selectedResults = projectFields(
    compactResults,
    options.fields ?? DEFAULT_COMPACT_FIELDS[spec.kind],
    AVAILABLE_COMPACT_FIELDS[spec.kind],
  ) as JsonValue[];
  const output: JsonObject = {
    page: startPage,
    pageCount,
    resultCount: items.length,
    empty: items.length === 0,
    hasMore,
    results: selectedResults,
  };
  if (all && !limitReached && currentPage >= pageCount) {
    output.totalCount = items.length;
  } else if (spec.exactCount !== undefined) {
    output.totalCount = await spec.exactCount(client);
  }
  const listAction = spec.actionName ?? "list";
  output.help = hasMore
    ? [
        `hevy-axi ${spec.helpName} ${listAction} --page ${currentPage + 1} --page-size ${pageSize}`,
      ]
    : [
        `hevy-axi ${spec.helpName} ${listAction} --all`,
        ...(spec.supportsView === false
          ? []
          : [
              `hevy-axi ${spec.helpName} view ${spec.kind === "measurement" ? "<date>" : "<id>"}`,
            ]),
      ];
  return output;
}

async function workoutCount(client: ClientLike): Promise<number> {
  const response = objectValue(
    await client.get<JsonValue>("/v1/workouts/count"),
    "workout count response",
  );
  if (
    !Number.isSafeInteger(response.workout_count) ||
    Number(response.workout_count) < 0
  ) {
    throw new HevyCliError(
      "PROTOCOL_ERROR",
      "Hevy returned an invalid workout count.",
    );
  }
  return Number(response.workout_count);
}

function unwrap(value: JsonValue, key: string): JsonValue {
  const object = objectValue(value, `${key} response`);
  const inner = object[key];
  if (inner === undefined) {
    throw new HevyCliError(
      "PROTOCOL_ERROR",
      `Hevy omitted ${key} from its response.`,
    );
  }
  return inner;
}

async function readStream(
  stream: NodeJS.ReadableStream,
  maximum: number,
): Promise<string> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of stream) {
    const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(String(chunk));
    size += buffer.byteLength;
    if (size > maximum) {
      throw validationError(`Input must not exceed ${maximum} bytes.`);
    }
    chunks.push(buffer);
  }
  return Buffer.concat(chunks).toString("utf8");
}

async function readFileHandleBounded(
  handle: FileHandle,
  maximum: number,
): Promise<string> {
  const chunks: Buffer[] = [];
  let offset = 0;
  while (true) {
    const chunk = Buffer.allocUnsafe(Math.min(64 * 1024, maximum + 1 - offset));
    const { bytesRead } = await handle.read(chunk, 0, chunk.length, offset);
    if (bytesRead === 0) break;
    offset += bytesRead;
    if (offset > maximum) {
      throw validationError(`Input must not exceed ${maximum} bytes.`);
    }
    chunks.push(chunk.subarray(0, bytesRead));
  }
  return Buffer.concat(chunks, offset).toString("utf8");
}

async function readMutationFile(
  path: string,
  deps: CommandDependencies,
): Promise<JsonObject> {
  let source: string;
  if (path === "-") {
    source = await readStream(deps.stdin ?? process.stdin, MAX_INPUT_BYTES);
  } else {
    const absolute = resolve(deps.cwd ?? process.cwd(), path);
    let metadata;
    try {
      metadata = await lstat(absolute);
    } catch {
      throw validationError("The mutation input file could not be inspected.");
    }
    if (metadata.isSymbolicLink() || !metadata.isFile()) {
      throw validationError(
        "The mutation input must be a regular, non-symbolic-link file.",
      );
    }
    if (metadata.size > MAX_INPUT_BYTES) {
      throw validationError(
        `The mutation input must not exceed ${MAX_INPUT_BYTES} bytes.`,
      );
    }
    const noFollow = fsConstants.O_NOFOLLOW ?? 0;
    let handle: FileHandle;
    try {
      handle = await open(absolute, fsConstants.O_RDONLY | noFollow);
    } catch {
      throw validationError(
        "The mutation input file could not be opened safely.",
      );
    }
    try {
      const current = await handle.stat();
      if (
        !current.isFile() ||
        current.dev !== metadata.dev ||
        current.ino !== metadata.ino
      ) {
        throw validationError(
          "The mutation input file changed while being opened.",
        );
      }
      source = await readFileHandleBounded(handle, MAX_INPUT_BYTES);
    } finally {
      await handle.close();
    }
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(source);
  } catch {
    throw validationError("The mutation input must contain valid JSON.");
  }
  if (parsed === null || Array.isArray(parsed) || typeof parsed !== "object") {
    throw validationError("The mutation input must be a JSON object.");
  }
  return parsed as JsonObject;
}

function envelope(input: JsonObject, key: string): JsonObject {
  if (Object.prototype.hasOwnProperty.call(input, key)) {
    if (Object.keys(input).length !== 1) {
      throw validationError(
        `The ${key} envelope must not contain sibling fields.`,
      );
    }
    objectValue(input[key] ?? null, `${key} request`);
    return input;
  }
  return { [key]: input };
}

function requireFields(
  object: JsonObject,
  fields: readonly string[],
  description: string,
): void {
  const missing = fields.filter(
    (field) => !Object.prototype.hasOwnProperty.call(object, field),
  );
  if (missing.length > 0) {
    throw validationError(
      `${description} is missing required field(s): ${missing.join(", ")}.`,
    );
  }
}

function requireNonemptyString(
  object: JsonObject,
  field: string,
  description: string,
): void {
  if (typeof object[field] !== "string" || object[field].trim() === "") {
    throw validationError(
      `${description} ${field} must be a non-empty string.`,
    );
  }
}

function requireString(
  object: JsonObject,
  field: string,
  description: string,
): void {
  if (typeof object[field] !== "string") {
    throw validationError(`${description} ${field} must be a string.`);
  }
}

function requireArray(
  object: JsonObject,
  field: string,
  description: string,
): void {
  if (!Array.isArray(object[field])) {
    throw validationError(`${description} ${field} must be an array.`);
  }
}

function validateMeasurementBody(input: JsonObject, update: boolean): void {
  const allowed = update
    ? MEASUREMENT_FIELD_SET
    : new Set<string>(["date", ...MEASUREMENT_FIELDS]);
  const unknown = Object.keys(input).filter((field) => !allowed.has(field));
  if (unknown.length > 0) {
    throw validationError(
      `Unknown measurement field${unknown.length === 1 ? "" : "s"}: ${unknown.join(", ")}.`,
      [
        `Allowed fields: ${[...(update ? [] : ["date"]), ...MEASUREMENT_FIELDS].join(", ")}`,
      ],
    );
  }
  if (update && Object.keys(input).length === 0) {
    throw validationError("Measurement update patch must not be empty.");
  }
  for (const field of MEASUREMENT_FIELDS) {
    if (!Object.prototype.hasOwnProperty.call(input, field)) continue;
    const value = input[field];
    if (
      value !== null &&
      (typeof value !== "number" || !Number.isFinite(value))
    ) {
      throw validationError(
        `Measurement ${field} must be a finite number or null.`,
      );
    }
  }
}

function validateKnownTopLevelTypes(
  resource: "workout" | "routine" | "exercise" | "folder",
  inner: JsonObject,
  update: boolean,
): void {
  if (resource === "workout") {
    requireFields(
      inner,
      [
        "title",
        "start_time",
        "end_time",
        ...(update ? ["description"] : []),
        "exercises",
      ],
      "Workout",
    );
    requireNonemptyString(inner, "title", "Workout");
    requireString(inner, "start_time", "Workout");
    requireString(inner, "end_time", "Workout");
    requireArray(inner, "exercises", "Workout");
    if (
      Object.prototype.hasOwnProperty.call(inner, "description") &&
      inner.description !== null &&
      typeof inner.description !== "string"
    ) {
      throw validationError("Workout description must be a string or null.");
    }
    if (
      Object.prototype.hasOwnProperty.call(inner, "is_private") &&
      typeof inner.is_private !== "boolean"
    ) {
      throw validationError("Workout is_private must be a boolean.");
    }
    return;
  }
  if (resource === "routine") {
    requireFields(
      inner,
      ["title", ...(update ? ["folder_id", "notes"] : []), "exercises"],
      "Routine",
    );
    requireNonemptyString(inner, "title", "Routine");
    requireArray(inner, "exercises", "Routine");
    if (
      Object.prototype.hasOwnProperty.call(inner, "folder_id") &&
      inner.folder_id !== null &&
      typeof inner.folder_id !== "number"
    ) {
      throw validationError("Routine folder_id must be a number or null.");
    }
    if (
      Object.prototype.hasOwnProperty.call(inner, "notes") &&
      inner.notes !== null &&
      typeof inner.notes !== "string"
    ) {
      throw validationError("Routine notes must be a string or null.");
    }
    return;
  }
  if (resource === "exercise") {
    requireFields(
      inner,
      [
        "title",
        "exercise_type",
        "equipment_category",
        "muscle_group",
        "other_muscles",
      ],
      "Exercise",
    );
    requireNonemptyString(inner, "title", "Exercise");
    for (const field of [
      "exercise_type",
      "equipment_category",
      "muscle_group",
    ]) {
      requireString(inner, field, "Exercise");
    }
    requireArray(inner, "other_muscles", "Exercise");
    if (
      (inner.other_muscles as JsonValue[]).some(
        (muscle) => typeof muscle !== "string",
      )
    ) {
      throw validationError("Exercise other_muscles entries must be strings.");
    }
    return;
  }
  requireFields(inner, ["title"], "Routine folder");
  requireNonemptyString(inner, "title", "Routine folder");
}

function prepareBody(
  resource: "workout" | "routine" | "exercise" | "folder" | "measurement",
  input: JsonObject,
  update: boolean,
): JsonObject {
  if (resource === "measurement") {
    validateMeasurementBody(input, update);
    if (!update) {
      requireFields(input, ["date"], "Measurement");
      if (typeof input.date !== "string") {
        throw validationError("Measurement date must be a YYYY-MM-DD string.");
      }
      validateDate(input.date);
    }
    return input;
  }
  const key = resource === "folder" ? "routine_folder" : resource;
  const body = envelope(input, key);
  const inner = objectValue(body[key] ?? null, `${key} request`);
  validateKnownTopLevelTypes(resource, inner, update);
  return body;
}

async function mutate(
  deps: CommandDependencies,
  parsed: ParsedArgs,
  resource: "workout" | "routine" | "exercise" | "folder" | "measurement",
  method: "POST" | "PUT",
  path: string,
): Promise<JsonValue | string> {
  const file = stringFlag(parsed, "file");
  if (file === undefined) {
    throw validationError(
      "--file <path|-> is required. Inline JSON is not accepted.",
    );
  }
  const dryRun = booleanFlag(parsed, "dry-run");
  if (!dryRun && !booleanFlag(parsed, "confirm")) {
    throw validationError(
      "This mutation requires --confirm, or use --dry-run.",
    );
  }
  if (dryRun && booleanFlag(parsed, "confirm")) {
    throw validationError("--dry-run cannot be combined with --confirm.");
  }
  const body = prepareBody(
    resource,
    await readMutationFile(file, deps),
    method === "PUT",
  );
  const options = commonOptions(parsed);
  const replacement = method === "PUT" && resource !== "measurement";
  const folderInsertion = resource === "folder" && method === "POST";
  if (dryRun) {
    const preview: JsonObject = {
      dryRun: true,
      method,
      path,
      idempotent: method === "PUT",
      ...(replacement ? { semantics: "full_replacement" } : {}),
      ...(folderInsertion
        ? { insertionIndex: 0, shiftsExistingFolders: true }
        : {}),
      bodySummary: {
        envelope:
          resource === "measurement"
            ? "none"
            : resource === "folder"
              ? "routine_folder"
              : resource,
        fields: Object.keys(
          resource === "measurement"
            ? body
            : objectValue(
                body[resource === "folder" ? "routine_folder" : resource] ??
                  null,
                "request",
              ),
        ),
      },
      ...(options.full ? { body } : {}),
      help: [
        `hevy-axi ${resource} ${method === "POST" ? "create" : "update <id>"} --file ${file} --confirm`,
      ],
    };
    return formatResult(
      finalizeOutput(preview, { full: options.full }),
      options.format,
    );
  }
  const client = await configuredClient(deps);
  const response =
    method === "POST"
      ? await client.post<JsonValue>(path, { body })
      : await client.put<JsonValue>(path, { body });
  const result: JsonObject = {
    status: "success",
    method,
    path,
    idempotent: method === "PUT",
    retried: false,
    ...(replacement ? { semantics: "full_replacement" } : {}),
    ...(folderInsertion
      ? { insertionIndex: 0, shiftsExistingFolders: true }
      : {}),
    result: response === undefined ? null : response,
  };
  return formatResult(
    finalizeOutput(result, {
      full: options.full,
      ...(options.fields === undefined ? {} : { fields: options.fields }),
    }),
    options.format,
  );
}

async function updateMeasurement(
  deps: CommandDependencies,
  parsed: ParsedArgs,
  date: string,
): Promise<JsonValue | string> {
  const file = stringFlag(parsed, "file");
  if (file === undefined) {
    throw validationError(
      "--file <path|-> is required. Inline JSON is not accepted.",
    );
  }
  const dryRun = booleanFlag(parsed, "dry-run");
  if (!dryRun && !booleanFlag(parsed, "confirm")) {
    throw validationError(
      "This mutation requires --confirm, or use --dry-run.",
    );
  }
  if (dryRun && booleanFlag(parsed, "confirm")) {
    throw validationError("--dry-run cannot be combined with --confirm.");
  }
  const patch = prepareBody(
    "measurement",
    await readMutationFile(file, deps),
    true,
  );
  const options = commonOptions(parsed);
  const path = `/v1/body_measurements/${date}`;
  const patchFields = Object.keys(patch);
  if (dryRun) {
    const preview: JsonObject = {
      dryRun: true,
      method: "PUT",
      path,
      idempotent: true,
      strategy: "merge_with_current",
      semantics: "partial_patch_merged_into_complete_replacement",
      patchFields,
      readBeforeWrite: false,
      ...(options.full ? { patch } : {}),
      help: [`hevy-axi measurement update ${date} --file ${file} --confirm`],
    };
    return formatResult(
      finalizeOutput(preview, { full: options.full }),
      options.format,
    );
  }

  const client = await configuredClient(deps);
  const current = objectValue(
    await client.get<JsonValue>(path),
    "body measurement",
  );
  const allowedUpstreamFields = new Set<string>([
    "date",
    ...MEASUREMENT_FIELDS,
  ]);
  const unknownUpstream = Object.keys(current).filter(
    (field) => !allowedUpstreamFields.has(field),
  );
  if (unknownUpstream.length > 0) {
    throw new HevyCliError(
      "PROTOCOL_ERROR",
      `Hevy returned unknown body measurement field${unknownUpstream.length === 1 ? "" : "s"}: ${unknownUpstream.join(", ")}. Refusing a replacement that could lose data.`,
    );
  }
  if (current.date !== date) {
    throw new HevyCliError(
      "PROTOCOL_ERROR",
      "Hevy returned a body measurement for an unexpected date.",
    );
  }
  const merged: JsonObject = {};
  for (const field of MEASUREMENT_FIELDS) {
    const currentValue = current[field];
    if (
      currentValue !== undefined &&
      currentValue !== null &&
      (typeof currentValue !== "number" || !Number.isFinite(currentValue))
    ) {
      throw new HevyCliError(
        "PROTOCOL_ERROR",
        `Hevy returned an invalid ${field} body measurement.`,
      );
    }
    merged[field] = Object.prototype.hasOwnProperty.call(patch, field)
      ? (patch[field] ?? null)
      : (currentValue ?? null);
  }
  const changedFields = patchFields.filter(
    (field) => !Object.is(current[field], patch[field]),
  );
  const response = await client.put<JsonValue>(path, { body: merged });
  const result: JsonObject = {
    status: "success",
    method: "PUT",
    path,
    idempotent: true,
    retried: false,
    strategy: "merge_with_current",
    semantics: "partial_patch_merged_into_complete_replacement",
    patchFields,
    changedFields,
    readBeforeWrite: true,
    result: response === undefined ? null : response,
  };
  return formatResult(
    finalizeOutput(result, {
      full: options.full,
      ...(options.fields === undefined ? {} : { fields: options.fields }),
    }),
    options.format,
  );
}

function readHandler(
  deps: CommandDependencies,
  command: string,
  action: string,
  args: string[],
  expectedPositionals: number,
  path: (positionals: readonly string[]) => string,
  kind: CompactKind | "user" | "count",
  unwrapKey?: string,
): Promise<JsonValue | string> {
  const parsed = parseArgs(args, READ_FLAGS);
  if (booleanFlag(parsed, "help")) return Promise.resolve(helpResult(command));
  requirePositionalCount(
    parsed,
    expectedPositionals,
    `hevy-axi ${command} ${action}${expectedPositionals > 0 ? " <id>" : ""} [flags]`,
  );
  return configuredClient(deps).then(async (client) => {
    const wire = await client.get<JsonValue>(path(parsed.positionals));
    const normalized = unwrapKey === undefined ? wire : unwrap(wire, unwrapKey);
    const options = commonOptions(parsed);
    let result: JsonValue = wire;
    if (!options.full) {
      if (kind === "count") {
        const compactCount: JsonObject = {
          workoutCount: await Promise.resolve(workoutCountValue(wire)),
        };
        result = {
          ...(selectCompactFields(compactCount, options.fields, [
            "workoutCount",
          ]) as JsonObject),
          help: ["hevy-axi workout list"],
        };
      } else if (kind === "user") {
        result = {
          account: selectCompactFields(normalized, options.fields, USER_FIELDS),
          help: ["hevy-axi workout list", "hevy-axi routine list"],
        };
      } else {
        const compactResult = compact(normalized, kind, true);
        result = {
          result: selectCompactFields(
            compactResult,
            options.fields,
            Object.keys(compactResult),
          ),
          help: detailHelp(command, compactResult),
        };
      }
    }
    return formatResult(
      finalizeOutput(result, { full: options.full }),
      options.format,
    );
  });
}

function selectCompactFields(
  value: JsonValue,
  fields: readonly string[] | undefined,
  availableFields: readonly string[],
): JsonValue {
  return fields === undefined
    ? value
    : projectFields(value, fields, availableFields);
}

function workoutCountValue(wire: JsonValue): number {
  const object = objectValue(wire, "workout count response");
  if (
    !Number.isSafeInteger(object.workout_count) ||
    Number(object.workout_count) < 0
  ) {
    throw new HevyCliError(
      "PROTOCOL_ERROR",
      "Hevy returned an invalid workout count.",
    );
  }
  return Number(object.workout_count);
}

function detailHelp(command: string, result: JsonValue): string[] {
  const object = objectValue(result, `${command} detail`);
  const id = command === "measurement" ? object.date : object.id;
  return [
    `hevy-axi ${command} list`,
    ...(typeof id === "string" || typeof id === "number"
      ? [`hevy-axi ${command} view ${String(id)} --full`]
      : []),
  ];
}

function listHandler(
  deps: CommandDependencies,
  command: string,
  args: string[],
  spec: ListSpec,
): Promise<JsonValue | string> {
  const parsed = parseArgs(args, LIST_FLAGS);
  if (booleanFlag(parsed, "help")) return Promise.resolve(helpResult(command));
  requirePositionalCount(parsed, 0, `hevy-axi ${command} list [flags]`);
  return configuredClient(deps).then(async (client) => {
    const options = commonOptions(parsed);
    return formatResult(
      finalizeOutput(await listResource(client, parsed, spec), {
        full: options.full,
      }),
      options.format,
    );
  });
}

function mutationArgs(
  args: string[],
  command: string,
  action: string,
  idRequired: boolean,
): ParsedArgs | string {
  const parsed = parseArgs(args, MUTATION_FLAGS);
  if (booleanFlag(parsed, "help")) return helpResult(command);
  requirePositionalCount(
    parsed,
    idRequired ? 1 : 0,
    `hevy-axi ${command} ${action}${idRequired ? " <id>" : ""} --file <path|-> (--confirm|--dry-run)`,
  );
  return parsed;
}

function credentialCategory(
  config: ResolvedConfig,
  deps: CommandDependencies,
): JsonValue {
  if (config.apiKey === undefined) return null;
  const source = config.credentialSource;
  if (source === "environment" || source === undefined)
    return source ?? "explicit";
  const cwd = resolve(deps.cwd ?? process.cwd(), ".env");
  const global = resolve(
    deps.homeDir ?? homedir(),
    ".config/hevy-axi/credentials.env",
  );
  if (resolve(source) === cwd) return "project";
  if (resolve(source) === global) return "global";
  return "explicit";
}

function safeHookStatus(deps: CommandDependencies): JsonObject {
  const hooks = deps.hooks ?? {
    install: installSessionStartHooks,
    status: sessionStartHookStatus,
    uninstall: uninstallSessionStartHooks,
  };
  const status = hooks.status({
    marker: HOOK_MARKER,
    ...(deps.execPath === undefined ? {} : { execPath: deps.execPath }),
  });
  return {
    scope: status.scope,
    claude: status.claude.installed,
    codex: status.codex.installed,
    opencode: status.opencode.installed,
  };
}

async function setupHandler(
  deps: CommandDependencies,
  args: string[],
): Promise<JsonValue | string> {
  const action = ensureAction(args, "setup");
  if (action === "--help") return helpResult("setup");
  const parsed = parseArgs(args.slice(1), SETUP_FLAGS);
  if (booleanFlag(parsed, "help")) return helpResult("setup");
  requirePositionalCount(parsed, 0, `hevy-axi setup ${action} [flags]`);
  const options = commonOptions(parsed);
  const requireConfirm = (): void => {
    if (!booleanFlag(parsed, "confirm"))
      throw validationError(`setup ${action} requires --confirm.`);
  };
  let result: JsonValue;
  switch (action) {
    case "status": {
      if (booleanFlag(parsed, "confirm"))
        throw validationError("setup status does not accept --confirm.");
      const config = await (deps.resolveConfig ?? resolveConfig)();
      result = {
        configured: config.apiKey !== undefined,
        credentialSource: credentialCategory(config, deps),
        baseUrl: config.baseUrl,
        hooks: safeHookStatus(deps),
        help:
          config.apiKey === undefined
            ? [
                "printf '%s\\n' \"$HEVY_API_KEY\" | hevy-axi setup key --confirm",
              ]
            : ["hevy-axi user info"],
      };
      break;
    }
    case "key": {
      requireConfirm();
      let key = (
        await readStream(deps.stdin ?? process.stdin, MAX_INPUT_BYTES)
      ).trim();
      key = key.replace(/^HEVY_API_KEY\s*=\s*/u, "").trim();
      if (
        (key.startsWith('"') && key.endsWith('"')) ||
        (key.startsWith("'") && key.endsWith("'"))
      )
        key = key.slice(1, -1);
      if (key === "")
        throw validationError("No API key was provided on stdin.");
      const stored = await (deps.writeStoredApiKey ?? writeStoredApiKey)(key);
      result = {
        status: stored.status,
        credentialSource: "global",
        help: ["hevy-axi setup status", "hevy-axi user info"],
      };
      break;
    }
    case "remove-key": {
      requireConfirm();
      const removed = await (deps.removeStoredApiKey ?? removeStoredApiKey)();
      result = { status: removed.status, credentialSource: "global" };
      break;
    }
    case "hooks": {
      requireConfirm();
      const hooks = deps.hooks ?? {
        install: installSessionStartHooks,
        status: sessionStartHookStatus,
        uninstall: uninstallSessionStartHooks,
      };
      hooks.install({
        marker: HOOK_MARKER,
        binaryNames: [...BINARY_NAMES],
        ...(deps.execPath === undefined ? {} : { execPath: deps.execPath }),
      });
      result = { status: "installed", hooks: safeHookStatus(deps) };
      break;
    }
    case "remove-hooks": {
      requireConfirm();
      const hooks = deps.hooks ?? {
        install: installSessionStartHooks,
        status: sessionStartHookStatus,
        uninstall: uninstallSessionStartHooks,
      };
      hooks.uninstall({
        marker: HOOK_MARKER,
        ...(deps.execPath === undefined ? {} : { execPath: deps.execPath }),
      });
      result = { status: "removed", hooks: safeHookStatus(deps) };
      break;
    }
    default:
      throw validationError(`Unknown setup action: ${action}.`);
  }
  return formatResult(
    finalizeOutput(result, {
      full: options.full,
      ...(options.fields === undefined ? {} : { fields: options.fields }),
    }),
    options.format,
  );
}

function workoutHandler(
  deps: CommandDependencies,
  args: string[],
): Promise<JsonValue | string> {
  const action = ensureAction(args, "workout");
  if (action === "--help") return Promise.resolve(helpResult("workout"));
  const rest = args.slice(1);
  if (action === "list")
    return listHandler(deps, "workout", rest, {
      path: "/v1/workouts",
      arrayKey: "workouts",
      maximumPageSize: 10,
      kind: "workout",
      helpName: "workout",
      exactCount: workoutCount,
    });
  if (action === "count")
    return readHandler(
      deps,
      "workout",
      "count",
      rest,
      0,
      () => "/v1/workouts/count",
      "count",
    );
  if (action === "view")
    return readHandler(
      deps,
      "workout",
      "view",
      rest,
      1,
      (p) => `/v1/workouts/${validateIdentifier(p[0] ?? "", "Workout ID")}`,
      "workout",
    );
  if (action === "events") {
    const parsed = parseArgs(
      rest,
      combineFlags(LIST_FLAGS, { since: { kind: "string" } }),
    );
    if (booleanFlag(parsed, "help"))
      return Promise.resolve(helpResult("workout"));
    requirePositionalCount(parsed, 0, "hevy-axi workout events [flags]");
    const since =
      validateIso(stringFlag(parsed, "since"), "--since") ??
      DEFAULT_EVENTS_SINCE;
    return configuredClient(deps).then(async (client) => {
      const options = commonOptions(parsed);
      return formatResult(
        finalizeOutput(
          await listResource(client, parsed, {
            path: "/v1/workouts/events",
            arrayKey: "events",
            maximumPageSize: 10,
            kind: "event",
            helpName: "workout",
            actionName: "events",
            supportsView: false,
            extraQuery: { since },
          }),
          { full: options.full },
        ),
        options.format,
      );
    });
  }
  if (action === "create" || action === "update") {
    const parsed = mutationArgs(rest, "workout", action, action === "update");
    if (typeof parsed === "string") return Promise.resolve(parsed);
    const path =
      action === "create"
        ? "/v1/workouts"
        : `/v1/workouts/${validateIdentifier(parsed.positionals[0] ?? "", "Workout ID")}`;
    return mutate(
      deps,
      parsed,
      "workout",
      action === "create" ? "POST" : "PUT",
      path,
    );
  }
  throw validationError(`Unknown workout action: ${action}.`);
}

function routineHandler(
  deps: CommandDependencies,
  args: string[],
): Promise<JsonValue | string> {
  const action = ensureAction(args, "routine");
  if (action === "--help") return Promise.resolve(helpResult("routine"));
  const rest = args.slice(1);
  if (action === "list")
    return listHandler(deps, "routine", rest, {
      path: "/v1/routines",
      arrayKey: "routines",
      maximumPageSize: 10,
      kind: "routine",
      helpName: "routine",
    });
  if (action === "view")
    return readHandler(
      deps,
      "routine",
      "view",
      rest,
      1,
      (p) => `/v1/routines/${validateIdentifier(p[0] ?? "", "Routine ID")}`,
      "routine",
      "routine",
    );
  if (action === "create" || action === "update") {
    const parsed = mutationArgs(rest, "routine", action, action === "update");
    if (typeof parsed === "string") return Promise.resolve(parsed);
    const path =
      action === "create"
        ? "/v1/routines"
        : `/v1/routines/${validateIdentifier(parsed.positionals[0] ?? "", "Routine ID")}`;
    return mutate(
      deps,
      parsed,
      "routine",
      action === "create" ? "POST" : "PUT",
      path,
    );
  }
  throw validationError(`Unknown routine action: ${action}.`);
}

function exerciseHandler(
  deps: CommandDependencies,
  args: string[],
): Promise<JsonValue | string> {
  const action = ensureAction(args, "exercise");
  if (action === "--help") return Promise.resolve(helpResult("exercise"));
  const rest = args.slice(1);
  if (action === "list")
    return listHandler(deps, "exercise", rest, {
      path: "/v1/exercise_templates",
      arrayKey: "exercise_templates",
      maximumPageSize: 100,
      defaultPageSize: 10,
      kind: "exercise",
      helpName: "exercise",
    });
  if (action === "view")
    return readHandler(
      deps,
      "exercise",
      "view",
      rest,
      1,
      (p) =>
        `/v1/exercise_templates/${validateIdentifier(p[0] ?? "", "Exercise template ID")}`,
      "exercise",
    );
  if (action === "history") {
    const parsed = parseArgs(
      rest,
      combineFlags(COMMON_OUTPUT_FLAGS, HELP_FLAG, {
        start: { kind: "string" },
        end: { kind: "string" },
      }),
    );
    if (booleanFlag(parsed, "help"))
      return Promise.resolve(helpResult("exercise"));
    requirePositionalCount(parsed, 1, "hevy-axi exercise history <id> [flags]");
    const start = validateIso(stringFlag(parsed, "start"), "--start");
    const end = validateIso(stringFlag(parsed, "end"), "--end");
    if (
      start !== undefined &&
      end !== undefined &&
      Date.parse(start) > Date.parse(end)
    )
      throw validationError("--start must not be later than --end.");
    return configuredClient(deps).then(async (client) => {
      const wire = objectValue(
        await client.get<JsonValue>(
          `/v1/exercise_history/${validateIdentifier(parsed.positionals[0] ?? "", "Exercise template ID")}`,
          { query: { start_date: start, end_date: end } },
        ),
        "exercise history response",
      );
      const entries = arrayValue(
        wire.exercise_history,
        "exercise history array",
      );
      const options = commonOptions(parsed);
      let result: JsonValue;
      if (options.full) {
        result = wire;
      } else {
        const compactEntries = entries.map(compactHistory);
        const selectedEntries = projectFields(
          compactEntries,
          options.fields ?? DEFAULT_COMPACT_FIELDS.history,
          AVAILABLE_COMPACT_FIELDS.history,
        ) as JsonValue[];
        const results = selectedEntries.slice(0, 50);
        const omittedCount = entries.length - results.length;
        result = {
          totalCount: entries.length,
          resultCount: results.length,
          omittedCount,
          truncated: omittedCount > 0,
          results,
          filters: { start: start ?? null, end: end ?? null },
          help: [
            `hevy-axi exercise view ${parsed.positionals[0] ?? "<id>"}`,
            ...(omittedCount > 0
              ? [
                  `hevy-axi exercise history ${parsed.positionals[0] ?? "<id>"} --full`,
                ]
              : []),
          ],
        };
      }
      return formatResult(
        finalizeOutput(result, { full: options.full }),
        options.format,
      );
    });
  }
  if (action === "create") {
    const parsed = mutationArgs(rest, "exercise", action, false);
    if (typeof parsed === "string") return Promise.resolve(parsed);
    return mutate(deps, parsed, "exercise", "POST", "/v1/exercise_templates");
  }
  throw validationError(`Unknown exercise action: ${action}.`);
}

function folderHandler(
  deps: CommandDependencies,
  args: string[],
): Promise<JsonValue | string> {
  const action = ensureAction(args, "folder");
  if (action === "--help") return Promise.resolve(helpResult("folder"));
  const rest = args.slice(1);
  if (action === "list")
    return listHandler(deps, "folder", rest, {
      path: "/v1/routine_folders",
      arrayKey: "routine_folders",
      maximumPageSize: 10,
      kind: "folder",
      helpName: "folder",
      legacyArrayKey: "routines",
    });
  if (action === "view")
    return readHandler(
      deps,
      "folder",
      "view",
      rest,
      1,
      (p) =>
        `/v1/routine_folders/${validateIdentifier(p[0] ?? "", "Folder ID")}`,
      "folder",
    );
  if (action === "create") {
    const parsed = mutationArgs(rest, "folder", action, false);
    if (typeof parsed === "string") return Promise.resolve(parsed);
    return mutate(deps, parsed, "folder", "POST", "/v1/routine_folders");
  }
  throw validationError(`Unknown folder action: ${action}.`);
}

function measurementHandler(
  deps: CommandDependencies,
  args: string[],
): Promise<JsonValue | string> {
  const action = ensureAction(args, "measurement");
  if (action === "--help") return Promise.resolve(helpResult("measurement"));
  const rest = args.slice(1);
  if (action === "list")
    return listHandler(deps, "measurement", rest, {
      path: "/v1/body_measurements",
      arrayKey: "body_measurements",
      maximumPageSize: 10,
      kind: "measurement",
      helpName: "measurement",
    });
  if (action === "view")
    return readHandler(
      deps,
      "measurement",
      "view",
      rest,
      1,
      (p) => `/v1/body_measurements/${validateDate(p[0] ?? "")}`,
      "measurement",
    );
  if (action === "create" || action === "update") {
    const parsed = mutationArgs(
      rest,
      "measurement",
      action,
      action === "update",
    );
    if (typeof parsed === "string") return Promise.resolve(parsed);
    const path =
      action === "create"
        ? "/v1/body_measurements"
        : `/v1/body_measurements/${validateDate(parsed.positionals[0] ?? "")}`;
    if (action === "update") {
      return updateMeasurement(
        deps,
        parsed,
        validateDate(parsed.positionals[0] ?? ""),
      );
    }
    return mutate(deps, parsed, "measurement", "POST", path);
  }
  throw validationError(`Unknown measurement action: ${action}.`);
}

function updateHandler(args: string[]): Promise<JsonValue | string> {
  const parsed = parseArgs(args, UPDATE_FLAGS);
  requirePositionalCount(parsed, 0, "hevy-axi update [--check] [--json]");
  if (booleanFlag(parsed, "help")) return Promise.resolve(helpResult("update"));
  const result: JsonObject = {
    status: "manual_update_required",
    currentVersion: VERSION,
    checkoutCommands: ["pnpm install --frozen-lockfile", "just check"],
  };
  return Promise.resolve(
    formatResult(result, booleanFlag(parsed, "json") ? "json" : "toon"),
  );
}

function userHandler(
  deps: CommandDependencies,
  args: string[],
): Promise<JsonValue | string> {
  const action = ensureAction(args, "user");
  if (action === "--help") return Promise.resolve(helpResult("user"));
  if (action !== "info")
    throw validationError(`Unknown user action: ${action}.`);
  return readHandler(
    deps,
    "user",
    "info",
    args.slice(1),
    0,
    () => "/v1/user/info",
    "user",
    "data",
  );
}

export function homeCommand(deps: CommandDependencies): CommandHandler {
  return async (args: string[]): Promise<Record<string, unknown> | string> => {
    const parsed = parseArgs(
      args,
      combineFlags(COMMON_OUTPUT_FLAGS, HELP_FLAG),
    );
    if (booleanFlag(parsed, "help")) return TOP_LEVEL_HELP;
    requirePositionalCount(parsed, 0, "hevy-axi [output flags]");
    const config = await (deps.resolveConfig ?? resolveConfig)();
    const options = commonOptions(parsed);
    let result: JsonValue;
    if (config.apiKey === undefined) {
      result = {
        status: "not_configured",
        configured: false,
        help: [
          "printf '%s\\n' \"$HEVY_API_KEY\" | hevy-axi setup key --confirm",
          "hevy-axi setup status",
        ],
      };
    } else {
      const client = deps.clientFactory(config);
      const [userWire, countWire, recentWire] = await Promise.all([
        client.get<JsonValue>("/v1/user/info"),
        client.get<JsonValue>("/v1/workouts/count"),
        client.get<JsonValue>("/v1/workouts", {
          query: { page: 1, pageSize: 3 },
        }),
      ]);
      const recent = objectValue(recentWire, "recent workouts response");
      const account = objectValue(unwrap(userWire, "data"), "user account");
      result = {
        status: "configured",
        account: options.full
          ? account
          : {
              username: scalar(account.username),
              weightUnit: scalar(account.weight_unit),
              distanceUnit: scalar(account.distance_unit),
            },
        workoutCount: workoutCountValue(countWire),
        recentWorkouts: arrayValue(recent.workouts, "recent workouts").map(
          (entry) => compactWorkout(entry),
        ),
        help: [
          "hevy-axi workout list",
          "hevy-axi routine list",
          "hevy-axi exercise list",
        ],
      };
    }
    return formatResult(
      finalizeOutput(result, {
        full: options.full,
        ...(options.fields === undefined ? {} : { fields: options.fields }),
      }),
      options.format,
    ) as Record<string, unknown> | string;
  };
}

export function createCommands(deps: CommandDependencies): CommandMap {
  return {
    update: (args) =>
      updateHandler(args) as Promise<Record<string, unknown> | string>,
    user: (args) =>
      userHandler(deps, args) as Promise<Record<string, unknown> | string>,
    workout: (args) =>
      workoutHandler(deps, args) as Promise<Record<string, unknown> | string>,
    routine: (args) =>
      routineHandler(deps, args) as Promise<Record<string, unknown> | string>,
    exercise: (args) =>
      exerciseHandler(deps, args) as Promise<Record<string, unknown> | string>,
    folder: (args) =>
      folderHandler(deps, args) as Promise<Record<string, unknown> | string>,
    measurement: (args) =>
      measurementHandler(deps, args) as Promise<
        Record<string, unknown> | string
      >,
    setup: (args) =>
      setupHandler(deps, args) as Promise<Record<string, unknown> | string>,
  };
}
