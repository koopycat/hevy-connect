import {
  lstat,
  mkdir,
  readFile,
  rm,
  symlink,
  writeFile,
} from "node:fs/promises";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { HevyClient } from "../src/client.js";
import {
  DEFAULT_BASE_URL,
  removeStoredApiKey,
  requireApiKey,
  resolveConfig,
  writeStoredApiKey,
} from "../src/config.js";
import {
  exitCodeForHevyError,
  HevyCliError,
  isHevyCliError,
  validationError,
} from "../src/errors.js";
import type { JsonValue } from "../src/types.js";

const tempRoots: string[] = [];

async function makeTempRoot(): Promise<string> {
  const { mkdtemp } = await import("node:fs/promises");
  const root = await mkdtemp(join(tmpdir(), "hevy-axi-test-"));
  tempRoots.push(root);
  return root;
}

async function writeCredential(
  path: string,
  contents: string,
  mode = 0o600,
): Promise<void> {
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, contents, { encoding: "utf8", mode });
}

function fetchMock(
  implementation: (
    input: RequestInfo | URL,
    init?: RequestInit,
  ) => Promise<Response>,
): typeof fetch {
  return vi.fn(implementation) as unknown as typeof fetch;
}

function clientWith(
  mockedFetch: typeof fetch,
  overrides: Partial<ConstructorParameters<typeof HevyClient>[0]> = {},
): HevyClient {
  return new HevyClient({
    apiKey: "test-secret-key",
    baseUrl: "https://api.example.test",
    fetchImpl: mockedFetch,
    sleep: async () => undefined,
    ...overrides,
  });
}

function errorSnapshot(error: unknown): string {
  if (error instanceof Error) {
    return `${String(error)} ${JSON.stringify(error)}`;
  }
  return JSON.stringify(error);
}

afterEach(async () => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  await Promise.all(
    tempRoots.splice(0).map(async (root) => {
      await rm(root, { recursive: true, force: true });
    }),
  );
});

