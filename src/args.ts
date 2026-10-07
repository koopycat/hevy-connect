import { validationError } from "./errors.js";

export type OutputFormat = "toon" | "json";

export interface FlagDefinition {
  readonly kind: "boolean" | "string";
  readonly repeatable?: boolean;
}

export interface ParsedArgs {
  readonly positionals: string[];
  readonly flags: Readonly<Record<string, string | boolean | string[]>>;
}

export const COMMON_OUTPUT_FLAGS: Readonly<Record<string, FlagDefinition>> = {
  format: { kind: "string" },
  json: { kind: "boolean" },
  full: { kind: "boolean" },
  fields: { kind: "string" },
};

function flagDisplay(name: string): string {
  return `--${name}`;
}

/** Parse long options without silently accepting misspellings or positional options. */
export function parseArgs(
  args: readonly string[],
  definitions: Readonly<Record<string, FlagDefinition>>,
): ParsedArgs {
  const positionals: string[] = [];
  const flags: Record<string, string | boolean | string[]> = {};
  let positionalOnly = false;

  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === undefined) {
      continue;
    }
    if (argument === "--") {
      positionalOnly = true;
      continue;
    }
    if (positionalOnly || !argument.startsWith("-")) {
      positionals.push(argument);
      continue;
    }
    if (!argument.startsWith("--") || argument === "--") {
      throw validationError(`Unknown flag: ${argument}.`);
    }

    const equals = argument.indexOf("=");
    const name = argument.slice(2, equals === -1 ? undefined : equals);
    const definition = definitions[name];
    if (definition === undefined) {
      throw validationError(`Unknown flag: ${flagDisplay(name)}.`);
    }
    const inlineValue = equals === -1 ? undefined : argument.slice(equals + 1);
    let value: string | boolean;
    if (definition.kind === "boolean") {
      if (inlineValue !== undefined) {
        throw validationError(`${flagDisplay(name)} does not take a value.`);
      }
      value = true;
    } else {
      const next = args[index + 1];
      if (inlineValue !== undefined) {
        value = inlineValue;
      } else if (
        next !== undefined &&
        // A bare "-" is the conventional stdin operand, not a flag.
        (next === "-" || !next.startsWith("-"))
      ) {
        value = next;
        index += 1;
      } else {
        throw validationError(`${flagDisplay(name)} requires a value.`);
      }
      if (value === "") {
        throw validationError(
          `${flagDisplay(name)} requires a non-empty value.`,
        );
      }
    }

    const existing = flags[name];
    if (existing !== undefined && definition.repeatable !== true) {
      throw validationError(`${flagDisplay(name)} may only be specified once.`);
    }
    if (definition.repeatable === true) {
      const values = Array.isArray(existing) ? existing : [];
      values.push(String(value));
      flags[name] = values;
    } else {
      flags[name] = value;
    }
  }

  return { positionals, flags };
}

export function combineFlags(
  ...groups: ReadonlyArray<Readonly<Record<string, FlagDefinition>>>
): Readonly<Record<string, FlagDefinition>> {
  return Object.assign({}, ...groups);
}

export function booleanFlag(parsed: ParsedArgs, name: string): boolean {
  return parsed.flags[name] === true;
}

export function stringFlag(
  parsed: ParsedArgs,
  name: string,
): string | undefined {
  const value = parsed.flags[name];
  return typeof value === "string" ? value : undefined;
}

export function positiveSafeInteger(
  value: string | undefined,
  displayName: string,
): number | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (!/^[1-9]\d*$/u.test(value)) {
    throw validationError(`${displayName} must be a positive integer.`);
  }
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed)) {
    throw validationError(`${displayName} must be a safe integer.`);
  }
  return parsed;
}

export function outputFormat(parsed: ParsedArgs): OutputFormat {
  const explicit = stringFlag(parsed, "format");
  if (explicit !== undefined && explicit !== "toon" && explicit !== "json") {
    throw validationError('--format must be either "toon" or "json".');
  }
  if (booleanFlag(parsed, "json") && explicit === "toon") {
    throw validationError("--json cannot be combined with --format toon.");
  }
  return booleanFlag(parsed, "json") || explicit === "json" ? "json" : "toon";
}

export function requirePositionalCount(
  parsed: ParsedArgs,
  count: number,
  usage: string,
): void {
  if (parsed.positionals.length !== count) {
    throw validationError(`Usage: ${usage}`);
  }
}
