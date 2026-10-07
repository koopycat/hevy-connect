import { HevyCliError, validationError } from "./errors.js";
import type { HttpMethod, JsonObject, JsonValue } from "./types.js";

const DEFAULT_TIMEOUT_MS = 20_000;
const DEFAULT_MAX_READ_RETRIES = 2;
const MAX_RESPONSE_BYTES = 10 * 1024 * 1024;
const MAX_ERROR_BYTES = 8 * 1024;
const MAX_RETRY_AFTER_MS = 30_000;
const BASE_RETRY_DELAY_MS = 250;

export interface HevyClientOptions {
  apiKey: string;
  baseUrl: string;
  fetchImpl?: typeof fetch;
  timeoutMs?: number;
  maxReadRetries?: number;
  sleep?: (delayMs: number) => Promise<void>;
}

export type QueryScalar = string | number | boolean;

export interface RequestOptions {
  query?: Readonly<Record<string, QueryScalar | null | undefined>>;
  body?: JsonValue;
}

interface ReadBodyResult {
  text: string;
  truncated: boolean;
}

function defaultSleep(delayMs: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, delayMs));
}

function validateNonnegativeInteger(value: number, name: string): void {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw validationError(`${name} must be a non-negative integer.`);
  }
}

function normalizeClientBaseUrl(value: string): string {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw validationError("The Hevy API base URL is invalid.");
  }
  if (url.username !== "" || url.password !== "") {
    throw validationError(
      "The Hevy API base URL must not contain user information.",
    );
  }
  if (url.search !== "" || url.hash !== "") {
    throw validationError(
      "The Hevy API base URL must not contain a query or fragment.",
    );
  }
  const localHttpHosts = new Set(["localhost", "127.0.0.1", "[::1]"]);
  if (
    url.protocol !== "https:" &&
    !(url.protocol === "http:" && localHttpHosts.has(url.hostname))
  ) {
    throw validationError("The Hevy API base URL must use HTTPS.");
  }
  return url.toString().replace(/\/+$/u, "");
}

async function readBoundedBody(
  response: Response,
  limit: number,
): Promise<ReadBodyResult> {
  if (response.body === null) {
    return { text: "", truncated: false };
  }

  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let bytesRead = 0;
  let truncated = false;

  try {
    while (true) {
      const result = await reader.read();
      if (result.done) {
        break;
      }
      if (result.value.byteLength > limit - bytesRead) {
        const remaining = limit - bytesRead;
        if (remaining > 0) {
          chunks.push(result.value.subarray(0, remaining));
          bytesRead += remaining;
        }
        truncated = true;
        await reader.cancel();
        break;
      }
      chunks.push(result.value);
      bytesRead += result.value.byteLength;
    }
  } finally {
    reader.releaseLock();
  }

  const combined = new Uint8Array(bytesRead);
  let offset = 0;
  for (const chunk of chunks) {
    combined.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return { text: new TextDecoder().decode(combined), truncated };
}

function parseBody(
  text: string,
  contentType: string | null,
): JsonValue | string | undefined {
  if (text === "") {
    return undefined;
  }
  const looksLikeJson =
    contentType?.toLowerCase().includes("application/json") === true ||
    contentType?.toLowerCase().includes("+json") === true;
  if (looksLikeJson) {
    try {
      return JSON.parse(text) as JsonValue;
    } catch {
      return text;
    }
  }
  return text;
}

function redact(
  value: JsonValue | string | undefined,
  secret: string,
): JsonValue | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (typeof value === "string") {
    return secret === "" ? value : value.split(secret).join("[REDACTED]");
  }
  if (Array.isArray(value)) {
    return value.map((entry) => redact(entry, secret) ?? null);
  }
  if (value !== null && typeof value === "object") {
    const result: JsonObject = {};
    for (const [key, entry] of Object.entries(value)) {
      const safeKey =
        secret === "" ? key : key.split(secret).join("[REDACTED]");
      result[safeKey] = redact(entry, secret) ?? null;
    }
    return result;
  }
  return value;
}

