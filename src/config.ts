import { constants as fsConstants } from "node:fs";
import { chmod, lstat, mkdir, open, rename, unlink } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { randomBytes } from "node:crypto";

import { HevyCliError, validationError } from "./errors.js";
import type { JsonObject } from "./types.js";

export const DEFAULT_BASE_URL = "https://api.hevyapp.com";
export const DEFAULT_CREDENTIAL_PATH = "~/.config/hevy-axi/credentials.env";
export const DEFAULT_CREDENTIALS_PATH = DEFAULT_CREDENTIAL_PATH;
export const DEFAULT_HEVY_API_BASE_URL = DEFAULT_BASE_URL;
export const HEVY_AXI_CREDENTIAL_PATH = DEFAULT_CREDENTIAL_PATH;

const CREDENTIAL_RELATIVE_PATH = join(".config", "hevy-axi", "credentials.env");
const MAX_CREDENTIAL_FILE_BYTES = 64 * 1024;

export interface ResolvedConfig {
  apiKey?: string;
  baseUrl: string;
  credentialSource?: string;
}

export interface ResolveConfigOptions {
  cwd?: string;
  homeDir?: string;
  env?: Readonly<Record<string, string | undefined>>;
}

export interface WriteStoredApiKeyResult {
  status: "created" | "updated";
  path: string;
}

export interface RemoveStoredApiKeyResult {
  status: "removed" | "not_found";
  path: string;
}

interface ParsedEnvironment {
  apiKey?: string;
  baseUrl?: string;
}

interface CredentialCandidate {
  path: string;
  required: boolean;
}

function configError(
  code: "CONFIG_ERROR" | "CONFIG_INSECURE",
  message: string,
  path?: string,
): HevyCliError {
  const details: JsonObject | undefined =
    path === undefined ? undefined : { path };
  return new HevyCliError(code, message, {
    suggestions:
      code === "CONFIG_INSECURE"
        ? [
            "Use a regular file owned and readable only by your user (mode 0600).",
          ]
        : [],
    ...(details === undefined ? {} : { details }),
  });
}

function expandConfigPath(path: string, cwd: string, homeDir: string): string {
  if (path === "~") {
    return homeDir;
  }
  if (path.startsWith("~/")) {
    return join(homeDir, path.slice(2));
  }
  return isAbsolute(path) ? path : resolve(cwd, path);
}

function storedCredentialPath(homeDir: string): string {
  return join(homeDir, CREDENTIAL_RELATIVE_PATH);
}

