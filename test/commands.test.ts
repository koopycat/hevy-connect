import { mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Readable } from "node:stream";
import { fileURLToPath } from "node:url";

import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";

import {
  booleanFlag,
  combineFlags,
  outputFormat,
  parseArgs,
  positiveSafeInteger,
  requirePositionalCount,
  stringFlag,
  type FlagDefinition,
} from "../src/args.js";
import {
  createCommands,
  homeCommand,
  type CommandDependencies,
  type CommandHandler,
} from "../src/commands.js";
import type { ResolvedConfig } from "../src/config.js";
import { HevyCliError } from "../src/errors.js";
import { commandHelp, TOP_LEVEL_HELP } from "../src/help.js";
import {
  finalizeOutput,
  formatCliError,
  formatResult,
  parseFields,
  projectFields,
  renderOutput,
  shellArgument,
  truncateOutput,
} from "../src/output.js";
import type { JsonObject, JsonValue } from "../src/types.js";

interface ClientOptions {
  query?: Readonly<
    Record<string, string | number | boolean | null | undefined>
  >;
  body?: JsonValue;
}

interface ClientCall {
  method: "GET" | "POST" | "PUT";
  path: string;
  options?: ClientOptions;
}

type Responder = (call: ClientCall) => unknown;

const CONFIG: ResolvedConfig = {
  apiKey: "fake-api-key",
  baseUrl: "https://example.test",
  credentialSource: "environment",
};

const WORKOUT: JsonObject = {
  id: "w-1",
  title: "Leg day",
  start_time: "2024-01-01T10:00:00Z",
  end_time: "2024-01-01T11:15:00Z",
  description: "training",
  exercises: [
    {
      title: "Squat",
      exercise_template_id: "e-1",
      sets: [{ reps: 5 }, { reps: 5 }],
    },
  ],
};
const ROUTINE: JsonObject = {
  id: "r-1",
  title: "Strength",
  folder_id: 7,
  exercises: [
    { title: "Squat", exercise_template_id: "e-1", sets: [{ reps: 5 }] },
  ],
};
const EXERCISE: JsonObject = {
  id: "e-1",
  title: "Squat",
  type: "weight_reps",
  primary_muscle_group: "legs",
  equipment: "barbell",
  is_custom: false,
};
const FOLDER: JsonObject = { id: 7, index: 1, title: "Main" };
const MEASUREMENT: JsonObject = {
  date: "2024-08-14",
  weight_kg: 80,
  fat_percent: 15,
  waist: 82,
};
const HISTORY: JsonObject = {
  workout_id: "w-1",
  workout_title: "Leg day",
  workout_start_time: "2024-01-01T10:00:00Z",
  set_type: "normal",
  weight_kg: 100,
  reps: 5,
  distance_meters: null,
  duration_seconds: null,
  rpe: 8,
  custom_metric: null,
};

function page(key: string, entries: JsonValue[]): JsonObject {
  return { page: 1, page_count: 1, [key]: entries };
}

function defaultResponse(call: ClientCall): unknown {
  if (call.method !== "GET") return { accepted: true };
  switch (call.path) {
    case "/v1/user/info":
      return {
        data: {
          id: "u-1",
          username: "tester",
          name: "Test User",
          url: "https://hevy.com/user/tester",
          weight_unit: "kg",
          distance_unit: "km",
        },
      };
    case "/v1/workouts":
      return page("workouts", [WORKOUT]);
    case "/v1/workouts/count":
      return { workout_count: 12 };
    case "/v1/workouts/events":
      return page("events", [
        { type: "updated", workout: { ...WORKOUT, updated_at: "2024-01-02" } },
      ]);
    case "/v1/workouts/w-1":
      return WORKOUT;
    case "/v1/routines":
      return page("routines", [ROUTINE]);
    case "/v1/routines/r-1":
      return { routine: ROUTINE };
    case "/v1/exercise_templates":
      return page("exercise_templates", [EXERCISE]);
    case "/v1/exercise_templates/e-1":
      return EXERCISE;
    case "/v1/exercise_history/e-1":
      return { exercise_history: [HISTORY] };
    case "/v1/routine_folders":
      return page("routine_folders", [FOLDER]);
    case "/v1/routine_folders/7":
      return FOLDER;
    case "/v1/body_measurements":
      return page("body_measurements", [MEASUREMENT]);
    case "/v1/body_measurements/2024-08-14":
      return MEASUREMENT;
    default:
      throw new Error(`Unexpected fake request: ${call.method} ${call.path}`);
  }
}

class FakeClient {
  readonly calls: ClientCall[] = [];

  constructor(private readonly responder: Responder = defaultResponse) {}

  async get<T>(path: string, options?: ClientOptions): Promise<T> {
    const call: ClientCall = {
      method: "GET",
      path,
      ...(options ? { options } : {}),
    };
    this.calls.push(call);
    return this.responder(call) as T;
  }

  async post<T>(path: string, options?: ClientOptions): Promise<T> {
    const call: ClientCall = {
      method: "POST",
      path,
      ...(options ? { options } : {}),
    };
    this.calls.push(call);
    return this.responder(call) as T;
  }

  async put<T>(path: string, options?: ClientOptions): Promise<T> {
    const call: ClientCall = {
      method: "PUT",
      path,
      ...(options ? { options } : {}),
    };
    this.calls.push(call);
    return this.responder(call) as T;
  }
}

interface HarnessOptions {
  config?: ResolvedConfig;
  responder?: Responder;
  stdin?: NodeJS.ReadableStream;
}

type CommandName =
  | "user"
  | "workout"
  | "routine"
  | "exercise"
  | "folder"
  | "measurement"
  | "setup"
  | "update";

type TestCommands = Record<CommandName, CommandHandler>;

function harness(options: HarnessOptions = {}) {
  const client = new FakeClient(options.responder);
  const clientFactory = vi.fn((_config: ResolvedConfig) => client);
  const resolveConfig = vi.fn(async () => options.config ?? CONFIG);
  const writeKey = vi.fn(async (_key: string) => ({
    status: "created" as const,
    path: "/fake/credentials.env",
  }));
  const removeKey = vi.fn(async () => ({
    status: "removed" as const,
    path: "/fake/credentials.env",
  }));
  const installHook = vi.fn();
  const uninstallHook = vi.fn();
  const hookStatus = vi.fn((_options?: unknown) => ({
    marker: "hevy-axi",
    scope: "user" as const,
    claude: { installed: true, path: "/fake/claude" },
    codex: {
      installed: false,
      path: "/fake/codex",
      userFeatureEnabled: true,
      userFeaturePath: "/fake/codex-user",
    },
    opencode: { installed: true, path: "/fake/opencode" },
  }));
  const deps: CommandDependencies = {
    clientFactory,
    resolveConfig,
    writeStoredApiKey: writeKey,
    removeStoredApiKey: removeKey,
    ...(options.stdin === undefined ? {} : { stdin: options.stdin }),
    cwd: "/fake/project",
    homeDir: "/fake/home",
    execPath: "/fake/hevy-axi",
    hooks: {
      install(hookOptions) {
        installHook(hookOptions);
      },
      status(hookOptions) {
        hookStatus(hookOptions);
        return {
          marker: "hevy-axi",
          scope: "user",
          claude: { installed: true, path: "/fake/claude" },
          codex: {
            installed: false,
            path: "/fake/codex",
            userFeatureEnabled: true,
            userFeaturePath: "/fake/codex-user",
          },
          opencode: { installed: true, path: "/fake/opencode" },
        };
      },
      uninstall(hookOptions) {
        uninstallHook(hookOptions);
      },
    },
  };
  return {
    client,
    clientFactory,
    resolveConfig,
    writeKey,
    removeKey,
    installHook,
    uninstallHook,
    hookStatus,
    deps,
    commands: createCommands(deps) as TestCommands,
  };
}