function retryAfterDelay(
  value: string | null,
  now = Date.now(),
): number | undefined {
  if (value === null) {
    return undefined;
  }
  const trimmed = value.trim();
  if (/^\d+(?:\.\d+)?$/u.test(trimmed)) {
    const milliseconds = Number(trimmed) * 1_000;
    return Number.isFinite(milliseconds)
      ? Math.min(Math.max(0, milliseconds), MAX_RETRY_AFTER_MS)
      : MAX_RETRY_AFTER_MS;
  }
  const date = Date.parse(trimmed);
  if (Number.isNaN(date)) {
    return undefined;
  }
  return Math.min(Math.max(0, date - now), MAX_RETRY_AFTER_MS);
}

function retryableStatus(status: number): boolean {
  return status === 429 || status === 502 || status === 503 || status === 504;
}

function httpErrorCode(status: number): string {
  if (status === 401 || status === 403) {
    return "AUTH_ERROR";
  }
  if (status === 404) {
    return "NOT_FOUND";
  }
  if (status === 429) {
    return "RATE_LIMITED";
  }
  return "API_ERROR";
}

function mutationFailureSuggestions(method: HttpMethod): string[] {
  return method === "POST" || method === "PUT"
    ? [
        "The mutation outcome may be unknown. Inspect Hevy before manually retrying to avoid a duplicate or overwrite.",
      ]
    : [];
}

function httpErrorMessage(status: number): string {
  if (status === 401 || status === 403) {
    return "Hevy rejected the API credentials or denied access.";
  }
  if (status === 404) {
    return "The requested Hevy resource was not found.";
  }
  if (status === 429) {
    return "Hevy rate-limited the request.";
  }
  return `The Hevy API returned HTTP ${status}.`;
}

export class HevyClient {
  readonly #apiKey: string;
  readonly #baseUrl: string;
  readonly #fetch: typeof fetch;
  readonly #timeoutMs: number;
  readonly #maxReadRetries: number;
  readonly #sleep: (delayMs: number) => Promise<void>;

  constructor(options: HevyClientOptions) {
    if (options.apiKey.trim() === "") {
      throw validationError("A Hevy API key is required.");
    }
    const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
    const maxReadRetries = options.maxReadRetries ?? DEFAULT_MAX_READ_RETRIES;
    validateNonnegativeInteger(timeoutMs, "timeoutMs");
    validateNonnegativeInteger(maxReadRetries, "maxReadRetries");

    this.#apiKey = options.apiKey;
    this.#baseUrl = normalizeClientBaseUrl(options.baseUrl);
    this.#fetch = options.fetchImpl ?? fetch;
    this.#timeoutMs = timeoutMs;
    this.#maxReadRetries = maxReadRetries;
    this.#sleep = options.sleep ?? defaultSleep;
  }