describe("configuration resolution", () => {
  it("uses environment credentials and URL ahead of every credential file", async () => {
    const root = await makeTempRoot();
    const cwd = join(root, "work");
    const homeDir = join(root, "home");
    const explicit = join(root, "explicit.env");
    await writeCredential(
      explicit,
      "HEVY_API_KEY=explicit-key\nHEVY_API_BASE_URL=https://explicit.example/\n",
    );
    await writeCredential(
      join(cwd, ".env"),
      "HEVY_API_KEY=cwd-key\nHEVY_API_BASE_URL=https://cwd.example/\n",
    );
    await writeCredential(
      join(homeDir, ".config/hevy-axi/credentials.env"),
      "HEVY_API_KEY=global-key\nHEVY_API_BASE_URL=https://global.example/\n",
    );

    await expect(
      resolveConfig({
        cwd,
        homeDir,
        env: {
          HEVY_API_KEY: "  environment-key  ",
          HEVY_API_BASE_URL: "https://environment.example///",
          HEVY_AXI_ENV_FILE: explicit,
        },
      }),
    ).resolves.toEqual({
      apiKey: "environment-key",
      baseUrl: "https://environment.example",
      credentialSource: "environment",
    });
  });

  it("uses an explicit env file ahead of cwd and global files", async () => {
    const root = await makeTempRoot();
    const cwd = join(root, "work");
    const homeDir = join(root, "home");
    const explicit = join(root, "explicit.env");
    await writeCredential(
      explicit,
      "HEVY_API_KEY=explicit-key\nHEVY_API_BASE_URL=https://explicit.example/base/\n",
    );
    await writeCredential(join(cwd, ".env"), "HEVY_API_KEY=cwd-key\n");
    await writeCredential(
      join(homeDir, ".config/hevy-axi/credentials.env"),
      "HEVY_API_KEY=global-key\n",
    );

    await expect(
      resolveConfig({
        cwd,
        homeDir,
        env: { HEVY_AXI_ENV_FILE: "../explicit.env" },
      }),
    ).resolves.toEqual({
      apiKey: "explicit-key",
      baseUrl: "https://explicit.example/base",
      credentialSource: explicit,
    });
  });

  it("uses cwd .env ahead of the global credential file without borrowing its URL", async () => {
    const root = await makeTempRoot();
    const cwd = join(root, "work");
    const homeDir = join(root, "home");
    const cwdFile = join(cwd, ".env");
    await writeCredential(cwdFile, "HEVY_API_KEY=cwd-key\n");
    await writeCredential(
      join(homeDir, ".config/hevy-axi/credentials.env"),
      "HEVY_API_KEY=global-key\nHEVY_API_BASE_URL=https://global.example\n",
    );

    await expect(resolveConfig({ cwd, homeDir, env: {} })).resolves.toEqual({
      apiKey: "cwd-key",
      baseUrl: DEFAULT_BASE_URL,
      credentialSource: cwdFile,
    });
  });

  it("does not inspect credential files when an environment key is set", async () => {
    const root = await makeTempRoot();
    const cwd = join(root, "work");
    const poisonedUrl = "https://attacker.example";
    await writeCredential(
      join(cwd, ".env"),
      `HEVY_API_BASE_URL=${poisonedUrl}\n`,
    );

    await expect(
      resolveConfig({
        cwd,
        homeDir: join(root, "home"),
        env: {
          HEVY_API_KEY: "direct-key",
          HEVY_AXI_ENV_FILE: join(root, "missing-explicit.env"),
        },
      }),
    ).resolves.toEqual({
      apiKey: "direct-key",
      baseUrl: DEFAULT_BASE_URL,
      credentialSource: "environment",
    });
  });

  it("uses a direct environment URL override with every key source", async () => {
    const root = await makeTempRoot();
    const cwd = join(root, "work");
    const homeDir = join(root, "home");
    const override = "https://proxy.example/base";
    await writeCredential(
      join(cwd, ".env"),
      "HEVY_API_KEY=file-key\nHEVY_API_BASE_URL=https://file.example\n",
    );

    await expect(
      resolveConfig({
        cwd,
        homeDir,
        env: { HEVY_API_BASE_URL: override },
      }),
    ).resolves.toEqual({
      apiKey: "file-key",
      baseUrl: override,
      credentialSource: join(cwd, ".env"),
    });
  });

  it.each([
    ["explicit base and cwd key", "explicit", "cwd"],
    ["cwd base and global key", "cwd", "global"],
  ])("never combines files for %s", async (_label, baseSource, keySource) => {
    const root = await makeTempRoot();
    const cwd = join(root, "work");
    const homeDir = join(root, "home");
    const explicit = join(root, "explicit.env");
    const cwdFile = join(cwd, ".env");
    const globalFile = join(homeDir, ".config/hevy-axi/credentials.env");
    const files = { explicit, cwd: cwdFile, global: globalFile };
    await writeCredential(explicit, "# explicitly selected credential file\n");
    await writeCredential(
      files[baseSource as keyof typeof files],
      "HEVY_API_BASE_URL=https://attacker.example\n",
    );
    await writeCredential(
      files[keySource as keyof typeof files],
      `${keySource === "global" ? "HEVY_API_KEY=global-key" : "HEVY_API_KEY=cwd-key"}\n`,
    );

    const config = await resolveConfig({
      cwd,
      homeDir,
      env: { HEVY_AXI_ENV_FILE: explicit },
    });
    expect(config).toMatchObject({
      apiKey: keySource === "global" ? "global-key" : "cwd-key",
      baseUrl: DEFAULT_BASE_URL,
      credentialSource: files[keySource as keyof typeof files],
    });
    expect(JSON.stringify(config)).not.toContain("attacker.example");
  });

  it("falls back to the global credential file and expands an explicit ~/ path", async () => {
    const root = await makeTempRoot();
    const cwd = join(root, "work");
    const homeDir = join(root, "home");
    const globalFile = join(homeDir, ".config/hevy-axi/credentials.env");
    await writeCredential(globalFile, "HEVY_API_KEY=global-key\n");

    await expect(resolveConfig({ cwd, homeDir, env: {} })).resolves.toEqual({
      apiKey: "global-key",
      baseUrl: DEFAULT_BASE_URL,
      credentialSource: globalFile,
    });

    const namedFile = join(homeDir, "named.env");
    await writeCredential(namedFile, "HEVY_API_KEY=tilde-key\n");
    await expect(
      resolveConfig({
        cwd,
        homeDir,
        env: { HEVY_AXI_ENV_FILE: "~/named.env" },
      }),
    ).resolves.toMatchObject({
      apiKey: "tilde-key",
      credentialSource: namedFile,
    });
  });

  it("parses export syntax, comments, quoting, escapes, and quoted base URLs", async () => {
    const root = await makeTempRoot();
    const cwd = join(root, "work");
    const file = join(cwd, ".env");
    await writeCredential(
      file,
      [
        "IGNORED=value",
        "export HEVY_API_KEY=old-value # ignored suffix",
        String.raw`HEVY_API_KEY="quoted\"key\\part" # comment`,
        "HEVY_API_BASE_URL='https://quoted.example/v1/' # comment",
        "",
      ].join("\n"),
    );

    await expect(
      resolveConfig({ cwd, homeDir: join(root, "home"), env: {} }),
    ).resolves.toEqual({
      apiKey: 'quoted"key\\part',
      baseUrl: "https://quoted.example/v1",
      credentialSource: file,
    });
  });

  it("reports invalid dotenv quoting without disclosing earlier values", async () => {
    const root = await makeTempRoot();
    const file = join(root, "work", ".env");
    const secret = "must-not-leak";
    await writeCredential(
      file,
      `HEVY_API_KEY=${secret}\nHEVY_API_BASE_URL="unterminated\n`,
    );

    let caught: unknown;
    try {
      await resolveConfig({
        cwd: dirname(file),
        homeDir: join(root, "home"),
        env: {},
      });
    } catch (error) {
      caught = error;
    }
    expect(caught).toMatchObject({ code: "CONFIG_ERROR" });
    expect(errorSnapshot(caught)).not.toContain(secret);
  });

  it("returns default configuration when no files or environment values exist", async () => {
    const root = await makeTempRoot();
    await expect(
      resolveConfig({
        cwd: join(root, "work"),
        homeDir: join(root, "home"),
        env: {},
      }),
    ).resolves.toEqual({ baseUrl: DEFAULT_BASE_URL });
  });

  it("reports a missing explicitly configured file", async () => {
    const root = await makeTempRoot();
    const missing = join(root, "missing.env");
    await expect(
      resolveConfig({
        cwd: root,
        homeDir: join(root, "home"),
        env: { HEVY_AXI_ENV_FILE: missing },
      }),
    ).rejects.toMatchObject({
      code: "CONFIG_ERROR",
      details: { path: missing },
    });
  });

  it("rejects invalid environment API keys without echoing them", async () => {
    const root = await makeTempRoot();
    const secret = "first-line\nsecond-line";
    let caught: unknown;
    try {
      await resolveConfig({
        cwd: root,
        homeDir: join(root, "home"),
        env: { HEVY_API_KEY: secret },
      });
    } catch (error) {
      caught = error;
    }
    expect(caught).toMatchObject({ code: "CONFIG_ERROR" });
    expect(errorSnapshot(caught)).not.toContain(secret);
  });

  it.runIf(process.platform !== "win32")(
    "rejects group- or world-accessible files containing API keys",
    async () => {
      const root = await makeTempRoot();
      const file = join(root, "work", ".env");
      await writeCredential(file, "HEVY_API_KEY=insecure-key\n", 0o644);

      await expect(
        resolveConfig({
          cwd: dirname(file),
          homeDir: join(root, "home"),
          env: {},
        }),
      ).rejects.toMatchObject({
        code: "CONFIG_INSECURE",
        details: { path: file },
      });
    },
  );

  it.runIf(process.platform !== "win32")(
    "rejects non-owner-only credential files even when they contain only a URL",
    async () => {
      const root = await makeTempRoot();
      const file = join(root, "work", ".env");
      await writeCredential(
        file,
        "HEVY_API_BASE_URL=https://public.example/\n",
        0o644,
      );

      await expect(
        resolveConfig({
          cwd: dirname(file),
          homeDir: join(root, "home"),
          env: {},
        }),
      ).rejects.toMatchObject({
        code: "CONFIG_INSECURE",
        details: { path: file },
      });
    },
  );

  it.runIf(process.platform !== "win32")(
    "ignores a group- or world-readable project .env without Hevy settings",
    async () => {
      const root = await makeTempRoot();
      const homeDir = join(root, "home");
      const project = join(root, "work");
      const stored = join(homeDir, ".config/hevy-axi/credentials.env");
      await writeCredential(join(project, ".env"), "DATABASE_URL=x\n", 0o644);
      await writeCredential(stored, "HEVY_API_KEY=global-key\n");

      await expect(
        resolveConfig({ cwd: project, homeDir, env: {} }),
      ).resolves.toEqual({
        apiKey: "global-key",
        baseUrl: DEFAULT_BASE_URL,
        credentialSource: stored,
      });
    },
  );

  it("ignores a project .env directory such as a virtualenv", async () => {
    const root = await makeTempRoot();
    const homeDir = join(root, "home");
    const project = join(root, "work");
    const stored = join(homeDir, ".config/hevy-axi/credentials.env");
    await mkdir(join(project, ".env", "bin"), { recursive: true });
    await writeCredential(stored, "HEVY_API_KEY=global-key\n");

    await expect(
      resolveConfig({ cwd: project, homeDir, env: {} }),
    ).resolves.toMatchObject({
      apiKey: "global-key",
      credentialSource: stored,
    });
  });

  it("rejects credential files larger than 64 KiB", async () => {
    const root = await makeTempRoot();
    const file = join(root, "work", ".env");
    await writeCredential(file, `#${"x".repeat(64 * 1024)}\n`);

    await expect(
      resolveConfig({
        cwd: dirname(file),
        homeDir: join(root, "home"),
        env: {},
      }),
    ).rejects.toMatchObject({
      code: "CONFIG_ERROR",
      details: { path: file },
    });
  });

  it("rejects symlinked credential files", async () => {
    const root = await makeTempRoot();
    const target = join(root, "target.env");
    const linked = join(root, "work", ".env");
    await writeCredential(target, "HEVY_API_KEY=secret\n");
    await mkdir(dirname(linked), { recursive: true });
    await symlink(target, linked);

    await expect(
      resolveConfig({
        cwd: dirname(linked),
        homeDir: join(root, "home"),
        env: {},
      }),
    ).rejects.toMatchObject({
      code: "CONFIG_INSECURE",
      details: { path: linked },
    });
  });

  it("rejects non-file credential paths", async () => {
    const root = await makeTempRoot();
    const path = join(root, "credentials-dir");
    await mkdir(path, { recursive: true });

    await expect(
      resolveConfig({
        cwd: root,
        homeDir: join(root, "home"),
        env: { HEVY_AXI_ENV_FILE: path },
      }),
    ).rejects.toMatchObject({ code: "CONFIG_INSECURE" });
  });
});