function objectResult(
  value: Record<string, unknown> | string,
): Record<string, unknown> {
  expect(typeof value).toBe("object");
  return value as Record<string, unknown>;
}

async function expectValidation(promise: Promise<unknown>): Promise<void> {
  await expect(promise).rejects.toMatchObject({ code: "VALIDATION_ERROR" });
}

let temporaryDirectory = "";
const mutationFiles: Record<string, string> = {};

beforeAll(async () => {
  temporaryDirectory = await mkdtemp(join(tmpdir(), "hevy-axi-commands-"));
  const inputs: Readonly<Record<string, JsonValue>> = {
    workout: {
      title: "Workout",
      start_time: "2024-01-01T10:00:00Z",
      end_time: "2024-01-01T11:00:00Z",
      description: "Replacement workout",
      exercises: [],
    },
    routine: {
      title: "Routine",
      folder_id: null,
      notes: null,
      exercises: [],
    },
    exercise: {
      title: "Custom lift",
      exercise_type: "weight_reps",
      equipment_category: "barbell",
      muscle_group: "legs",
      other_muscles: [],
    },
    folder: { title: "Folder" },
    measurement: { date: "2024-08-14", weight_kg: 80 },
    measurementUpdate: { weight_kg: 81, fat_percent: null },
  };
  for (const [name, value] of Object.entries(inputs)) {
    const path = join(temporaryDirectory, `${name}.json`);
    await writeFile(path, JSON.stringify(value), "utf8");
    mutationFiles[name] = path;
  }
  const invalidPath = join(temporaryDirectory, "invalid.json");
  await writeFile(invalidPath, "not json", "utf8");
  mutationFiles.invalid = invalidPath;
  const oversizedPath = join(temporaryDirectory, "oversized.json");
  await writeFile(oversizedPath, "x".repeat(1024 * 1024 + 1), "utf8");
  mutationFiles.oversized = oversizedPath;
  const symlinkPath = join(temporaryDirectory, "symlink.json");
  await symlink(mutationFiles.folder ?? "", symlinkPath);
  mutationFiles.symlink = symlinkPath;
});

afterAll(async () => {
  await rm(temporaryDirectory, { recursive: true, force: true });
});

describe("argument parsing", () => {
  const definitions: Readonly<Record<string, FlagDefinition>> = {
    verbose: { kind: "boolean" },
    name: { kind: "string" },
    tag: { kind: "string", repeatable: true },
  };

  it("parses boolean, separate, inline, repeatable, and positional-only values", () => {
    const parsed = parseArgs(
      [
        "first",
        "--verbose",
        "--name=value",
        "--tag",
        "a",
        "--tag=b",
        "--",
        "--literal",
      ],
      definitions,
    );
    expect(parsed).toEqual({
      positionals: ["first", "--literal"],
      flags: { verbose: true, name: "value", tag: ["a", "b"] },
    });
    expect(booleanFlag(parsed, "verbose")).toBe(true);
    expect(stringFlag(parsed, "name")).toBe("value");
  });

  it("accepts a separate bare - as the stdin operand value", () => {
    expect(parseArgs(["--name", "-", "--verbose"], definitions)).toEqual({
      positionals: [],
      flags: { name: "-", verbose: true },
    });
    expect(() => parseArgs(["--name", "-x"], definitions)).toThrow(
      "requires a value",
    );
  });

  it.each([
    [["-v"], "Unknown flag"],
    [["--missing"], "Unknown flag"],
    [["--verbose=yes"], "does not take"],
    [["--name"], "requires a value"],
    [["--name="], "non-empty"],
    [["--name", "a", "--name", "b"], "only be specified once"],
  ] as const)("strictly rejects invalid flags %#", (args, message) => {
    expect(() => parseArgs(args, definitions)).toThrow(message);
  });

  it("combines definitions without changing the inputs", () => {
    const first = { one: { kind: "boolean" as const } };
    const second = { two: { kind: "string" as const } };
    expect(combineFlags(first, second)).toEqual({ ...first, ...second });
    expect(first).toEqual({ one: { kind: "boolean" } });
  });

  it("validates safe positive integers", () => {
    expect(positiveSafeInteger(undefined, "value")).toBeUndefined();
    expect(positiveSafeInteger("42", "value")).toBe(42);
    for (const value of ["0", "-1", "1.5", "x"]) {
      expect(() => positiveSafeInteger(value, "value")).toThrow(
        "positive integer",
      );
    }
    expect(() => positiveSafeInteger("99999999999999999999", "value")).toThrow(
      "safe integer",
    );
  });

  it("selects output formats and rejects contradictory/unknown formats", () => {
    const defs = {
      json: { kind: "boolean" as const },
      format: { kind: "string" as const },
    };
    expect(outputFormat(parseArgs([], defs))).toBe("toon");
    expect(outputFormat(parseArgs(["--json"], defs))).toBe("json");
    expect(outputFormat(parseArgs(["--format=json"], defs))).toBe("json");
    expect(() => outputFormat(parseArgs(["--format", "yaml"], defs))).toThrow();
    expect(() =>
      outputFormat(parseArgs(["--json", "--format", "toon"], defs)),
    ).toThrow();
  });

  it("enforces positional counts", () => {
    expect(() =>
      requirePositionalCount(parseArgs(["id"], {}), 1, "usage"),
    ).not.toThrow();
    expect(() => requirePositionalCount(parseArgs([], {}), 1, "usage")).toThrow(
      "Usage:",
    );
  });
});