  async request<T>(
    method: HttpMethod,
    path: string,
    options: RequestOptions = {},
  ): Promise<T> {
    if (!path.startsWith("/v1/")) {
      throw validationError('Hevy API paths must begin with "/v1/".');
    }

    const url = new URL(`${this.#baseUrl}${path}`);
    for (const [name, value] of Object.entries(options.query ?? {})) {
      if (value !== null && value !== undefined) {
        url.searchParams.append(name, String(value));
      }
    }

    let serializedBody: string | undefined;
    if (options.body !== undefined) {
      try {
        serializedBody = JSON.stringify(options.body);
      } catch {
        throw validationError("The request body must be JSON-compatible.");
      }
    }

    const retries = method === "GET" ? this.#maxReadRetries : 0;
    for (let attempt = 0; ; attempt += 1) {
      const controller = new AbortController();
      let timedOut = false;
      let timeoutHandle: ReturnType<typeof setTimeout> | undefined;
      const timeout = new Promise<never>((_resolve, reject) => {
        timeoutHandle = setTimeout(() => {
          timedOut = true;
          controller.abort();
          reject(new Error("Request timed out"));
        }, this.#timeoutMs);
      });
      let retryDelay: number | undefined;

      try {
        const response = await Promise.race([
          this.#fetch(url, {
            method,
            redirect: "manual",
            headers: {
              accept: "application/json",
              "content-type": "application/json",
              "api-key": this.#apiKey,
            },
            signal: controller.signal,
            ...(serializedBody === undefined ? {} : { body: serializedBody }),
          }),
          timeout,
        ]);

        if (response.status >= 300 && response.status < 400) {
          // Never inspect Location or the body: redirect targets and payloads
          // are untrusted, and no redirect may receive the API key.
          throw new HevyCliError(
            "UNSAFE_REDIRECT",
            "The Hevy API request was redirected and was blocked.",
            {
              status: response.status,
              details: { status: response.status },
            },
          );
        } else if (attempt < retries && retryableStatus(response.status)) {
          await response.body?.cancel().catch(() => undefined);
          retryDelay =
            retryAfterDelay(response.headers.get("retry-after")) ??
            Math.min(BASE_RETRY_DELAY_MS * 2 ** attempt, 2_000);
        } else if (response.ok) {
          const body = await Promise.race([
            readBoundedBody(response, MAX_RESPONSE_BYTES),
            timeout,
          ]);
          if (body.truncated) {
            throw new HevyCliError(
              "API_ERROR",
              "The Hevy API response was too large.",
              {
                status: response.status,
                details: {
                  status: response.status,
                  maximumBytes: MAX_RESPONSE_BYTES,
                },
              },
            );
          }
          return parseBody(
            body.text,
            response.headers.get("content-type"),
          ) as T;
        } else {
          const body = await Promise.race([
            readBoundedBody(response, MAX_ERROR_BYTES),
            timeout,
          ]);
          const parsedBody = parseBody(
            body.text,
            response.headers.get("content-type"),
          );
          const safeBody = redact(parsedBody, this.#apiKey);
          const details: JsonObject = { status: response.status };
          if (safeBody !== undefined) {
            details.body = safeBody;
          }
          if (body.truncated) {
            details.bodyTruncated = true;
          }

          throw new HevyCliError(
            httpErrorCode(response.status),
            httpErrorMessage(response.status),
            {
              status: response.status,
              details,
              suggestions:
                response.status === 401 || response.status === 403
                  ? ["Check the configured Hevy API key and account access."]
                  : [],
            },
          );
        }
      } catch (error) {
        if (timedOut) {
          throw new HevyCliError("TIMEOUT", "The Hevy API request timed out.", {
            details: { timeoutMs: this.#timeoutMs },
            suggestions: mutationFailureSuggestions(method),
          });
        }
        if (error instanceof HevyCliError) {
          throw error;
        }
        if (error instanceof Error && error.name === "AbortError") {
          throw new HevyCliError(
            "NETWORK_ERROR",
            "The Hevy API request was aborted.",
            { suggestions: mutationFailureSuggestions(method) },
          );
        }
        if (attempt < retries) {
          retryDelay = Math.min(BASE_RETRY_DELAY_MS * 2 ** attempt, 2_000);
        } else {
          throw new HevyCliError(
            "NETWORK_ERROR",
            "The Hevy API request failed.",
            { suggestions: mutationFailureSuggestions(method) },
          );
        }
      } finally {
        if (timeoutHandle !== undefined) {
          clearTimeout(timeoutHandle);
        }
      }

      await this.#sleep(retryDelay ?? 0);
    }
  }

  get<T>(path: string, options?: RequestOptions): Promise<T> {
    return this.request<T>("GET", path, options);
  }

  post<T>(path: string, options?: RequestOptions): Promise<T> {
    return this.request<T>("POST", path, options);
  }

  put<T>(path: string, options?: RequestOptions): Promise<T> {
    return this.request<T>("PUT", path, options);
  }
}