describe("configuration URL and API-key validation", () => {
  it.each([
    ["https://example.test/", "https://example.test"],
    ["https://example.test/base///", "https://example.test/base"],
    ["http://localhost:3000/", "http://localhost:3000"],
    ["http://127.0.0.1:3000/", "http://127.0.0.1:3000"],
    ["http://[::1]:3000/", "http://[::1]:3000"],
  ])("accepts and normalizes %s", async (input, expected) => {
    const root = await makeTempRoot();
    await expect(
      resolveConfig({
        cwd: root,
        homeDir: join(root, "home"),
        env: { HEVY_API_BASE_URL: input },
      }),
    ).resolves.toEqual({ baseUrl: expected });
  });

  it.each([
    "not-a-url",
    "http://api.example.test",
    "ftp://api.example.test",
    "https://user:password@example.test",
    "https://example.test?key=value",
    "https://example.test#fragment",
  ])("rejects unsafe or malformed base URL %s", async (baseUrl) => {
    const root = await makeTempRoot();
    await expect(
      resolveConfig({
        cwd: root,
        homeDir: join(root, "home"),
        env: { HEVY_API_BASE_URL: baseUrl },
      }),
    ).rejects.toMatchObject({ code: "CONFIG_ERROR" });
  });

  it("requires a nonblank API key and gives actionable metadata", () => {
    expect(
      requireApiKey({ apiKey: "  value  ", baseUrl: DEFAULT_BASE_URL }),
    ).toBe("value");

    expect(() => requireApiKey({ baseUrl: DEFAULT_BASE_URL })).toThrowError(
      expect.objectContaining({
        code: "VALIDATION_ERROR",
        suggestions: expect.arrayContaining([
          expect.stringContaining("HEVY_API_KEY"),
        ]),
      }),
    );
    expect(() =>
      requireApiKey({ apiKey: "   ", baseUrl: DEFAULT_BASE_URL }),
    ).toThrowError(expect.objectContaining({ code: "VALIDATION_ERROR" }));
  });
});

