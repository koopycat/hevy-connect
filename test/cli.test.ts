import { createServer } from "node:http";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { main } from "../src/cli.js";
import { VERSION } from "../src/version.js";

const CONFIG_ENVIRONMENT_KEYS = [
  "HOME",
  "HEVY_API_KEY",
  "HEVY_API_BASE_URL",
  "HEVY_AXI_ENV_FILE",
] as const;

interface Invocation {
  readonly stdout: string;
  readonly exitCode: number;
}

async function invoke(argv: readonly string[]): Promise<Invocation> {
  let stdout = "";
  const write = vi
    .spyOn(process.stdout, "write")
    .mockImplementation((chunk: string | Uint8Array) => {
      stdout += chunk.toString();
      return true;
    });

  process.exitCode = undefined;
  try {
    await main([...argv]);
    return { stdout, exitCode: process.exitCode ?? 0 };
  } finally {
    write.mockRestore();
  }
}

describe.sequential("CLI runtime dispatch", () => {
  let originalCwd: string;
  let originalExitCode: typeof process.exitCode;
  let originalEnvironment: Record<string, string | undefined>;
  let sandbox: string;

  beforeEach(async () => {
    originalCwd = process.cwd();
    originalExitCode = process.exitCode;
    originalEnvironment = Object.fromEntries(
      CONFIG_ENVIRONMENT_KEYS.map((key) => [key, process.env[key]]),
    );
    sandbox = await mkdtemp(join(tmpdir(), "hevy-axi-cli-"));

    process.chdir(sandbox);
    for (const key of CONFIG_ENVIRONMENT_KEYS) delete process.env[key];
    process.env.HOME = sandbox;
    process.exitCode = undefined;
  });

  afterEach(async () => {
    vi.restoreAllMocks();
    process.chdir(originalCwd);
    for (const key of CONFIG_ENVIRONMENT_KEYS) {
      const value = originalEnvironment[key];
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
    process.exitCode = originalExitCode;
    await rm(sandbox, { recursive: true, force: true });
  });

  it("renders top-level help without resolving credentials", async () => {
    const result = await invoke(["--help"]);

    expect(result.exitCode).toBe(0);
    expect(result.stdout).toContain("Usage:\n  hevy-axi <command>");
    expect(result.stdout).toContain("workout      list, count, events");
    expect(result.stdout).toContain('"built-in":');
    expect(result.stdout).not.toContain("error:");
  });

  it.each(["-v", "-V", "--version"])(
    "renders the version for %s",
    async (flag) => {
      await expect(invoke([flag])).resolves.toEqual({
        stdout: `${VERSION}\n`,
        exitCode: 0,
      });
    },
  );

  it("returns a structured TOON error for an unknown command", async () => {
    const result = await invoke(["does-not-exist"]);

    expect(result.exitCode).toBe(2);
    expect(result.stdout).toMatch(
      new RegExp(
        '^error:\\n {2}code: VALIDATION_ERROR\\n {2}message: "Unknown command: does-not-exist\\."',
        "u",
      ),
    );
    expect(result.stdout).toContain("--help");
  });

  it("returns a structured JSON error for an unknown command", async () => {
    const result = await invoke(["does-not-exist", "--json"]);

    expect(result.exitCode).toBe(2);
    expect(JSON.parse(result.stdout)).toMatchObject({
      error: {
        code: "VALIDATION_ERROR",
        message: "Unknown command: does-not-exist.",
        suggestions: [expect.stringContaining("--help")],
      },
    });
  });

  it("renders the unconfigured home view with content before guidance", async () => {
    const result = await invoke([]);

    expect(result.exitCode).toBe(0);
    expect(result.stdout).toContain(
      "description: Agent-ergonomic access to the Hevy Public API.",
    );
    expect(result.stdout).toContain("status: not_configured");
    expect(result.stdout).toContain("configured: false");
    expect(result.stdout.indexOf("status: not_configured")).toBeLessThan(
      result.stdout.indexOf("help[2]:"),
    );
    expect(result.stdout).toContain("hevy-axi setup key --confirm");
    expect(result.stdout).not.toContain("AUTH_ERROR");
  });

  it("renders resource help without requiring an API key", async () => {
    const result = await invoke(["workout", "--help"]);

    expect(result.exitCode).toBe(0);
    expect(result.stdout).toMatch(
      /^Usage: hevy-axi workout <action> \[arguments\] \[flags\]/u,
    );
    expect(result.stdout).toContain("--page-size <n>");
    expect(result.stdout).toContain("workout create --file workout.json");
    expect(result.stdout).not.toContain("A Hevy API key is required");
  });

  it("uses exit code 2 for a command validation error", async () => {
    const result = await invoke(["user", "info"]);

    expect(result.exitCode).toBe(2);
    expect(result.stdout).toContain("code: VALIDATION_ERROR");
    expect(result.stdout).toContain("A Hevy API key is required.");
  });

  it("uses exit code 1 for API errors and never prints the API key", async () => {
    const apiKey = "test-secret-api-key-never-print";
    let receivedApiKey: string | undefined;
    const server = createServer((request, response) => {
      const header = request.headers["api-key"];
      receivedApiKey = Array.isArray(header) ? header[0] : header;
      response.writeHead(401, { "content-type": "application/json" });
      response.end(
        JSON.stringify({
          message: `The rejected credential was ${apiKey}`,
          [apiKey]: apiKey,
        }),
      );
    });

    await new Promise<void>((resolve, reject) => {
      server.once("error", reject);
      server.listen(0, "127.0.0.1", resolve);
    });

    try {
      const address = server.address();
      if (address === null || typeof address === "string") {
        throw new Error("Expected the test server to listen on a TCP port.");
      }
      process.env.HEVY_API_KEY = apiKey;
      process.env.HEVY_API_BASE_URL = `http://127.0.0.1:${address.port}`;

      const result = await invoke(["user", "info", "--json"]);
      const output = JSON.parse(result.stdout) as {
        error: {
          code: string;
          details: { status: number; body: unknown };
        };
      };

      expect(receivedApiKey).toBe(apiKey);
      expect(result.exitCode).toBe(1);
      expect(output.error).toMatchObject({
        code: "AUTH_ERROR",
        details: { status: 401 },
      });
      expect(JSON.stringify(output.error.details.body)).toContain("[REDACTED]");
      expect(result.stdout).not.toContain(apiKey);
    } finally {
      await new Promise<void>((resolve, reject) => {
        server.close((error) =>
          error === undefined ? resolve() : reject(error),
        );
      });
    }
  });

  it("treats stdout EPIPE as a successful pipeline exit", async () => {
    await invoke(["--version"]);
    process.exitCode = 9;
    const error = Object.assign(new Error("downstream closed"), {
      code: "EPIPE",
    });

    expect(() => process.stdout.emit("error", error)).not.toThrow();
    expect(process.exitCode).toBe(0);
  });
});