function parseDotenvValue(rawValue: string): string | undefined {
  const value = rawValue.trim();
  if (value === "") {
    return "";
  }

  const quote = value[0];
  if (quote === '"' || quote === "'") {
    let closing = -1;
    let escaped = false;
    for (let index = 1; index < value.length; index += 1) {
      const character = value[index];
      if (quote === '"' && character === "\\" && !escaped) {
        escaped = true;
        continue;
      }
      if (character === quote && !escaped) {
        closing = index;
        break;
      }
      escaped = false;
    }
    if (closing === -1) {
      return undefined;
    }
    const remainder = value.slice(closing + 1).trim();
    if (remainder !== "" && !remainder.startsWith("#")) {
      return undefined;
    }

    const quoted = value.slice(1, closing);
    if (quote === "'") {
      return quoted;
    }
    return quoted.replace(
      /\\(n|r|t|"|\\)/g,
      (_match, escapedCharacter: string) => {
        switch (escapedCharacter) {
          case "n":
            return "\n";
          case "r":
            return "\r";
          case "t":
            return "\t";
          default:
            return escapedCharacter;
        }
      },
    );
  }

  const comment = value.indexOf("#");
  return (comment === -1 ? value : value.slice(0, comment)).trim();
}

function parseEnvironmentFile(
  contents: string,
  path: string,
): ParsedEnvironment {
  const parsed: ParsedEnvironment = {};

  for (const line of contents.split(/\r?\n/u)) {
    const match =
      /^\s*(?:export\s+)?(HEVY_API_KEY|HEVY_API_BASE_URL)\s*=\s*(.*)$/u.exec(
        line,
      );
    if (match === null) {
      continue;
    }

    const name = match[1];
    const value = parseDotenvValue(match[2] ?? "");
    if (value === undefined) {
      throw configError(
        "CONFIG_ERROR",
        "A credential file contains an invalid value.",
        path,
      );
    }
    if (name === "HEVY_API_KEY") {
      parsed.apiKey = value;
    } else {
      parsed.baseUrl = value;
    }
  }

  return parsed;
}

async function readCredentialCandidate(
  candidate: CredentialCandidate,
): Promise<ParsedEnvironment | undefined> {
  const noFollow =
    typeof fsConstants.O_NOFOLLOW === "number" ? fsConstants.O_NOFOLLOW : 0;
  let handle;
  try {
    handle = await open(candidate.path, fsConstants.O_RDONLY | noFollow);
  } catch (error) {
    if (isNodeError(error, "ENOENT") && !candidate.required) {
      return undefined;
    }
    if (isNodeError(error, "ENOENT")) {
      throw configError(
        "CONFIG_ERROR",
        "The configured credential file does not exist.",
        candidate.path,
      );
    }
    if (isNodeError(error, "ELOOP")) {
      throw configError(
        "CONFIG_INSECURE",
        "The credential path must not be a symbolic link.",
        candidate.path,
      );
    }
    throw configError(
      "CONFIG_ERROR",
      "The credential file could not be opened safely.",
      candidate.path,
    );
  }

  try {
    const openedMetadata = await handle.stat();
    let pathMetadata;
    try {
      pathMetadata = await lstat(candidate.path);
    } catch (error) {
      if (isNodeError(error, "ENOENT")) {
        throw configError(
          "CONFIG_INSECURE",
          "The credential path changed while it was being opened.",
          candidate.path,
        );
      }
      throw error;
    }

    if (
      pathMetadata.isSymbolicLink() ||
      !pathMetadata.isFile() ||
      !openedMetadata.isFile()
    ) {
      throw configError(
        "CONFIG_INSECURE",
        "The credential path must be a regular, non-symbolic-link file.",
        candidate.path,
      );
    }
    const hasStableIdentity =
      openedMetadata.ino !== 0 && pathMetadata.ino !== 0;
    if (
      hasStableIdentity &&
      (openedMetadata.dev !== pathMetadata.dev ||
        openedMetadata.ino !== pathMetadata.ino)
    ) {
      throw configError(
        "CONFIG_INSECURE",
        "The credential path changed while it was being opened.",
        candidate.path,
      );
    }
    if (
      process.platform !== "win32" &&
      ((openedMetadata.mode & 0o077) !== 0 ||
        (typeof process.getuid === "function" &&
          openedMetadata.uid !== process.getuid()))
    ) {
      throw configError(
        "CONFIG_INSECURE",
        "The credential file must be owned by the current user with mode 0600.",
        candidate.path,
      );
    }
    if (openedMetadata.size > MAX_CREDENTIAL_FILE_BYTES) {
      throw configError(
        "CONFIG_ERROR",
        "The credential file exceeds the 64 KiB size limit.",
        candidate.path,
      );
    }

    const buffer = Buffer.alloc(MAX_CREDENTIAL_FILE_BYTES + 1);
    let bytesRead = 0;
    while (bytesRead < buffer.byteLength) {
      const result = await handle.read(
        buffer,
        bytesRead,
        buffer.byteLength - bytesRead,
        null,
      );
      if (result.bytesRead === 0) {
        break;
      }
      bytesRead += result.bytesRead;
    }
    if (bytesRead > MAX_CREDENTIAL_FILE_BYTES) {
      throw configError(
        "CONFIG_ERROR",
        "The credential file exceeds the 64 KiB size limit.",
        candidate.path,
      );
    }

    return parseEnvironmentFile(
      buffer.subarray(0, bytesRead).toString("utf8"),
      candidate.path,
    );
  } catch (error) {
    if (error instanceof HevyCliError) {
      throw error;
    }
    throw configError(
      "CONFIG_ERROR",
      "The credential file could not be read safely.",
      candidate.path,
    );
  } finally {
    await handle.close().catch(() => undefined);
  }
}

function normalizeBaseUrl(value: string): string {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw configError("CONFIG_ERROR", "The Hevy API base URL is invalid.");
  }

  if (url.username !== "" || url.password !== "") {
    throw configError(
      "CONFIG_ERROR",
      "The Hevy API base URL must not contain user information.",
    );
  }
  if (url.search !== "" || url.hash !== "") {
    throw configError(
      "CONFIG_ERROR",
      "The Hevy API base URL must not contain a query or fragment.",
    );
  }

  const localHttpHosts = new Set(["localhost", "127.0.0.1", "[::1]"]);
  if (
    url.protocol !== "https:" &&
    !(url.protocol === "http:" && localHttpHosts.has(url.hostname))
  ) {
    throw configError(
      "CONFIG_ERROR",
      "The Hevy API base URL must use HTTPS (except for a local test server).",
    );
  }

  return url.toString().replace(/\/+$/u, "");
}