describe("stored API key management", () => {
  it("atomically creates and updates the global key with private modes", async () => {
    const homeDir = await makeTempRoot();
    const path = join(homeDir, ".config/hevy-axi/credentials.env");
    const directory = dirname(path);
    const firstKey = 'first-"key\\value';
    const first = await writeStoredApiKey(firstKey, { homeDir });

    expect(first).toEqual({ status: "created", path });
    expect(JSON.stringify(first)).not.toContain(firstKey);
    expect(await readFile(path, "utf8")).toBe(
      'HEVY_API_KEY="first-\\"key\\\\value"\n',
    );
    expect((await lstat(path)).isFile()).toBe(true);
    expect((await lstat(path)).isSymbolicLink()).toBe(false);
    if (process.platform !== "win32") {
      expect((await lstat(path)).mode & 0o777).toBe(0o600);
      expect((await lstat(directory)).mode & 0o777).toBe(0o700);
    }
    await expect(
      (await import("node:fs/promises"))
        .readdir(directory)
        .then((entries) => entries.filter((entry) => entry.endsWith(".tmp"))),
    ).resolves.toEqual([]);

    const secondKey = "replacement-key";
    const second = await writeStoredApiKey(secondKey, { homeDir });
    expect(second).toEqual({ status: "updated", path });
    expect(JSON.stringify(second)).not.toContain(secondKey);
    expect(await readFile(path, "utf8")).toBe(
      'HEVY_API_KEY="replacement-key"\n',
    );
    if (process.platform !== "win32") {
      expect((await lstat(path)).mode & 0o777).toBe(0o600);
    }
  });

  it("round-trips a stored escaped key through config resolution", async () => {
    const root = await makeTempRoot();
    const homeDir = join(root, "home");
    const key = 'quote"and\\slash';
    await writeStoredApiKey(key, { homeDir });

    await expect(
      resolveConfig({ cwd: join(root, "work"), homeDir, env: {} }),
    ).resolves.toMatchObject({ apiKey: key });
  });

  it("removes a stored key and reports an absent key idempotently", async () => {
    const homeDir = await makeTempRoot();
    const path = join(homeDir, ".config/hevy-axi/credentials.env");
    const secret = "remove-me-secret";
    await writeStoredApiKey(secret, { homeDir });

    const removed = await removeStoredApiKey({ homeDir });
    expect(removed).toEqual({ status: "removed", path });
    expect(JSON.stringify(removed)).not.toContain(secret);
    await expect(lstat(path)).rejects.toMatchObject({ code: "ENOENT" });

    const absent = await removeStoredApiKey({ homeDir });
    expect(absent).toEqual({ status: "not_found", path });
    expect(JSON.stringify(absent)).not.toContain(secret);
  });

  it("treats concurrent removal races as idempotent", async () => {
    const homeDir = await makeTempRoot();
    const path = join(homeDir, ".config/hevy-axi/credentials.env");
    await writeStoredApiKey("raced-secret", { homeDir });

    const results = await Promise.all([
      removeStoredApiKey({ homeDir }),
      removeStoredApiKey({ homeDir }),
    ]);
    expect(results).toHaveLength(2);
    expect(results).toEqual(
      expect.arrayContaining([expect.objectContaining({ path })]),
    );
    expect(
      results.every(
        ({ status }) => status === "removed" || status === "not_found",
      ),
    ).toBe(true);
  });

  it.each(["", "   ", "secret\nsecond-line", "secret\0tail"])(
    "rejects invalid keys without exposing them: %j",
    async (secret) => {
      const homeDir = await makeTempRoot();
      let caught: unknown;
      try {
        await writeStoredApiKey(secret, { homeDir });
      } catch (error) {
        caught = error;
      }
      expect(caught).toMatchObject({ code: "VALIDATION_ERROR" });
      if (secret !== "" && secret.trim() !== "") {
        expect(errorSnapshot(caught)).not.toContain(secret);
      }
    },
  );

  it("refuses to overwrite or remove a symlinked global credential", async () => {
    const homeDir = await makeTempRoot();
    const path = join(homeDir, ".config/hevy-axi/credentials.env");
    const target = join(homeDir, "actual.env");
    const secret = "never-written-secret";
    await writeCredential(target, "unchanged\n");
    await mkdir(dirname(path), { recursive: true });
    await symlink(target, path);

    for (const operation of [
      () => writeStoredApiKey(secret, { homeDir }),
      () => removeStoredApiKey({ homeDir }),
    ]) {
      let caught: unknown;
      try {
        await operation();
      } catch (error) {
        caught = error;
      }
      expect(caught).toMatchObject({ code: "CONFIG_INSECURE" });
      expect(errorSnapshot(caught)).not.toContain(secret);
    }
    await expect(readFile(target, "utf8")).resolves.toBe("unchanged\n");
  });

  it("refuses a symlinked credential directory", async () => {
    const homeDir = await makeTempRoot();
    const actual = join(homeDir, "actual-directory");
    const configDirectory = join(homeDir, ".config");
    const linkedDirectory = join(configDirectory, "hevy-axi");
    await mkdir(actual, { recursive: true });
    await mkdir(configDirectory, { recursive: true });
    await symlink(actual, linkedDirectory, "dir");

    await expect(
      writeStoredApiKey("directory-link-secret", { homeDir }),
    ).rejects.toMatchObject({ code: "CONFIG_INSECURE" });
  });
});

