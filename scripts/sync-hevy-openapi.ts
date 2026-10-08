/**
 * Refreshes `docs/hevy-openapi.json` from Hevy's public Swagger UI and
 * regenerates `src/generated/hevy-api.ts`.
 *
 * The spec is embedded in `swagger-ui-init.js` as the `swaggerDoc` object. This
 * script copies that object byte-for-byte, so the committed file stays a verbatim
 * capture. Only the public script is requested; no API key is sent.
 *
 * Usage:
 *   tsx scripts/sync-hevy-openapi.ts           write the capture and types if changed
 *   tsx scripts/sync-hevy-openapi.ts --check   report drift; exit 1 if changed, write nothing
 *
 * This is deliberately not part of `pnpm check` or `pnpm build`: it needs the
 * network, and a change in Hevy's contract should be reviewed, not absorbed.
 */
import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

import {
  generateApiTypesFrom,
  SPEC_PATH,
  OUTPUT_PATH,
} from "./generate-api-types.js";

export const SOURCE_URL = "https://api.hevyapp.com/docs/swagger-ui-init.js";
const FETCH_TIMEOUT_MS = 20_000;
const MARKER = '"swaggerDoc": ';

type SpecObject = {
  openapi?: unknown;
  paths?: Record<string, Record<string, unknown>>;
  components?: { schemas?: Record<string, unknown> };
};

/** Index of the brace closing the object that opens at `open`, skipping strings. */
function matchingBrace(source: string, open: number): number {
  let depth = 0;
  let inString = false;
  for (let index = open; index < source.length; index += 1) {
    const char = source[index];
    if (inString) {
      if (char === "\\") index += 1;
      else if (char === '"') inString = false;
    } else if (char === '"') {
      inString = true;
    } else if (char === "{") {
      depth += 1;
    } else if (char === "}") {
      depth -= 1;
      if (depth === 0) return index;
    }
  }
  throw new Error("Unbalanced braces in swaggerDoc object.");
}

/** Returns the `swaggerDoc` object text exactly as the script embeds it. */
export function extractSwaggerDoc(script: string): string {
  const first = script.indexOf(MARKER);
  if (first === -1) {
    throw new Error("swaggerDoc not found in the Swagger UI init script.");
  }
  if (script.indexOf(MARKER, first + 1) !== -1) {
    throw new Error("swaggerDoc appears more than once; refusing to guess.");
  }
  const start = first + MARKER.length;
  if (script[start] !== "{") {
    throw new Error("swaggerDoc is not an object literal.");
  }
  const text = script.slice(start, matchingBrace(script, start) + 1);
  const parsed = JSON.parse(text) as SpecObject;
  if (typeof parsed.openapi !== "string" || typeof parsed.paths !== "object") {
    throw new Error("swaggerDoc is not an OpenAPI document.");
  }
  return text;
}

async function fetchLiveScript(): Promise<string> {
  const response = await fetch(SOURCE_URL, {
    redirect: "error",
    signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
  });
  if (!response.ok) {
    throw new Error(
      `Fetching ${SOURCE_URL} failed with HTTP ${response.status}.`,
    );
  }
  return response.text();
}

/** Human-readable summary of how the contract changed. */
export function describeChanges(committed: string, live: string): string[] {
  const before = JSON.parse(committed) as SpecObject;
  const after = JSON.parse(live) as SpecObject;
  const lines: string[] = [];
  const beforePaths = before.paths ?? {};
  const afterPaths = after.paths ?? {};
  for (const path of Object.keys(afterPaths)) {
    if (!(path in beforePaths)) lines.push(`+ path ${path}`);
  }
  for (const path of Object.keys(beforePaths)) {
    if (!(path in afterPaths)) lines.push(`- path ${path}`);
    else if (
      JSON.stringify(beforePaths[path]) !== JSON.stringify(afterPaths[path])
    ) {
      lines.push(`~ path ${path}`);
    }
  }
  const beforeSchemas = before.components?.schemas ?? {};
  const afterSchemas = after.components?.schemas ?? {};
  for (const name of Object.keys(afterSchemas)) {
    if (!(name in beforeSchemas)) lines.push(`+ schema ${name}`);
  }
  for (const name of Object.keys(beforeSchemas)) {
    if (!(name in afterSchemas)) lines.push(`- schema ${name}`);
    else if (
      JSON.stringify(beforeSchemas[name]) !== JSON.stringify(afterSchemas[name])
    ) {
      lines.push(`~ schema ${name}`);
    }
  }
  if (lines.length === 0) lines.push("Only formatting or ordering changed.");
  return lines;
}

function sha256(text: string): string {
  return createHash("sha256").update(text).digest("hex");
}

async function main(): Promise<void> {
  const checkOnly = process.argv.includes("--check");
  const live = extractSwaggerDoc(await fetchLiveScript());
  const committed = await readFile(SPEC_PATH, "utf8");

  if (live === committed) {
    console.log(
      `docs/hevy-openapi.json matches the live docs (sha256 ${sha256(live)}).`,
    );
    return;
  }

  console.log(
    `Live contract differs from the committed capture (sha256 ${sha256(live)}):`,
  );
  for (const line of describeChanges(committed, live)) console.log(`  ${line}`);

  // Generate before writing anything, so a spec the repairs cannot handle leaves
  // the committed files untouched.
  const types = await generateApiTypesFrom(live);
  if (checkOnly) {
    process.exitCode = 1;
    console.error(
      "Run `just api-sync` to adopt the change, then review docs/hevy-api-analysis.md.",
    );
    return;
  }
  await writeFile(SPEC_PATH, live);
  await writeFile(OUTPUT_PATH, types);
  console.log("Wrote docs/hevy-openapi.json and src/generated/hevy-api.ts.");
  console.log(
    "Next: update the SHA and capture date in docs/hevy-api-analysis.md and AGENTS.md, then run `just check`.",
  );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  await main();
}