describe("output helpers", () => {
  it("quotes only shell arguments that need it", () => {
    expect(shellArgument("/src/hevy-axi")).toBe("/src/hevy-axi");
    expect(shellArgument("2024-01-01T10:00:00+02:00")).toBe(
      "2024-01-01T10:00:00+02:00",
    );
    expect(shellArgument("/My Projects/hevy")).toBe("'/My Projects/hevy'");
    expect(shellArgument("it's")).toBe("'it'\\''s'");
  });

  it("renders JSON and TOON and returns JSON strings only for JSON format", () => {
    const value: JsonObject = { name: "sample", count: 2 };
    expect(JSON.parse(renderOutput(value, "json"))).toEqual(value);
    expect(renderOutput(value, "toon")).toContain("sample");
    expect(formatResult(value, "toon")).toBe(value);
    expect(JSON.parse(formatResult(value, "json") as string)).toEqual(value);
  });

  it("parses, trims, and deduplicates safe field paths", () => {
    expect(parseFields(undefined)).toBeUndefined();
    expect(parseFields(" user.name,items_1.id,user.name ")).toEqual([
      "user.name",
      "items_1.id",
    ]);
    for (const value of [
      "",
      "a,,b",
      "a.__proto__",
      "a.constructor",
      "a.$bad",
    ]) {
      expect(() => parseFields(value)).toThrow();
    }
  });

  it("projects nested objects and each member of arrays", () => {
    const value: JsonObject = {
      users: [
        { id: 1, profile: { name: "A", ignored: true } },
        { id: 2, profile: { name: "B", ignored: true } },
      ],
      ignored: true,
    };
    expect(projectFields(value, ["users.id", "users.profile.name"])).toEqual({
      users: [
        { id: 1, profile: { name: "A" } },
        { id: 2, profile: { name: "B" } },
      ],
    });
  });

  it("reports unknown fields with declared or discovered choices", () => {
    expect(() => projectFields({ known: 1 }, ["missing"])).toThrow(
      "Unknown field",
    );
    try {
      projectFields({}, ["missing"], []);
      expect.fail("projection should fail");
    } catch (error) {
      expect(error).toMatchObject({
        code: "VALIDATION_ERROR",
        suggestions: [expect.stringContaining("No fields")],
      });
    }
  });

  it("truncates long strings recursively and marks lossy output", () => {
    const long = "x".repeat(300);
    const output = truncateOutput({
      nested: [{ long }],
      short: "ok",
    }) as JsonObject;
    expect(output).toMatchObject({ truncated: true, short: "ok" });
    const nested = output.nested as JsonValue[];
    const first = nested[0] as JsonObject;
    expect(String(first.long)).toHaveLength(241);
    expect(String(first.long)).toMatch(/…$/u);
    expect(truncateOutput("short")).toBe("short");
    expect(truncateOutput(long)).toMatchObject({ truncated: true });
  });

  it("caps nested arrays with an exact aggregate omitted count and full hint", () => {
    const nested = Array.from({ length: 55 }, (_, index) => ({ index }));
    const output = truncateOutput({
      groups: [{ entries: nested }, { entries: nested }],
    }) as JsonObject;
    expect(output).toMatchObject({
      truncated: true,
      omittedItems: 10,
      truncationHelp: expect.stringContaining("--full"),
    });
    const groups = output.groups as JsonObject[];
    expect(groups[0]?.entries).toHaveLength(50);
    expect(groups[1]?.entries).toHaveLength(50);
  });

  it("does not break top-level list metadata while truncating nested arrays", () => {
    const output = truncateOutput({
      resultCount: 55,
      results: Array.from({ length: 55 }, (_, index) => ({
        id: index,
        nested: Array.from({ length: 52 }, (_, nestedIndex) => nestedIndex),
      })),
    }) as JsonObject;
    expect(output.results).toHaveLength(55);
    expect(output).toMatchObject({ resultCount: 55, omittedItems: 110 });
  });

  it("applies fields before truncation and lets --full bypass both", () => {
    const value: JsonObject = { keep: "y".repeat(300), drop: "secret" };
    expect(finalizeOutput(value, { fields: ["keep"] })).toMatchObject({
      truncated: true,
    });
    expect(finalizeOutput(value, { full: true, fields: ["keep"] })).toBe(value);
  });

  it("formats known and unexpected errors without leaking internals", () => {
    const known = formatCliError(
      new HevyCliError("PROTOCOL_ERROR", "bad response", {
        details: { request: "x" },
        suggestions: ["retry"],
      }),
      "json",
    );
    expect(known.exitCode).toBe(1);
    expect(JSON.parse(known.output)).toMatchObject({
      error: { code: "PROTOCOL_ERROR", message: "bad response" },
    });
    const validation = formatCliError(
      new HevyCliError("VALIDATION_ERROR", "bad"),
    );
    expect(validation.exitCode).toBe(2);
    const unexpected = JSON.parse(
      formatCliError(new Error("private"), "json").output,
    ) as JsonObject;
    expect(JSON.stringify(unexpected)).not.toContain("private");
  });
});

describe("help", () => {
  it("advertises every command at top level", () => {
    for (const command of [
      "user",
      "workout",
      "routine",
      "exercise",
      "folder",
      "measurement",
      "setup",
      "update",
    ]) {
      expect(TOP_LEVEL_HELP).toContain(command);
    }
  });

  it.each([
    ["user", ["info"]],
    ["workout", ["list", "count", "events", "view", "create", "update"]],
    ["routine", ["list", "view", "create", "update"]],
    ["exercise", ["list", "view", "history", "create"]],
    ["folder", ["list", "view", "create"]],
    ["measurement", ["list", "view", "create", "update"]],
    ["setup", ["status", "key", "remove-key", "hooks", "remove-hooks"]],
    ["update", ["--check", "--json", "--help"]],
  ] as const)("documents %s actions", (command, actions) => {
    const help = commandHelp(command);
    expect(help).toContain("Usage:");
    for (const action of actions) expect(help).toContain(action);
  });

  it("documents the read-only manual update command", async () => {
    const h = harness();
    const checkout = fileURLToPath(new URL("..", import.meta.url)).replace(
      /\/$/u,
      "",
    );
    const result = objectResult(await h.commands.update([]));
    expect(result).toEqual({
      status: "manual_update_required",
      currentVersion: "0.1.0",
      checkout,
      help: [
        `cd ${shellArgument(checkout)} && git pull --ff-only && just install && just build`,
        "hevy-axi --version",
      ],
    });
    for (const args of [
      ["--check", "--json"],
      ["--check", "--format", "json"],
    ]) {
      expect(JSON.parse((await h.commands.update(args)) as string)).toEqual(
        result,
      );
    }
    expect(
      objectResult(await h.commands.update(["--fields", "currentVersion"])),
    ).toEqual({ currentVersion: "0.1.0" });
    expect(commandHelp("update")).toContain("private");
    expect(commandHelp("update")).toContain("AXI compatibility");
    expect(commandHelp("update")).toContain("git pull --ff-only");
    expect(h.resolveConfig).not.toHaveBeenCalled();
    expect(h.clientFactory).not.toHaveBeenCalled();
  });

  it("strictly rejects unsupported update flags and positionals", async () => {
    const h = harness();
    await expectValidation(
      Promise.resolve().then(() => h.commands.update(["--force"])),
    );
    await expectValidation(
      Promise.resolve().then(() => h.commands.update(["latest"])),
    );
    await expectValidation(
      Promise.resolve().then(() => h.commands.update(["--help", "extra"])),
    );
  });

  it("explains replacement, folder insertion, history caps, and safe measurement merge", () => {
    expect(commandHelp("workout")).toContain("Fully replace");
    expect(commandHelp("routine")).toContain("Fully replace");
    expect(commandHelp("folder")).toContain("index 0");
    expect(commandHelp("folder")).toContain("shifting");
    expect(commandHelp("exercise")).toContain("capped at 50");
    expect(commandHelp("measurement")).toContain("merge_with_current");
  });

  it("returns undefined for an unknown command", () => {
    expect(commandHelp("unknown")).toBeUndefined();
  });
});