describe("HevyClient requests and successful responses", () => {
  it("sends API headers, encoded query values, and a JSON request body", async () => {
    const mockedFetch = fetchMock(async () =>
      Promise.resolve(
        new Response(JSON.stringify({ id: 1 }), {
          headers: { "content-type": "application/json" },
        }),
      ),
    );
    const client = clientWith(mockedFetch);

    await expect(
      client.post<{ id: number }>("/v1/routines", {
        query: {
          name: "a value & more",
          page: 2,
          enabled: false,
          absent: null,
          missing: undefined,
        },
        body: { name: "Routine", active: true },
      }),
    ).resolves.toEqual({ id: 1 });

    expect(mockedFetch).toHaveBeenCalledTimes(1);
    const [input, init] = vi.mocked(mockedFetch).mock.calls[0] ?? [];
    expect(input).toBeInstanceOf(URL);
    const url = input as URL;
    expect(url.origin + url.pathname).toBe(
      "https://api.example.test/v1/routines",
    );
    expect(Object.fromEntries(url.searchParams)).toEqual({
      name: "a value & more",
      page: "2",
      enabled: "false",
    });
    expect(init).toMatchObject({
      method: "POST",
      redirect: "manual",
      headers: {
        accept: "application/json",
        "content-type": "application/json",
        "api-key": "test-secret-key",
      },
      body: '{"name":"Routine","active":true}',
    });
    expect(init?.signal).toBeInstanceOf(AbortSignal);
  });

  it("omits the request body and nullish query parameters", async () => {
    const mockedFetch = fetchMock(async () =>
      Promise.resolve(new Response(null, { status: 204 })),
    );
    const client = clientWith(mockedFetch);

    await expect(
      client.get("/v1/workouts", {
        query: { page: null, limit: undefined },
      }),
    ).resolves.toBeUndefined();
    const [input, init] = vi.mocked(mockedFetch).mock.calls[0] ?? [];
    expect((input as URL).search).toBe("");
    expect(init).not.toHaveProperty("body");
  });

  it.each([
    [
      "JSON",
      new Response('{"ok":true}', {
        headers: { "content-type": "application/json; charset=utf-8" },
      }),
      { ok: true },
    ],
    [
      "vendor JSON",
      new Response("[1,2]", {
        headers: { "content-type": "application/vnd.hevy+json" },
      }),
      [1, 2],
    ],
    [
      "text",
      new Response("plain response", {
        headers: { "content-type": "text/plain" },
      }),
      "plain response",
    ],
    ["empty content", new Response(null, { status: 204 }), undefined],
    [
      "malformed JSON",
      new Response("{not valid", {
        headers: { "content-type": "application/json" },
      }),
      "{not valid",
    ],
  ])("returns successful %s bodies", async (_label, response, expected) => {
    const mockedFetch = fetchMock(async () => Promise.resolve(response));
    await expect(clientWith(mockedFetch).get("/v1/resource")).resolves.toEqual(
      expected,
    );
  });

  it("supports PUT and strips trailing slashes from the base URL", async () => {
    const mockedFetch = fetchMock(async () =>
      Promise.resolve(new Response('{"ok":true}', { status: 200 })),
    );
    const client = clientWith(mockedFetch, {
      baseUrl: "https://api.example.test///",
    });

    await client.put("/v1/resource", { body: { value: 1 } });
    expect(String(vi.mocked(mockedFetch).mock.calls[0]?.[0])).toBe(
      "https://api.example.test/v1/resource",
    );
  });

  it("rejects non-JSON-compatible request bodies before fetch", async () => {
    const mockedFetch = fetchMock(async () => Promise.resolve(new Response()));
    const circular: Record<string, unknown> = {};
    circular.self = circular;
    const client = clientWith(mockedFetch);

    await expect(
      client.post("/v1/resource", { body: circular as JsonValue }),
    ).rejects.toMatchObject({ code: "VALIDATION_ERROR" });
    expect(mockedFetch).not.toHaveBeenCalled();
  });
});

describe("HevyClient retries", () => {
  beforeEach(() => {
    vi.spyOn(Math, "random").mockReturnValue(0);
  });

  it("retries GET network failures with bounded exponential delays", async () => {
    const mockedFetch = fetchMock(
      vi
        .fn<() => Promise<Response>>()
        .mockRejectedValueOnce(new Error("offline"))
        .mockRejectedValueOnce(new Error("still offline"))
        .mockResolvedValueOnce(
          new Response('{"ok":true}', {
            headers: { "content-type": "application/json" },
          }),
        ),
    );
    const sleep = vi.fn(async (_delayMs: number) => undefined);
    const client = clientWith(mockedFetch, { maxReadRetries: 2, sleep });

    await expect(client.get("/v1/resource")).resolves.toEqual({ ok: true });
    expect(mockedFetch).toHaveBeenCalledTimes(3);
    expect(sleep.mock.calls.map(([delay]) => delay)).toEqual([250, 500]);
  });

  it.each([429, 502, 503, 504])("retries GET after HTTP %i", async (status) => {
    const mockedFetch = fetchMock(
      vi
        .fn<() => Promise<Response>>()
        .mockResolvedValueOnce(new Response("retry", { status }))
        .mockResolvedValueOnce(new Response("ok", { status: 200 })),
    );
    const sleep = vi.fn(async (_delayMs: number) => undefined);
    const client = clientWith(mockedFetch, { maxReadRetries: 1, sleep });

    await expect(client.get("/v1/resource")).resolves.toBe("ok");
    expect(mockedFetch).toHaveBeenCalledTimes(2);
    expect(sleep).toHaveBeenCalledWith(250);
  });

  it.each([
    ["0.125", 125],
    ["999999", 30_000],
    ["invalid", 250],
  ])("honors and bounds Retry-After %s", async (retryAfter, expectedDelay) => {
    const mockedFetch = fetchMock(
      vi
        .fn<() => Promise<Response>>()
        .mockResolvedValueOnce(
          new Response("retry", {
            status: 429,
            headers: { "retry-after": retryAfter },
          }),
        )
        .mockResolvedValueOnce(new Response("ok")),
    );
    const sleep = vi.fn(async (_delayMs: number) => undefined);
    const client = clientWith(mockedFetch, { maxReadRetries: 1, sleep });

    await expect(client.get("/v1/resource")).resolves.toBe("ok");
    expect(sleep).toHaveBeenCalledExactlyOnceWith(expectedDelay);
  });

  it.each([400, 401, 404, 500])(
    "does not retry non-retryable HTTP %i",
    async (status) => {
      const mockedFetch = fetchMock(async () =>
        Promise.resolve(new Response("failure", { status })),
      );
      const sleep = vi.fn(async (_delayMs: number) => undefined);
      const client = clientWith(mockedFetch, { maxReadRetries: 3, sleep });

      await expect(client.get("/v1/resource")).rejects.toBeInstanceOf(
        HevyCliError,
      );
      expect(mockedFetch).toHaveBeenCalledTimes(1);
      expect(sleep).not.toHaveBeenCalled();
    },
  );

  it.each(["post", "put"] as const)(
    "does not retry %s network failures and warns about unknown mutation outcomes",
    async (method) => {
      const secret = "mutation-network-secret";
      const mockedFetch = fetchMock(async () =>
        Promise.reject(new Error(`offline ${secret}`)),
      );
      const sleep = vi.fn(async (_delayMs: number) => undefined);
      const client = clientWith(mockedFetch, {
        apiKey: secret,
        maxReadRetries: 5,
        sleep,
      });
      let caught: unknown;
      try {
        await client[method]("/v1/resource");
      } catch (error) {
        caught = error;
      }

      expect(caught).toMatchObject({
        code: "NETWORK_ERROR",
        suggestions: [expect.stringMatching(/outcome may be unknown/i)],
      });
      expect(errorSnapshot(caught)).toMatch(/inspect Hevy/i);
      expect(errorSnapshot(caught)).toMatch(/avoid a duplicate or overwrite/i);
      expect(errorSnapshot(caught)).not.toContain(secret);
      expect(mockedFetch).toHaveBeenCalledTimes(1);
      expect(sleep).not.toHaveBeenCalled();
    },
  );

  it.each(["post", "put"] as const)(
    "does not retry %s retryable HTTP responses",
    async (method) => {
      const mockedFetch = fetchMock(async () =>
        Promise.resolve(new Response("busy", { status: 503 })),
      );
      const client = clientWith(mockedFetch, { maxReadRetries: 5 });

      await expect(client[method]("/v1/resource")).rejects.toMatchObject({
        code: "API_ERROR",
        status: 503,
      });
      expect(mockedFetch).toHaveBeenCalledTimes(1);
    },
  );
});

