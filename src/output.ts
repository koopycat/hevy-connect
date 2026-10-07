import { encode } from "@toon-format/toon";

import type { OutputFormat } from "./args.js";
import {
  HevyCliError,
  exitCodeForHevyError,
  validationError,
} from "./errors.js";
import type { JsonObject, JsonValue } from "./types.js";

const DEFAULT_STRING_LIMIT = 240;
const DEFAULT_ARRAY_LIMIT = 50;
const UNSAFE_PATH_SEGMENTS = new Set(["__proto__", "prototype", "constructor"]);

export interface OutputOptions {
  readonly full?: boolean;
  readonly fields?: readonly string[];
  readonly availableFields?: readonly string[];
}

export interface FormattedCliError {
  output: string;
  exitCode: number;
}

export function renderOutput(value: JsonValue, format: OutputFormat): string {
  return format === "json" ? JSON.stringify(value, null, 2) : encode(value);
}

export function formatResult(
  value: JsonValue,
  format: OutputFormat,
): JsonValue | string {
  // axi-sdk-js renders objects as TOON. A string bypasses that rendering in JSON mode.
  return format === "json" ? JSON.stringify(value, null, 2) : value;
}

export function formatCliError(
  error: unknown,
  format: OutputFormat = "toon",
): FormattedCliError {
  const known = error instanceof HevyCliError;
  const value: JsonObject = {
    error: {
      code: known ? error.code : "UNEXPECTED_ERROR",
      message: known ? error.message : "An unexpected error occurred.",
      ...(known && error.details !== undefined
        ? { details: error.details }
        : {}),
      suggestions: known ? [...error.suggestions] : [],
    },
  };
  return {
    output: renderOutput(value, format),
    exitCode: known ? exitCodeForHevyError(error) : 1,
  };
}

/** Quote a value for a POSIX shell command shown in help output. */
export function shellArgument(value: string): string {
  return /^[A-Za-z0-9_/:.+=-]+$/u.test(value)
    ? value
    : `'${value.replace(/'/gu, "'\\''")}'`;
}

export function parseFields(value: string | undefined): string[] | undefined {
  if (value === undefined) {
    return undefined;
  }
  const fields = value.split(",").map((field) => field.trim());
  if (fields.length === 0 || fields.some((field) => field === "")) {
    throw validationError(
      "--fields must be a comma-separated list of non-empty paths.",
    );
  }
  for (const field of fields) {
    const segments = field.split(".");
    if (
      segments.some(
        (segment) =>
          segment === "" ||
          UNSAFE_PATH_SEGMENTS.has(segment) ||
          !/^[A-Za-z0-9_-]+$/u.test(segment),
      )
    ) {
      throw validationError(`Unsafe field path: ${field}.`);
    }
  }
  return [...new Set(fields)];
}

function isObject(value: JsonValue): value is JsonObject {
  return value !== null && !Array.isArray(value) && typeof value === "object";
}

function collectFieldPaths(
  value: JsonValue,
  prefix: string,
  fields: Set<string>,
): void {
  if (Array.isArray(value)) {
    for (const entry of value) {
      collectFieldPaths(entry, prefix, fields);
    }
    return;
  }
  if (!isObject(value)) {
    return;
  }
  for (const [key, entry] of Object.entries(value)) {
    const path = prefix === "" ? key : `${prefix}.${key}`;
    fields.add(path);
    collectFieldPaths(entry, path, fields);
  }
}

function availableFieldPaths(value: JsonValue): string[] {
  const fields = new Set<string>();
  collectFieldPaths(value, "", fields);
  return [...fields].sort();
}

function projectPath(value: JsonValue, segments: readonly string[]): JsonValue {
  if (segments.length === 0) {
    return value;
  }
  if (Array.isArray(value)) {
    return value.map((entry) => projectPath(entry, segments));
  }
  if (!isObject(value)) {
    return null;
  }
  const [head, ...tail] = segments;
  if (head === undefined || !(head in value)) {
    return null;
  }
  const selected = value[head];
  if (selected === undefined) {
    return null;
  }
  return tail.length === 0
    ? { [head]: selected }
    : { [head]: projectPath(selected, tail) };
}

function mergeProjected(left: JsonValue, right: JsonValue): JsonValue {
  if (Array.isArray(left) && Array.isArray(right)) {
    return left.map((entry, index) =>
      mergeProjected(entry, right[index] ?? null),
    );
  }
  if (isObject(left) && isObject(right)) {
    const result: JsonObject = { ...left };
    for (const [key, value] of Object.entries(right)) {
      result[key] =
        result[key] === undefined
          ? value
          : mergeProjected(result[key] ?? null, value);
    }
    return result;
  }
  return right;
}

export function projectFields(
  value: JsonValue,
  fields: readonly string[],
  declaredFields?: readonly string[],
): JsonValue {
  const available = declaredFields ?? availableFieldPaths(value);
  const availableSet = new Set(available);
  const unknown = fields.filter((field) => !availableSet.has(field));
  if (unknown.length > 0) {
    throw validationError(
      `Unknown field${unknown.length === 1 ? "" : "s"}: ${unknown.join(", ")}.`,
      [
        available.length === 0
          ? "No fields are available for this output."
          : `Available fields: ${available.join(", ")}`,
      ],
    );
  }

  let result: JsonValue = Array.isArray(value) ? value.map(() => ({})) : {};
  for (const field of fields) {
    result = mergeProjected(result, projectPath(value, field.split(".")));
  }
  return result;
}

interface TruncationState {
  truncated: boolean;
  omittedItems: number;
}

function truncateValue(
  value: JsonValue,
  state: TruncationState,
  preserveArray = false,
): JsonValue {
  if (typeof value === "string" && value.length > DEFAULT_STRING_LIMIT) {
    state.truncated = true;
    return `${value.slice(0, DEFAULT_STRING_LIMIT)}…`;
  }
  if (Array.isArray(value)) {
    const limit = preserveArray ? value.length : DEFAULT_ARRAY_LIMIT;
    if (value.length > limit) {
      state.truncated = true;
      state.omittedItems += value.length - limit;
    }
    return value.slice(0, limit).map((entry) => truncateValue(entry, state));
  }
  if (isObject(value)) {
    const result: JsonObject = {};
    const isTopLevelList =
      typeof value.resultCount === "number" && Array.isArray(value.results);
    for (const [key, entry] of Object.entries(value)) {
      result[key] = truncateValue(
        entry,
        state,
        isTopLevelList && key === "results",
      );
    }
    return result;
  }
  return value;
}

/** Truncate long strings and annotate object results so loss is explicit. */
export function truncateOutput(value: JsonValue): JsonValue {
  const state: TruncationState = { truncated: false, omittedItems: 0 };
  const result = truncateValue(value, state);
  if (!state.truncated) {
    return result;
  }
  if (isObject(result)) {
    return {
      ...result,
      truncated: true,
      ...(state.omittedItems > 0 ? { omittedItems: state.omittedItems } : {}),
      truncationHelp: "Use --full to return untruncated wire data.",
    };
  }
  return {
    value: result,
    truncated: true,
    ...(state.omittedItems > 0 ? { omittedItems: state.omittedItems } : {}),
    truncationHelp: "Use --full to return untruncated wire data.",
  };
}

export function finalizeOutput(
  value: JsonValue,
  options: OutputOptions,
): JsonValue {
  if (options.full === true) {
    return value;
  }
  const selected =
    options.fields === undefined
      ? value
      : projectFields(value, options.fields, options.availableFields);
  return truncateOutput(selected);
}