describe("all 22 Hevy operation routes and compact schemas", () => {
  it("maps user info to its GET path", async () => {
    const h = harness();
    const result = objectResult(await h.commands.user(["info"]));
    expect(h.client.calls).toEqual([{ method: "GET", path: "/v1/user/info" }]);
    expect(result).toMatchObject({
      account: { id: "u-1", username: "tester", weight_unit: "kg" },
    });
  });

  it("maps all four read-only workout operations", async () => {
    const h = harness();
    const listed = objectResult(await h.commands.workout(["list"]));
    const counted = objectResult(await h.commands.workout(["count"]));
    const events = objectResult(await h.commands.workout(["events"]));
    const viewed = objectResult(await h.commands.workout(["view", "w-1"]));

    expect(
      h.client.calls.map(({ method, path }) => ({ method, path })),
    ).toEqual([
      { method: "GET", path: "/v1/workouts" },
      { method: "GET", path: "/v1/workouts/count" },
      { method: "GET", path: "/v1/workouts/count" },
      { method: "GET", path: "/v1/workouts/events" },
      { method: "GET", path: "/v1/workouts/w-1" },
    ]);
    expect(listed.results).toEqual([
      {
        id: "w-1",
        title: "Leg day",
        startTime: "2024-01-01T10:00:00Z",
        exerciseCount: 1,
      },
    ]);
    expect(counted).toMatchObject({ workoutCount: 12 });
    expect(events.results).toEqual([
      { type: "updated", id: "w-1", time: "2024-01-02", title: "Leg day" },
    ]);
    expect(viewed).toMatchObject({
      result: { id: "w-1", setCount: 2, exerciseCount: 1 },
    });
  });

  it("lists each exercise's sets in the compact workout view", async () => {
    const h = harness({
      responder: () => ({
        ...WORKOUT,
        exercises: [
          {
            title: "Squat",
            exercise_template_id: "e-1",
            sets: [
              { index: 0, type: "warmup", weight_kg: 60, reps: 8 },
              {
                index: 1,
                type: "normal",
                weight_kg: 100,
                reps: 5,
                distance_meters: null,
                duration_seconds: null,
                rpe: 8,
                custom_metric: null,
              },
            ],
          },
        ],
      }),
    });
    const viewed = objectResult(await h.commands.workout(["view", "w-1"]));
    const set = {
      type: null,
      weightKg: null,
      reps: null,
      distanceMeters: null,
      durationSeconds: null,
      rpe: null,
      customMetric: null,
    };
    expect((viewed.result as JsonObject).exercises).toEqual([
      {
        title: "Squat",
        exerciseTemplateId: "e-1",
        setCount: 2,
        sets: [
          { ...set, type: "warmup", weightKg: 60, reps: 8 },
          { ...set, type: "normal", weightKg: 100, reps: 5, rpe: 8 },
        ],
      },
    ]);
  });

  it("rejects . and .. IDs that URL parsing would resolve to another endpoint", async () => {
    const h = harness();
    for (const id of [".", ".."]) {
      await expectValidation(
        Promise.resolve().then(() => h.commands.workout(["view", id])),
      );
      await expectValidation(
        Promise.resolve().then(() =>
          h.commands.routine([
            "update",
            id,
            "--file",
            mutationFiles.routine ?? "",
            "--dry-run",
          ]),
        ),
      );
    }
    expect(h.client.calls).toHaveLength(0);
  });

  it("accepts only ISO-8601 dates and offset timestamps as time filters", async () => {
    const h = harness();
    for (const value of [
      "1",
      "June 3",
      "2024-01-01 10:00",
      "2024-01-01T10:00:00",
      "2024-02-30",
      "2024-02-30T00:00:00Z",
    ]) {
      await expectValidation(
        Promise.resolve().then(() =>
          h.commands.workout(["events", "--since", value]),
        ),
      );
      await expectValidation(
        Promise.resolve().then(() =>
          h.commands.exercise(["history", "e-1", "--start", value]),
        ),
      );
    }
    expect(h.client.calls).toHaveLength(0);

    await h.commands.exercise([
      "history",
      "e-1",
      "--start",
      "2024-01-01",
      "--end",
      "2024-12-31T23:59:59.999+01:00",
    ]);
    expect(h.client.calls[0]?.options?.query).toEqual({
      start_date: "2024-01-01",
      end_date: "2024-12-31T23:59:59.999+01:00",
    });
  });

  it("uses the maximum page size for --all unless one is given", async () => {
    const pageSizes: unknown[] = [];
    for (const args of [
      ["list"],
      ["list", "--all"],
      ["list", "--all", "--page-size", "20"],
    ]) {
      const h = harness();
      await h.commands.exercise(args);
      pageSizes.push(h.client.calls[0]?.options?.query?.pageSize);
    }
    expect(pageSizes).toEqual([10, 100, 20]);
  });

  it("maps routine list/view and uses the compact routine schema", async () => {
    const h = harness();
    const listed = objectResult(await h.commands.routine(["list"]));
    const viewed = objectResult(await h.commands.routine(["view", "r-1"]));
    expect(h.client.calls.map((call) => call.path)).toEqual([
      "/v1/routines",
      "/v1/routines/r-1",
    ]);
    expect(listed.results).toEqual([
      { id: "r-1", title: "Strength", folderId: 7, exerciseCount: 1 },
    ]);
    expect(viewed).toMatchObject({ result: { id: "r-1", exerciseCount: 1 } });
  });

  it("maps exercise list/view/history and compacts both schemas", async () => {
    const h = harness();
    const listed = objectResult(await h.commands.exercise(["list"]));
    const viewed = objectResult(await h.commands.exercise(["view", "e-1"]));
    const history = objectResult(
      await h.commands.exercise([
        "history",
        "e-1",
        "--start",
        "2024-01-01",
        "--end",
        "2024-02-01",
      ]),
    );
    expect(h.client.calls.map((call) => call.path)).toEqual([
      "/v1/exercise_templates",
      "/v1/exercise_templates/e-1",
      "/v1/exercise_history/e-1",
    ]);
    expect(h.client.calls[2]?.options?.query).toEqual({
      start_date: "2024-01-01",
      end_date: "2024-02-01",
    });
    expect(listed.results).toEqual([
      {
        id: "e-1",
        title: "Squat",
        primaryMuscle: "legs",
        equipment: "barbell",
      },
    ]);
    expect(viewed).toMatchObject({
      result: { id: "e-1", primaryMuscle: "legs" },
    });
    expect(history).toMatchObject({
      resultCount: 1,
      results: [
        {
          workoutId: "w-1",
          workoutStartTime: "2024-01-01T10:00:00Z",
          weightKg: 100,
          reps: 5,
        },
      ],
    });
  });

  it("maps folder list/view and uses the compact folder schema", async () => {
    const h = harness();
    const listed = objectResult(await h.commands.folder(["list"]));
    const viewed = objectResult(await h.commands.folder(["view", "7"]));
    expect(h.client.calls.map((call) => call.path)).toEqual([
      "/v1/routine_folders",
      "/v1/routine_folders/7",
    ]);
    expect(listed.results).toEqual([{ id: 7, index: 1, title: "Main" }]);
    expect(viewed).toMatchObject({ result: { id: 7, title: "Main" } });
  });

  it("maps measurement list/view and validates/compacts dates", async () => {
    const h = harness();
    const listed = objectResult(await h.commands.measurement(["list"]));
    const viewed = objectResult(
      await h.commands.measurement(["view", "2024-08-14"]),
    );
    expect(h.client.calls.map((call) => call.path)).toEqual([
      "/v1/body_measurements",
      "/v1/body_measurements/2024-08-14",
    ]);
    expect(listed.results).toEqual([
      { date: "2024-08-14", weightKg: 80, fatPercent: 15, waist: 82 },
    ]);
    expect(viewed).toMatchObject({
      result: { date: "2024-08-14", weightKg: 80 },
    });
    await expectValidation(h.commands.measurement(["view", "2024-02-30"]));
  });
});

interface MutationCase {
  label: string;
  command: "workout" | "routine" | "exercise" | "folder" | "measurement";
  action: "create" | "update";
  id?: string;
  fileKey?: string;
  stdin?: string;
  method: "POST" | "PUT";
  path: string;
  body: JsonObject;
}