describe("HevyClient redirect protection", () => {
  it.each(Array.from({ length: 100 }, (_, index) => index + 300))(
    "rejects HTTP %i without retrying or returning redirect data",
    async (status) => {
      const secret = "redirect-secret";
      const target = `https://attacker.example/collect?key=${secret}`;
      const response = new Response(null, {
        status,
        headers: { location: target },
      });
      const locationGet = vi.spyOn(response.headers, "get");
      const mockedFetch = fetchMock(async () => Promise.resolve(response));
      const sleep = vi.fn(async (_delayMs: number) => undefined);
      let caught: unknown;
      try {
        await clientWith(mockedFetch, {
          apiKey: secret,
          maxReadRetries: 5,
          sleep,
        }).get("/v1/resource");
      } catch (error) {
        caught = error;
      }

      expect(caught).toMatchObject({
        code: "UNSAFE_REDIRECT",
        status,
        details: { status },
      });
      expect((caught as HevyCliError).details).toEqual({ status });
      expect(errorSnapshot(caught)).not.toContain(secret);
      expect(errorSnapshot(caught)).not.toContain("attacker.example");
      expect(mockedFetch).toHaveBeenCalledTimes(1);
      expect(mockedFetch).toHaveBeenCalledWith(
        expect.any(URL),
        expect.objectContaining({ redirect: "manual" }),
      );
      expect(locationGet).not.toHaveBeenCalledWith("location");
      expect(sleep).not.toHaveBeenCalled();
    },
  );

  it("does not send the API key to a cross-origin redirect target", async () => {
    const secret = "cross-origin-secret";
    const targetHeaders: Array<string | undefined> = [];
    const targetServer = createServer((request, response) => {
      targetHeaders.push(
        Array.isArray(request.headers["api-key"])
          ? request.headers["api-key"][0]
          : request.headers["api-key"],
      );
      response.end("unexpected");
    });
    await new Promise<void>((resolveListen) =>
      targetServer.listen(0, "127.0.0.1", resolveListen),
    );
    const targetPort = (targetServer.address() as AddressInfo).port;

    const sourceServer = createServer((_request, response) => {
      response.writeHead(302, {
        location: `http://127.0.0.1:${targetPort}/stolen`,
      });
      response.end("redirecting");
    });
    await new Promise<void>((resolveListen) =>
      sourceServer.listen(0, "127.0.0.1", resolveListen),
    );
    const sourcePort = (sourceServer.address() as AddressInfo).port;

    try {
      const client = new HevyClient({
        apiKey: secret,
        baseUrl: `http://127.0.0.1:${sourcePort}`,
        maxReadRetries: 5,
      });
      let caught: unknown;
      try {
        await client.get("/v1/resource");
      } catch (error) {
        caught = error;
      }

      expect(caught).toMatchObject({
        code: "UNSAFE_REDIRECT",
        status: 302,
        details: { status: 302 },
      });
      expect(errorSnapshot(caught)).not.toContain(secret);
      await new Promise((resolveDelay) => setTimeout(resolveDelay, 25));
      expect(targetHeaders).toEqual([]);
    } finally {
      await Promise.all([
        new Promise<void>((resolveClose, rejectClose) =>
          sourceServer.close((error) =>
            error === undefined ? resolveClose() : rejectClose(error),
          ),
        ),
        new Promise<void>((resolveClose, rejectClose) =>
          targetServer.close((error) =>
            error === undefined ? resolveClose() : rejectClose(error),
          ),
        ),
      ]);
    }
  });
});