function nonblank(value: string | undefined): string | undefined {
  if (value === undefined || value.trim() === "") {
    return undefined;
  }
  return value.trim();
}

/** Resolves credentials without ever placing the credential itself in source metadata. */
export async function resolveConfig(
  options: ResolveConfigOptions = {},
): Promise<ResolvedConfig> {
  const cwd = options.cwd ?? process.cwd();
  const homeDir = options.homeDir ?? homedir();
  const env = options.env ?? process.env;

  let apiKey = nonblank(env.HEVY_API_KEY);
  let credentialSource = apiKey === undefined ? undefined : "environment";
  const environmentBaseUrl = nonblank(env.HEVY_API_BASE_URL);
  let baseUrlValue = environmentBaseUrl;

  if (apiKey !== undefined && /[\r\n\0]/u.test(apiKey)) {
    throw configError(
      "CONFIG_ERROR",
      "The HEVY_API_KEY environment value is invalid.",
    );
  }

  // A directly supplied key is intentionally independent of every credential
  // file. In particular, a local .env must not be able to redirect it.
  if (apiKey === undefined) {
    const candidates: CredentialCandidate[] = [];
    const explicitFile = nonblank(env.HEVY_AXI_ENV_FILE);
    if (explicitFile !== undefined) {
      candidates.push({
        path: expandConfigPath(explicitFile, cwd, homeDir),
        required: true,
      });
    }
    candidates.push({ path: resolve(cwd, ".env"), required: false });
    candidates.push({ path: storedCredentialPath(homeDir), required: false });

    let unboundBaseUrl: string | undefined;
    const seen = new Set<string>();
    for (const candidate of candidates) {
      if (seen.has(candidate.path)) {
        continue;
      }
      seen.add(candidate.path);

      const parsed = await readCredentialCandidate(candidate);
      if (parsed === undefined) {
        continue;
      }
      unboundBaseUrl ??= nonblank(parsed.baseUrl);
      const candidateKey = nonblank(parsed.apiKey);
      if (candidateKey === undefined) {
        continue;
      }
      if (/[\r\n\0]/u.test(candidateKey)) {
        throw configError(
          "CONFIG_ERROR",
          "A credential file contains an invalid API key.",
          candidate.path,
        );
      }

      apiKey = candidateKey;
      credentialSource = candidate.path;
      // Environment base URL is an explicit override. Otherwise a file key is
      // bound only to a base URL from that exact same file.
      baseUrlValue = environmentBaseUrl ?? nonblank(parsed.baseUrl);
      break;
    }
    if (apiKey === undefined && baseUrlValue === undefined) {
      baseUrlValue = unboundBaseUrl;
    }
  }

  const result: ResolvedConfig = {
    baseUrl: normalizeBaseUrl(baseUrlValue ?? DEFAULT_BASE_URL),
  };
  if (apiKey !== undefined) {
    result.apiKey = apiKey;
  }
  if (credentialSource !== undefined) {
    result.credentialSource = credentialSource;
  }
  return result;
}