const mutationCases: readonly MutationCase[] = [
  {
    label: "workout create",
    command: "workout",
    action: "create",
    fileKey: "workout",
    method: "POST",
    path: "/v1/workouts",
    body: {
      workout: {
        title: "Workout",
        start_time: "2024-01-01T10:00:00Z",
        end_time: "2024-01-01T11:00:00Z",
        description: "Replacement workout",
        exercises: [],
      },
    },
  },
  {
    label: "workout update",
    command: "workout",
    action: "update",
    id: "w-1",
    fileKey: "workout",
    method: "PUT",
    path: "/v1/workouts/w-1",
    body: {
      workout: {
        title: "Workout",
        start_time: "2024-01-01T10:00:00Z",
        end_time: "2024-01-01T11:00:00Z",
        description: "Replacement workout",
        exercises: [],
      },
    },
  },
  {
    label: "routine create",
    command: "routine",
    action: "create",
    fileKey: "routine",
    method: "POST",
    path: "/v1/routines",
    body: {
      routine: {
        title: "Routine",
        folder_id: null,
        notes: null,
        exercises: [],
      },
    },
  },
  {
    label: "routine update",
    command: "routine",
    action: "update",
    id: "r-1",
    fileKey: "routine",
    method: "PUT",
    path: "/v1/routines/r-1",
    body: {
      routine: {
        title: "Routine",
        folder_id: null,
        notes: null,
        exercises: [],
      },
    },
  },
  {
    label: "exercise create",
    command: "exercise",
    action: "create",
    fileKey: "exercise",
    method: "POST",
    path: "/v1/exercise_templates",
    body: {
      exercise: {
        title: "Custom lift",
        exercise_type: "weight_reps",
        equipment_category: "barbell",
        muscle_group: "legs",
        other_muscles: [],
      },
    },
  },
  {
    label: "folder create",
    command: "folder",
    action: "create",
    fileKey: "folder",
    method: "POST",
    path: "/v1/routine_folders",
    body: { routine_folder: { title: "Folder" } },
  },
  {
    label: "measurement create",
    command: "measurement",
    action: "create",
    fileKey: "measurement",
    method: "POST",
    path: "/v1/body_measurements",
    body: { date: "2024-08-14", weight_kg: 80 },
  },
];

describe("mutation commands", () => {
  it.each(mutationCases)(
    "$label maps to one request, wraps its envelope, and reports idempotency/effects",
    async (testCase) => {
      const h = harness({
        ...(testCase.stdin === undefined
          ? {}
          : { stdin: Readable.from([testCase.stdin]) }),
      });
      const file =
        testCase.stdin === undefined
          ? mutationFiles[testCase.fileKey ?? ""]
          : "-";
      expect(file).toBeDefined();
      const args = [
        testCase.action,
        ...(testCase.id === undefined ? [] : [testCase.id]),
        "--file",
        file ?? "",
        "--confirm",
      ];
      const result = objectResult(await h.commands[testCase.command](args));
      expect(h.client.calls).toHaveLength(1);
      expect(h.client.calls[0]).toEqual({
        method: testCase.method,
        path: testCase.path,
        options: { body: testCase.body },
      });
      expect(result).toMatchObject({
        status: "success",
        method: testCase.method,
        path: testCase.path,
        idempotent: testCase.method === "PUT",
        retried: false,
        ...(testCase.method === "PUT" ? { semantics: "full_replacement" } : {}),
        ...(testCase.command === "folder"
          ? { insertionIndex: 0, shiftsExistingFolders: true }
          : {}),
        result: { accepted: true },
      });
    },
  );

  it("requires --confirm before reading/sending and rejects confirm with dry-run", async () => {
    const h = harness();
    await expectValidation(
      h.commands.folder(["create", "--file", mutationFiles.folder ?? ""]),
    );
    await expectValidation(
      h.commands.folder([
        "create",
        "--file",
        mutationFiles.folder ?? "",
        "--confirm",
        "--dry-run",
      ]),
    );
    expect(h.client.calls).toHaveLength(0);
    expect(h.clientFactory).not.toHaveBeenCalled();
  });

  it("dry-runs a regular JSON file without resolving config or calling the API", async () => {
    const h = harness();
    const result = objectResult(
      await h.commands.workout([
        "create",
        "--file",
        mutationFiles.workout ?? "",
        "--dry-run",
        "--full",
      ]),
    );
    expect(result).toMatchObject({
      dryRun: true,
      method: "POST",
      path: "/v1/workouts",
      idempotent: false,
      body: { workout: { title: "Workout" } },
    });
    expect(h.resolveConfig).not.toHaveBeenCalled();
    expect(h.client.calls).toHaveLength(0);
  });

  it("validates mutation JSON and required files", async () => {
    const h = harness();
    await expectValidation(h.commands.routine(["create", "--confirm"]));
    await expectValidation(
      h.commands.routine([
        "create",
        "--file",
        mutationFiles.invalid ?? "",
        "--confirm",
      ]),
    );
    expect(h.client.calls).toHaveLength(0);
  });

  it("rejects symlinked and oversized mutation files", async () => {
    for (const file of [mutationFiles.symlink, mutationFiles.oversized]) {
      const h = harness();
      await expectValidation(
        h.commands.folder(["create", "--file", file ?? "", "--dry-run"]),
      );
      expect(h.client.calls).toHaveLength(0);
    }
  });

  it("dry-runs measurement patches without making any API call", async () => {
    const h = harness({
      stdin: Readable.from([
        JSON.stringify({ weight_kg: 81, fat_percent: null }),
      ]),
    });
    const result = objectResult(
      await h.commands.measurement([
        "update",
        "2024-08-14",
        "--file",
        "-",
        "--dry-run",
        "--full",
      ]),
    );
    expect(result).toMatchObject({
      dryRun: true,
      strategy: "merge_with_current",
      patchFields: ["weight_kg", "fat_percent"],
      readBeforeWrite: false,
      patch: { weight_kg: 81, fat_percent: null },
    });
    expect(h.client.calls).toHaveLength(0);
    expect(h.clientFactory).not.toHaveBeenCalled();
  });

  it("merges a measurement patch after one GET and sends one complete PUT", async () => {
    const h = harness({
      stdin: Readable.from([
        JSON.stringify({ weight_kg: 81, fat_percent: null }),
      ]),
    });
    const result = objectResult(
      await h.commands.measurement([
        "update",
        "2024-08-14",
        "--file",
        "-",
        "--confirm",
      ]),
    );
    expect(h.client.calls).toHaveLength(2);
    expect(h.client.calls[0]).toEqual({
      method: "GET",
      path: "/v1/body_measurements/2024-08-14",
    });
    const replacement: Record<string, JsonValue> = Object.fromEntries(
      [
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
      ].map((field) => [field, null]),
    );
    replacement.weight_kg = 81;
    replacement.waist = 82;
    expect(h.client.calls[1]).toEqual({
      method: "PUT",
      path: "/v1/body_measurements/2024-08-14",
      options: { body: replacement },
    });
    expect(result).toMatchObject({
      status: "success",
      strategy: "merge_with_current",
      changedFields: ["weight_kg", "fat_percent"],
      readBeforeWrite: true,
      retried: false,
    });
  });

  it("rejects empty, unknown, date, and non-number measurement patches", async () => {
    for (const patch of [
      {},
      { unknown_metric: 1 },
      { date: "2024-08-14" },
      { weight_kg: "81" },
    ]) {
      const h = harness({ stdin: Readable.from([JSON.stringify(patch)]) });
      await expectValidation(
        h.commands.measurement([
          "update",
          "2024-08-14",
          "--file=-",
          "--dry-run",
        ]),
      );
      expect(h.client.calls).toHaveLength(0);
    }
  });

  it("refuses measurement replacement when upstream adds an unknown field", async () => {
    const h = harness({
      stdin: Readable.from([JSON.stringify({ weight_kg: 81 })]),
      responder(call) {
        if (call.method === "GET") return { ...MEASUREMENT, new_metric: 1 };
        return { accepted: true };
      },
    });
    await expect(
      h.commands.measurement(["update", "2024-08-14", "--file=-", "--confirm"]),
    ).rejects.toMatchObject({ code: "PROTOCOL_ERROR" });
    expect(h.client.calls).toHaveLength(1);
  });

  it("requires complete replacement fields and rejects dangerous top-level types", async () => {
    const cases = [
      ["workout", { ...WORKOUT, description: undefined }],
      ["routine", { title: "R", folder_id: null, exercises: [] }],
      ["routine", { title: "R", folder_id: {}, notes: null, exercises: [] }],
    ] as const;
    for (const [command, body] of cases) {
      const h = harness({
        stdin: Readable.from([JSON.stringify(body)]),
      });
      await expectValidation(
        h.commands[command]([
          "update",
          command === "workout" ? "w-1" : "r-1",
          "--file=-",
          "--dry-run",
        ]),
      );
    }
  });
});