describe("HevyClient failures", () => {
  it.each([
    [400, "BAD_REQUEST"],
    [401, "AUTH_ERROR"],
    [403, "FORBIDDEN"],
    [404, "NOT_FOUND"],
    [409, "CONFLICT"],
    [429, "RATE_LIMITED"],
    [500, "API_ERROR"],
  ])("maps HTTP %i to structured %s errors", async (status, code) => {
    const mockedFetch = fetchMock(async () =>
      Promise.resolve(
        new Response('{"message":"failure"}', {
          status,
          headers: { "content-type": "application/json" },
        }),
      ),
    );
    const client = clientWith(mockedFetch, { maxReadRetries: 0 });

    await expect(client.get("/v1/resource")).rejects.toMatchObject({
      name: "HevyCliError",
      code,
      status,
      details: { status, body: { message: "failure" } },
    });
  });

  it("suggests a key check for 401 and an account limit for 403", async () => {
    const suggestionsFor = async (status: number): Promise<string> => {
      const mockedFetch = fetchMock(async () =>
        Promise.resolve(new Response("denied", { status })),
      );
      let caught: unknown;
      try {
        await clientWith(mockedFetch, { maxReadRetries: 0 }).get(
          "/v1/resource",
        );
      } catch (error) {
        caught = error;
      }
      expect(caught).toBeInstanceOf(HevyCliError);
      return (caught as HevyCliError).suggestions.join(" ");
    };

    expect(await suggestionsFor(401)).toMatch(/API key/);
    expect(await suggestionsFor(403)).toMatch(/account limits/);
    expect(await suggestionsFor(404)).toBe("");
  });

  it("uses Hevy's redacted error text in the message", async () => {
    const secret = "message-secret-key";
    const mockedFetch = fetchMock(async () =>
      Promise.resolve(
        new Response(
          JSON.stringify({ error: `Routine limit exceeded for ${secret}` }),
          { status: 403, headers: { "content-type": "application/json" } },
        ),
      ),
    );

    await expect(
      clientWith(mockedFetch, { apiKey: secret, maxReadRetries: 0 }).post(
        "/v1/routines",
      ),
    ).rejects.toMatchObject({
      code: "FORBIDDEN",
      message:
        "Hevy refused the request: Routine limit exceeded for [REDACTED]",
    });
  });

  it("reports the system error code, never the message, of a network failure", async () => {
    const secret = "network-cause-secret";
    const mockedFetch = fetchMock(async () =>
      Promise.reject(
        new TypeError("fetch failed", {
          cause: Object.assign(new Error(`connect ECONNREFUSED ${secret}`), {
            code: "ECONNREFUSED",
          }),
        }),
      ),
    );
    let caught: unknown;
    try {
      await clientWith(mockedFetch, { maxReadRetries: 0 }).get("/v1/resource");
    } catch (error) {
      caught = error;
    }

    expect(caught).toMatchObject({
      code: "NETWORK_ERROR",
      details: { cause: "ECONNREFUSED" },
    });
    expect(errorSnapshot(caught)).not.toContain(secret);
  });

  it("redacts the API key from nested error values and object keys", async () => {
    const secret = "top-secret-key";
    const responseBody = JSON.stringify({
      message: `server echoed ${secret}`,
      nested: [`prefix-${secret}-suffix`, { [secret]: secret }],
    });
    const mockedFetch = fetchMock(async () =>
      Promise.resolve(
        new Response(responseBody, {
          status: 500,
          headers: { "content-type": "application/json" },
        }),
      ),
    );
    let caught: unknown;
    try {
      await clientWith(mockedFetch, {
        apiKey: secret,
        maxReadRetries: 0,
      }).get("/v1/resource");
    } catch (error) {
      caught = error;
    }

    expect(caught).toBeInstanceOf(HevyCliError);
    expect(errorSnapshot(caught)).not.toContain(secret);
    expect(JSON.stringify((caught as HevyCliError).details)).toContain(
      "[REDACTED]",
    );
  });

  it("bounds error bodies and marks them as truncated", async () => {
    const body = "x".repeat(8 * 1024 + 100);
    const mockedFetch = fetchMock(async () =>
      Promise.resolve(
        new Response(body, {
          status: 500,
          headers: { "content-type": "text/plain" },
        }),
      ),
    );
    let caught: unknown;
    try {
      await clientWith(mockedFetch, { maxReadRetries: 0 }).get("/v1/resource");
    } catch (error) {
      caught = error;
    }

    expect(caught).toMatchObject({
      code: "API_ERROR",
      details: { status: 500, bodyTruncated: true },
    });
    const details = (caught as HevyCliError).details as {
      body: string;
      bodyTruncated: boolean;
    };
    expect(new TextEncoder().encode(details.body)).toHaveLength(8 * 1024);
  });

  it("rejects successful response bodies above the 10 MiB limit", async () => {
    const oversized = new Uint8Array(10 * 1024 * 1024 + 1);
    oversized.fill(120);
    const mockedFetch = fetchMock(async () =>
      Promise.resolve(new Response(oversized, { status: 200 })),
    );

    await expect(
      clientWith(mockedFetch).get("/v1/resource"),
    ).rejects.toMatchObject({
      code: "API_ERROR",
      status: 200,
      details: { status: 200, maximumBytes: 10 * 1024 * 1024 },
    });
  });

  it("turns an AbortError from fetch into a structured network error", async () => {
    const mockedFetch = fetchMock(async () =>
      Promise.reject(new DOMException("aborted", "AbortError")),
    );

    await expect(
      clientWith(mockedFetch, { maxReadRetries: 0 }).get("/v1/resource"),
    ).rejects.toMatchObject({
      code: "NETWORK_ERROR",
      message: "The Hevy API request was aborted.",
    });
  });

  it.each(["post", "put"] as const)(
    "warns safely when a %s mutation times out without retrying",
    async (method) => {
      vi.useFakeTimers();
      const secret = "mutation-timeout-secret";
      const mockedFetch = fetchMock(
        async (_input: RequestInfo | URL, init?: RequestInit) =>
          new Promise<Response>((_resolve, reject) => {
            init?.signal?.addEventListener("abort", () => {
              reject(new DOMException(secret, "AbortError"));
            });
          }),
      );
      const request = clientWith(mockedFetch, {
        apiKey: secret,
        timeoutMs: 25,
        maxReadRetries: 5,
      })[method]("/v1/resource");
      const assertion = expect(request).rejects.toMatchObject({
        code: "TIMEOUT",
        suggestions: [expect.stringMatching(/inspect Hevy/i)],
      });

      await vi.advanceTimersByTimeAsync(25);
      await assertion;
      try {
        await request;
      } catch (error) {
        expect(errorSnapshot(error)).toMatch(/avoid a duplicate or overwrite/i);
        expect(errorSnapshot(error)).not.toContain(secret);
      }
      expect(mockedFetch).toHaveBeenCalledTimes(1);
    },
  );

  it("keeps GET timeout guidance unchanged", async () => {
    vi.useFakeTimers();
    let observedSignal: AbortSignal | undefined;
    const mockedFetch = fetchMock(
      async (_input: RequestInfo | URL, init?: RequestInit) =>
        new Promise<Response>((_resolve, reject) => {
          observedSignal = init?.signal ?? undefined;
          observedSignal?.addEventListener("abort", () => {
            reject(new DOMException("aborted", "AbortError"));
          });
        }),
    );
    const request = clientWith(mockedFetch, {
      timeoutMs: 25,
      maxReadRetries: 3,
    }).get("/v1/resource");
    const assertion = expect(request).rejects.toMatchObject({
      code: "TIMEOUT",
      details: { timeoutMs: 25 },
    });

    await vi.advanceTimersByTimeAsync(25);
    await assertion;
    expect(observedSignal?.aborted).toBe(true);
    expect(mockedFetch).toHaveBeenCalledTimes(1);
    try {
      await request;
    } catch (error) {
      expect((error as HevyCliError).suggestions).toEqual([]);
    }
  });

  it("validates API paths before invoking fetch", async () => {
    const mockedFetch = fetchMock(async () => Promise.resolve(new Response()));
    const client = clientWith(mockedFetch);

    for (const path of [
      "v1/workouts",
      "/v2/workouts",
      "/",
      "https://bad.test/v1/x",
    ]) {
      await expect(client.get(path)).rejects.toMatchObject({
        code: "VALIDATION_ERROR",
      });
    }
    expect(mockedFetch).not.toHaveBeenCalled();
  });
});

