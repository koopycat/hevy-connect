import type { JsonValue } from "./types.js";

export interface HevyCliErrorOptions {
  suggestions?: readonly string[];
  status?: number;
  details?: JsonValue;
  cause?: unknown;
}

export class HevyCliError extends Error {
  readonly code: string;
  readonly suggestions: readonly string[];
  readonly status?: number;
  readonly details?: JsonValue;

  constructor(
    code: string,
    message: string,
    options: HevyCliErrorOptions = {},
  ) {
    super(
      message,
      options.cause === undefined ? undefined : { cause: options.cause },
    );
    this.name = "HevyCliError";
    this.code = code;
    this.suggestions = options.suggestions ?? [];

    if (options.status !== undefined) {
      this.status = options.status;
    }
    if (options.details !== undefined) {
      this.details = options.details;
    }
  }
}

export function validationError(
  message: string,
  suggestions: readonly string[] = [],
): HevyCliError {
  return new HevyCliError("VALIDATION_ERROR", message, { suggestions });
}

export function isHevyCliError(error: unknown): error is HevyCliError {
  return error instanceof HevyCliError;
}

export function exitCodeForHevyError(error: HevyCliError): number {
  return error.code === "VALIDATION_ERROR" || error.code === "CONFIG_INSECURE"
    ? 2
    : 1;
}