describe("pagination, events, and protocol validation", () => {
  it("uses resource defaults and accepts each maximum page size", async () => {
    const workouts = harness();
    await workouts.commands.workout(["list"]);
    expect(workouts.client.calls[0]?.options?.query).toEqual({
      page: 1,
      pageSize: 10,
    });

    const exercisesDefault = harness();
    await exercisesDefault.commands.exercise(["list"]);
    expect(exercisesDefault.client.calls[0]?.options?.query).toEqual({
      page: 1,
      pageSize: 10,
    });

    const exercises = harness();
    await exercises.commands.exercise(["list", "--page-size", "100"]);
    expect(exercises.client.calls[0]?.options?.query).toEqual({
      page: 1,
      pageSize: 100,
    });

    const rejected = harness();
    await expectValidation(
      rejected.commands.exercise(["list", "--page-size", "101"]),
    );
    expect(rejected.client.calls).toHaveLength(0);
  });

  it("fetches all pages and emits an exact total", async () => {
    const h = harness({
      responder(call) {
        const requested = Number(call.options?.query?.page);
        return {
          page: requested,
          page_count: 2,
          routines: [{ ...ROUTINE, id: `r-${requested}` }],
        };
      },
    });
    const result = objectResult(await h.commands.routine(["list", "--all"]));
    expect(h.client.calls).toHaveLength(2);
    expect(h.client.calls.map((call) => call.options?.query?.page)).toEqual([
      1, 2,
    ]);
    expect(result).toMatchObject({
      page: 1,
      pageCount: 2,
      resultCount: 2,
      totalCount: 2,
      hasMore: false,
    });
  });

  it("rejects mismatched, repeated, clamped, and impossible page metadata", async () => {
    for (const payload of [
      { page: 1, page_count: 2, routines: [ROUTINE] },
      { page: 3, page_count: 3, routines: [ROUTINE] },
      { page: 2, page_count: 1, routines: [ROUTINE] },
      { page: 2, page_count: 0, routines: [] },
    ]) {
      const h = harness({ responder: () => payload });
      await expect(
        h.commands.routine(["list", "--page", "2"]),
      ).rejects.toMatchObject({
        code: "PROTOCOL_ERROR",
      });
      expect(h.client.calls).toHaveLength(1);
    }

    const repeated = harness({
      responder(call) {
        const requested = Number(call.options?.query?.page);
        return {
          page: requested === 1 ? 1 : 1,
          page_count: 2,
          routines: [{ ...ROUTINE, id: `r-${requested}` }],
        };
      },
    });
    await expect(
      repeated.commands.routine(["list", "--all"]),
    ).rejects.toMatchObject({ code: "PROTOCOL_ERROR" });
    expect(
      repeated.client.calls.map((call) => call.options?.query?.page),
    ).toEqual([1, 2]);
  });

  it("honors a limit and stops before another page", async () => {
    const h = harness({
      responder() {
        return { page: 1, page_count: 3, routines: [ROUTINE, ROUTINE] };
      },
    });
    const result = objectResult(
      await h.commands.routine(["list", "--all", "--limit", "1"]),
    );
    expect(h.client.calls).toHaveLength(1);
    expect(result).toMatchObject({ resultCount: 1, hasMore: true });
    expect(result).not.toHaveProperty("totalCount");
  });

  describe("limits that end inside a page", () => {
    function pagedRoutines(total: number): Responder {
      const routines = Array.from({ length: total }, (_, index) => ({
        ...ROUTINE,
        id: `r-${index + 1}`,
      }));
      return (call) => {
        const requested = Number(call.options?.query?.page);
        const size = Number(call.options?.query?.pageSize);
        return {
          page: requested,
          page_count: Math.ceil(total / size),
          routines: routines.slice((requested - 1) * size, requested * size),
        };
      };
    }

    it("reports omitted items on a single page and resumes at that page", async () => {
      const h = harness({ responder: pagedRoutines(7) });
      const result = objectResult(
        await h.commands.routine(["list", "--limit", "3"]),
      );
      expect(result).toMatchObject({
        pageCount: 1,
        resultCount: 3,
        hasMore: true,
        resume: { page: 1, skip: 3 },
        help: ["hevy-axi routine list --page 1 --page-size 10"],
      });
      expect(result).not.toHaveProperty("totalCount");
    });

    it("reports the true total, not the limit, once every page was read", async () => {
      const h = harness({ responder: pagedRoutines(7) });
      const result = objectResult(
        await h.commands.routine(["list", "--all", "--limit", "3"]),
      );
      expect(result).toMatchObject({
        resultCount: 3,
        totalCount: 7,
        hasMore: true,
        resume: { page: 1, skip: 3 },
      });
    });

    it("resumes inside the page where an --all limit stopped", async () => {
      const h = harness({ responder: pagedRoutines(25) });
      const result = objectResult(
        await h.commands.routine(["list", "--all", "--limit", "15"]),
      );
      expect(h.client.calls.map((call) => call.options?.query?.page)).toEqual([
        1, 2,
      ]);
      expect(result).toMatchObject({
        resultCount: 15,
        hasMore: true,
        resume: { page: 2, skip: 5 },
        help: ["hevy-axi routine list --page 2 --page-size 10"],
      });
      expect(result).not.toHaveProperty("totalCount");
    });

    it("continues with the next page when a limit ends on a page boundary", async () => {
      const h = harness({ responder: pagedRoutines(25) });
      const result = objectResult(
        await h.commands.routine(["list", "--all", "--limit", "10"]),
      );
      expect(result).toMatchObject({
        resultCount: 10,
        hasMore: true,
        help: ["hevy-axi routine list --page 2 --page-size 10"],
      });
      expect(result).not.toHaveProperty("resume");
    });

    it("includes resume metadata in --full output", async () => {
      const h = harness({ responder: pagedRoutines(7) });
      const result = objectResult(
        await h.commands.routine(["list", "--limit", "2", "--full"]),
      );
      expect(result).toMatchObject({
        resultCount: 2,
        hasMore: true,
        resume: { page: 1, skip: 2 },
      });
    });
  });

  it("keeps a caller-supplied --since in event continuation commands", async () => {
    const h = harness({
      responder(call) {
        return {
          page: Number(call.options?.query?.page),
          page_count: 2,
          events: [{ type: "deleted", id: "w-1", deleted_at: "2024-01-02" }],
        };
      },
    });
    const since = "2024-01-01T00:00:00Z";
    const paged = objectResult(
      await h.commands.workout(["events", "--since", since]),
    );
    expect(paged.help).toEqual([
      `hevy-axi workout events --since ${since} --page 2 --page-size 10`,
    ]);

    const offset = objectResult(
      await h.commands.workout([
        "events",
        "--since",
        "2024-01-01T10:00:00+02:00",
        "--all",
      ]),
    );
    expect(offset.help).toEqual([
      "hevy-axi workout events --since 2024-01-01T10:00:00+02:00 --all",
    ]);

    const unfiltered = objectResult(await h.commands.workout(["events"]));
    expect(unfiltered.help).toEqual([
      "hevy-axi workout events --page 2 --page-size 10",
    ]);
  });

  it.each(["routine_folders", "routines"])(
    "accepts the %s folder-list envelope",
    async (arrayKey) => {
      const h = harness({
        responder() {
          return page(arrayKey, [FOLDER]);
        },
      });
      const result = objectResult(await h.commands.folder(["list"]));
      expect(result.results).toEqual([{ id: 7, index: 1, title: "Main" }]);
      expect(result.empty).toBe(false);
    },
  );

  it("represents an empty list without inventing entries", async () => {
    const h = harness({
      responder() {
        return { page: 1, page_count: 0, routine_folders: [] };
      },
    });
    const result = objectResult(await h.commands.folder(["list"]));
    expect(result).toMatchObject({
      page: 1,
      pageCount: 0,
      resultCount: 0,
      empty: true,
      hasMore: false,
      results: [],
    });
  });

  it("enforces page and item safety caps", async () => {
    const pages = harness({
      responder(call) {
        return {
          page: Number(call.options?.query?.page),
          page_count: 501,
          routines: [],
        };
      },
    });
    await expectValidation(pages.commands.routine(["list", "--all"]));
    expect(pages.client.calls).toHaveLength(500);

    const tooMany = Array.from({ length: 5001 }, (_, index) => ({
      ...FOLDER,
      id: index,
    }));
    const items = harness({
      responder() {
        return { page: 1, page_count: 1, routine_folders: tooMany };
      },
    });
    await expectValidation(items.commands.folder(["list", "--all"]));
    expect(items.client.calls).toHaveLength(1);
  });

  it("validates pagination combinations and numeric caps before requests", async () => {
    const h = harness();
    await expectValidation(
      h.commands.routine(["list", "--all", "--page", "2"]),
    );
    await expectValidation(h.commands.routine(["list", "--limit", "5001"]));
    await expectValidation(h.commands.routine(["list", "--page", "0"]));
    expect(h.client.calls).toHaveLength(0);
  });

  it.each([
    { page: "one", page_count: 1, routines: [] },
    { page: 1, page_count: -1, routines: [] },
    { page: 1, page_count: 1, routines: {} },
    { page: 1, page_count: 0, routines: [ROUTINE] },
  ] as const)("rejects malformed list protocol payload %#", async (payload) => {
    const h = harness({ responder: () => payload });
    await expect(h.commands.routine(["list"])).rejects.toMatchObject({
      code: "PROTOCOL_ERROR",
    });
  });

  it("rejects missing envelopes and invalid count protocol payloads", async () => {
    const missing = harness({ responder: () => ({}) });
    await expect(missing.commands.user(["info"])).rejects.toMatchObject({
      code: "PROTOCOL_ERROR",
    });
    const count = harness({ responder: () => ({ workout_count: -1 }) });
    await expect(count.commands.workout(["count"])).rejects.toMatchObject({
      code: "PROTOCOL_ERROR",
    });
  });

  it("uses the default event epoch and supports the legacy workouts envelope", async () => {
    const h = harness({
      responder() {
        return {
          page: 1,
          page_count: 1,
          workouts: [{ id: "legacy", title: "Old", updated_at: "2024-03-01" }],
        };
      },
    });
    const result = objectResult(await h.commands.workout(["events"]));
    expect(h.client.calls[0]?.options?.query).toEqual({
      page: 1,
      pageSize: 10,
      since: "1970-01-01T00:00:00Z",
    });
    expect(result.results).toEqual([
      { type: "updated", id: "legacy", time: "2024-03-01", title: "Old" },
    ]);
  });
});