describe("HevyClient constructor validation", () => {
  const unusedFetch = fetchMock(async () => Promise.resolve(new Response()));

  it("requires a nonblank API key", () => {
    expect(
      () => new HevyClient({ apiKey: "  ", baseUrl: "https://example.test" }),
    ).toThrowError(expect.objectContaining({ code: "VALIDATION_ERROR" }));
  });

  it.each([
    "not a URL",
    "http://api.example.test",
    "ftp://api.example.test",
    "https://user:pass@example.test",
    "https://example.test?query=yes",
    "https://example.test#fragment",
  ])("rejects malformed or unsafe base URL %s", (baseUrl) => {
    expect(
      () =>
        new HevyClient({
          apiKey: "key",
          baseUrl,
          fetchImpl: unusedFetch,
        }),
    ).toThrowError(expect.objectContaining({ code: "VALIDATION_ERROR" }));
  });

  it.each([
    "https://example.test/",
    "http://localhost:8080",
    "http://127.0.0.1:8080",
    "http://[::1]:8080",
  ])("accepts secure and local base URL %s", (baseUrl) => {
    expect(
      () =>
        new HevyClient({
          apiKey: "key",
          baseUrl,
          fetchImpl: unusedFetch,
        }),
    ).not.toThrow();
  });

  it.each([
    ["timeoutMs", -1],
    ["timeoutMs", 1.5],
    ["timeoutMs", Number.NaN],
    ["maxReadRetries", -1],
    ["maxReadRetries", 1.5],
    ["maxReadRetries", Number.POSITIVE_INFINITY],
  ] as const)("rejects invalid %s value %s", (name, value) => {
    expect(
      () =>
        new HevyClient({
          apiKey: "key",
          baseUrl: "https://example.test",
          fetchImpl: unusedFetch,
          [name]: value,
        }),
    ).toThrowError(expect.objectContaining({ code: "VALIDATION_ERROR" }));
  });
});

describe("error helpers", () => {
  it("constructs structured errors with optional cause and metadata", () => {
    const cause = new Error("root cause");
    const error = new HevyCliError("API_ERROR", "request failed", {
      status: 500,
      suggestions: ["try later"],
      details: { requestId: "abc" },
      cause,
    });

    expect(error).toBeInstanceOf(Error);
    expect(error).toMatchObject({
      name: "HevyCliError",
      message: "request failed",
      code: "API_ERROR",
      status: 500,
      suggestions: ["try later"],
      details: { requestId: "abc" },
      cause,
    });
    expect(isHevyCliError(error)).toBe(true);
    expect(isHevyCliError(new Error("other"))).toBe(false);
    expect(isHevyCliError({ code: "API_ERROR" })).toBe(false);
  });

  it("creates validation errors and selects stable exit codes", () => {
    const validation = validationError("bad argument", ["fix it"]);
    const api = new HevyCliError("API_ERROR", "bad response");

    expect(validation).toMatchObject({
      code: "VALIDATION_ERROR",
      suggestions: ["fix it"],
    });
    expect(exitCodeForHevyError(validation)).toBe(2);
    expect(
      exitCodeForHevyError(new HevyCliError("CONFIG_INSECURE", "unsafe")),
    ).toBe(2);
    expect(exitCodeForHevyError(new HevyCliError("CONFIG_ERROR", "bad"))).toBe(
      1,
    );
    expect(exitCodeForHevyError(api)).toBe(1);
  });
});