export function requireApiKey(config: ResolvedConfig): string {
  const apiKey = nonblank(config.apiKey);
  if (apiKey === undefined) {
    throw validationError("A Hevy API key is required.", [
      "Set HEVY_API_KEY or run the credential configuration command.",
    ]);
  }
  return apiKey;
}

function isNodeError(error: unknown, code: string): boolean {
  return (
    error instanceof Error &&
    "code" in error &&
    typeof error.code === "string" &&
    error.code === code
  );
}

async function inspectWriteTarget(path: string): Promise<boolean> {
  try {
    const metadata = await lstat(path);
    if (metadata.isSymbolicLink() || !metadata.isFile()) {
      throw configError(
        "CONFIG_INSECURE",
        "The stored credential target must be a regular, non-symbolic-link file.",
        path,
      );
    }
    return true;
  } catch (error) {
    if (isNodeError(error, "ENOENT")) {
      return false;
    }
    if (error instanceof HevyCliError) {
      throw error;
    }
    throw configError(
      "CONFIG_ERROR",
      "The stored credential target could not be inspected.",
      path,
    );
  }
}

async function prepareCredentialDirectory(path: string): Promise<void> {
  try {
    await mkdir(path, { recursive: true, mode: 0o700 });
    const metadata = await lstat(path);
    if (metadata.isSymbolicLink() || !metadata.isDirectory()) {
      throw configError(
        "CONFIG_INSECURE",
        "The credential directory must be a non-symbolic-link directory.",
        path,
      );
    }
    if (process.platform !== "win32") {
      await chmod(path, 0o700);
    }
  } catch (error) {
    if (error instanceof HevyCliError) {
      throw error;
    }
    throw configError(
      "CONFIG_ERROR",
      "The credential directory could not be secured.",
      path,
    );
  }
}

export async function writeStoredApiKey(
  apiKey: string,
  options: ResolveConfigOptions = {},
): Promise<WriteStoredApiKeyResult> {
  const normalizedKey = apiKey.trim();
  if (normalizedKey === "" || /[\r\n\0]/u.test(normalizedKey)) {
    throw validationError("The Hevy API key is invalid.");
  }

  const path = storedCredentialPath(options.homeDir ?? homedir());
  const directory = dirname(path);
  await prepareCredentialDirectory(directory);
  const existed = await inspectWriteTarget(path);
  const temporaryPath = join(
    directory,
    `.credentials.env.${process.pid}.${randomBytes(8).toString("hex")}.tmp`,
  );

  let handle;
  try {
    handle = await open(
      temporaryPath,
      fsConstants.O_WRONLY | fsConstants.O_CREAT | fsConstants.O_EXCL,
      0o600,
    );
    const quotedKey = normalizedKey.replace(/([\\"])/gu, "\\$1");
    await handle.writeFile(`HEVY_API_KEY="${quotedKey}"\n`, "utf8");
    await handle.sync();
    await handle.close();
    handle = undefined;
    await rename(temporaryPath, path);
    if (process.platform !== "win32") {
      await chmod(path, 0o600);
    }
  } catch {
    if (handle !== undefined) {
      await handle.close().catch(() => undefined);
    }
    await unlink(temporaryPath).catch(() => undefined);
    throw configError("CONFIG_ERROR", "The API key could not be stored.", path);
  }

  return { status: existed ? "updated" : "created", path };
}

export async function removeStoredApiKey(
  options: ResolveConfigOptions = {},
): Promise<RemoveStoredApiKeyResult> {
  const path = storedCredentialPath(options.homeDir ?? homedir());
  const exists = await inspectWriteTarget(path);
  if (!exists) {
    return { status: "not_found", path };
  }

  try {
    await unlink(path);
  } catch (error) {
    if (isNodeError(error, "ENOENT")) {
      return { status: "not_found", path };
    }
    throw configError(
      "CONFIG_ERROR",
      "The stored API key could not be removed.",
      path,
    );
  }
  return { status: "removed", path };
}