describe("command output, validation, and help behavior", () => {
  it("projects exercise history records and reports available fields", async () => {
    const h = harness();
    const result = objectResult(
      await h.commands.exercise([
        "history",
        "e-1",
        "--fields",
        "workoutId,reps,weightKg",
      ]),
    );
    expect(result.results).toEqual([
      { workoutId: "w-1", reps: 5, weightKg: 100 },
    ]);
    await expect(
      h.commands.exercise(["history", "e-1", "--fields", "unknown"]),
    ).rejects.toMatchObject({
      code: "VALIDATION_ERROR",
      suggestions: [expect.stringContaining("workoutId")],
    });
  });

  it("caps default history output at 50 and leaves --full as an escape hatch", async () => {
    const entries = Array.from({ length: 55 }, (_, index) => ({
      ...HISTORY,
      workout_id: `w-${index}`,
    }));
    const compactHarness = harness({
      responder: () => ({ exercise_history: entries }),
    });
    const compact = objectResult(
      await compactHarness.commands.exercise(["history", "e-1"]),
    );
    expect(compact).toMatchObject({
      totalCount: 55,
      resultCount: 50,
      omittedCount: 5,
      truncated: true,
    });
    expect(compact.results).toHaveLength(50);
    expect(compact.help).toContain("hevy-axi exercise history e-1 --full");

    const fullHarness = harness({
      responder: () => ({ exercise_history: entries }),
    });
    const full = objectResult(
      await fullHarness.commands.exercise(["history", "e-1", "--full"]),
    );
    expect(full.exercise_history).toHaveLength(55);
  });

  it("uses minimal exact default keys while keeping every compact field selectable", async () => {
    const h = harness();
    const workout = objectResult(await h.commands.workout(["list"]));
    const routine = objectResult(await h.commands.routine(["list"]));
    const exercise = objectResult(await h.commands.exercise(["list"]));
    const folder = objectResult(await h.commands.folder(["list"]));
    const measurement = objectResult(await h.commands.measurement(["list"]));
    const event = objectResult(await h.commands.workout(["events"]));
    const history = objectResult(await h.commands.exercise(["history", "e-1"]));

    expect(Object.keys((workout.results as JsonObject[])[0] ?? {})).toEqual([
      "id",
      "title",
      "startTime",
      "exerciseCount",
    ]);
    expect(Object.keys((routine.results as JsonObject[])[0] ?? {})).toEqual([
      "id",
      "title",
      "folderId",
      "exerciseCount",
    ]);
    expect(Object.keys((exercise.results as JsonObject[])[0] ?? {})).toEqual([
      "id",
      "title",
      "primaryMuscle",
      "equipment",
    ]);
    expect(Object.keys((folder.results as JsonObject[])[0] ?? {})).toEqual([
      "id",
      "index",
      "title",
    ]);
    expect(Object.keys((measurement.results as JsonObject[])[0] ?? {})).toEqual(
      ["date", "weightKg", "fatPercent", "waist"],
    );
    expect(Object.keys((event.results as JsonObject[])[0] ?? {})).toEqual([
      "type",
      "id",
      "time",
      "title",
    ]);
    expect(Object.keys((history.results as JsonObject[])[0] ?? {})).toEqual([
      "workoutId",
      "workoutStartTime",
      "weightKg",
      "reps",
    ]);

    const projectedWorkout = objectResult(
      await h.commands.workout(["list", "--fields", "durationMinutes"]),
    );
    expect(projectedWorkout.results).toEqual([{ durationMinutes: 75 }]);
    const projected = objectResult(
      await h.commands.exercise(["list", "--fields", "type,isCustom"]),
    );
    expect(projected.results).toEqual([
      { type: "weight_reps", isCustom: false },
    ]);
    const projectedHistory = objectResult(
      await h.commands.exercise([
        "history",
        "e-1",
        "--fields",
        "workoutTitle,setType,distanceMeters,durationSeconds,rpe,customMetric",
      ]),
    );
    expect(
      Object.keys((projectedHistory.results as JsonObject[])[0] ?? {}),
    ).toEqual([
      "workoutTitle",
      "setType",
      "distanceMeters",
      "durationSeconds",
      "rpe",
      "customMetric",
    ]);
    await expectValidation(
      h.commands.exercise(["list", "--fields", "unknown"]),
    );
  });

  it("rejects --full with --fields", async () => {
    const h = harness();
    await expectValidation(
      h.commands.user(["info", "--full", "--fields", "username"]),
    );
  });

  it("returns full wire data when requested", async () => {
    const h = harness();
    const result = objectResult(await h.commands.workout(["list", "--full"]));
    expect(result).toMatchObject({ workouts: [WORKOUT], resultCount: 1 });
    expect(result).not.toHaveProperty("results");
  });

  it("supports JSON and default TOON-compatible object output", async () => {
    const toon = harness();
    expect(typeof (await toon.commands.user(["info"]))).toBe("object");
    const json = harness();
    const output = await json.commands.user(["info", "--json"]);
    expect(typeof output).toBe("string");
    expect(JSON.parse(output as string)).toMatchObject({
      account: { username: "tester" },
    });
  });

  it("strictly rejects unknown flags, extra positionals, actions, and bad ranges", async () => {
    const h = harness();
    await expectValidation(
      Promise.resolve().then(() => h.commands.user(["info", "--typo"])),
    );
    await expectValidation(
      Promise.resolve().then(() => h.commands.folder(["view", "7", "extra"])),
    );
    await expectValidation(
      Promise.resolve().then(() => h.commands.routine(["missing"])),
    );
    await expectValidation(
      Promise.resolve().then(() =>
        h.commands.exercise([
          "history",
          "e-1",
          "--start",
          "2024-02-01",
          "--end",
          "2024-01-01",
        ]),
      ),
    );
  });

  it("returns command help without resolving config or creating a client", async () => {
    for (const command of [
      "user",
      "workout",
      "routine",
      "exercise",
      "folder",
      "measurement",
      "setup",
      "update",
    ] as const) {
      const h = harness();
      const result = await h.commands[command](["--help"]);
      expect(result).toContain("Usage:");
      expect(h.resolveConfig).not.toHaveBeenCalled();
      expect(h.clientFactory).not.toHaveBeenCalled();
    }
  });
});

describe("setup commands with fake config, streams, and hooks", () => {
  it("reports safe status metadata without exposing the API key", async () => {
    const h = harness({
      config: {
        apiKey: "never-print-this-secret",
        baseUrl: "https://example.test",
        credentialSource: "/fake/home/.config/hevy-axi/credentials.env",
      },
    });
    const result = objectResult(await h.commands.setup(["status"]));
    expect(result).toMatchObject({
      configured: true,
      credentialSource: "global",
      baseUrl: "https://example.test",
      hooks: { scope: "user", claude: true, codex: false, opencode: true },
    });
    expect(JSON.stringify(result)).not.toContain("never-print-this-secret");
    expect(h.hookStatus).toHaveBeenCalledOnce();
  });

  it("reads a key only from fake stdin, stores it, and never returns it", async () => {
    const secret = "stdin-only-secret";
    const h = harness({ stdin: Readable.from([`HEVY_API_KEY='${secret}'\n`]) });
    const result = objectResult(await h.commands.setup(["key", "--confirm"]));
    expect(h.writeKey).toHaveBeenCalledWith(secret);
    expect(result).toMatchObject({
      status: "created",
      credentialSource: "global",
    });
    expect(JSON.stringify(result)).not.toContain(secret);
    expect(h.resolveConfig).not.toHaveBeenCalled();
  });

  it("removes a stored key through the injected dependency", async () => {
    const h = harness();
    const result = objectResult(
      await h.commands.setup(["remove-key", "--confirm"]),
    );
    expect(h.removeKey).toHaveBeenCalledOnce();
    expect(result).toMatchObject({
      status: "removed",
      credentialSource: "global",
    });
  });

  it("installs and removes hooks through injected hooks", async () => {
    const install = harness();
    const installed = objectResult(
      await install.commands.setup(["hooks", "--confirm"]),
    );
    expect(install.installHook).toHaveBeenCalledWith(
      expect.objectContaining({
        marker: "hevy-axi",
        binaryNames: ["hevy-axi"],
      }),
    );
    expect(installed).toMatchObject({ status: "installed" });

    const remove = harness();
    const removed = objectResult(
      await remove.commands.setup(["remove-hooks", "--confirm"]),
    );
    expect(remove.uninstallHook).toHaveBeenCalledWith(
      expect.objectContaining({ marker: "hevy-axi" }),
    );
    expect(removed).toMatchObject({ status: "removed" });
  });

  it("gates every setup mutation with --confirm and forbids it on status", async () => {
    for (const action of ["key", "remove-key", "hooks", "remove-hooks"]) {
      const h = harness({ stdin: Readable.from(["secret"]) });
      await expectValidation(h.commands.setup([action]));
      expect(h.writeKey).not.toHaveBeenCalled();
      expect(h.removeKey).not.toHaveBeenCalled();
      expect(h.installHook).not.toHaveBeenCalled();
      expect(h.uninstallHook).not.toHaveBeenCalled();
    }
    const status = harness();
    await expectValidation(status.commands.setup(["status", "--confirm"]));
  });
});

describe("home command", () => {
  it("returns setup guidance without constructing a client when unconfigured", async () => {
    const h = harness({ config: { baseUrl: "https://example.test" } });
    const result = objectResult(await homeCommand(h.deps)([]));
    expect(result).toMatchObject({
      status: "not_configured",
      configured: false,
    });
    expect(h.clientFactory).not.toHaveBeenCalled();
  });

  it("reports configuration only, without an API call, when configured", async () => {
    const h = harness();
    const result = objectResult(await homeCommand(h.deps)([]));
    expect(h.clientFactory).not.toHaveBeenCalled();
    expect(h.client.calls).toHaveLength(0);
    expect(result).toEqual({
      status: "configured",
      configured: true,
      credentialSource: "environment",
      help: [
        "hevy-axi workout list",
        "hevy-axi routine list",
        "hevy-axi exercise list",
        "hevy-axi user info",
      ],
    });
  });

  it("supports home help, fields, JSON, and strict positionals", async () => {
    const help = harness();
    expect(await homeCommand(help.deps)(["--help"])).toBe(TOP_LEVEL_HELP);
    const projected = harness({ config: { baseUrl: "https://example.test" } });
    const json = await homeCommand(projected.deps)([
      "--fields",
      "status,configured",
      "--json",
    ]);
    expect(JSON.parse(json as string)).toEqual({
      status: "not_configured",
      configured: false,
    });
    await expectValidation(homeCommand(harness().deps)(["extra"]));
  });
});
