import { readFile } from "node:fs/promises";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import {
  describeChanges,
  extractSwaggerDoc,
} from "../scripts/sync-hevy-openapi.js";

const committed = await readFile(
  join(import.meta.dirname, "..", "docs", "hevy-openapi.json"),
  "utf8",
);

function embed(spec: string): string {
  return `window.onload = function() {\n  var options = {\n  "swaggerDoc": ${spec},\n  "dom_id": "#swagger-ui" };\n};`;
}

describe("extractSwaggerDoc", () => {
  it("returns the committed capture byte-for-byte from a script that embeds it", () => {
    expect(extractSwaggerDoc(embed(committed))).toBe(committed);
  });

  it("does not stop at braces or quotes inside string values", () => {
    const spec = JSON.stringify({
      openapi: "3.0.0",
      info: { description: 'a } brace { and "quotes" \\ backslash' },
      paths: {},
    });
    expect(extractSwaggerDoc(embed(spec))).toBe(spec);
  });

  it("refuses a script without the swaggerDoc object", () => {
    expect(() => extractSwaggerDoc("var nothing = 1;")).toThrow(
      "swaggerDoc not found",
    );
  });

  it("refuses an ambiguous script with two swaggerDoc objects", () => {
    const script = embed(committed) + embed(committed);
    expect(() => extractSwaggerDoc(script)).toThrow("more than once");
  });

  it("refuses an embedded value that is not an OpenAPI document", () => {
    expect(() => extractSwaggerDoc(embed('{"paths": {}}'))).toThrow(
      "not an OpenAPI document",
    );
  });
});

describe("describeChanges", () => {
  it("reports no change for identical documents", () => {
    expect(describeChanges(committed, committed)).toEqual([
      "Only formatting or ordering changed.",
    ]);
  });

  it("names added, removed and changed paths and schemas", () => {
    const before = JSON.parse(committed) as {
      paths: Record<string, unknown>;
      components: { schemas: Record<string, unknown> };
    };
    const firstPath = Object.keys(before.paths)[0] as string;
    const firstSchema = Object.keys(before.components.schemas)[0] as string;
    const after = structuredClone(before);
    delete after.paths[firstPath];
    after.paths["/v1/new"] = { get: {} };
    after.components.schemas[firstSchema] = { type: "string" };
    delete after.components.schemas[
      Object.keys(before.components.schemas)[1] as string
    ];

    const lines = describeChanges(committed, JSON.stringify(after));

    expect(lines).toContain("+ path /v1/new");
    expect(lines).toContain(`- path ${firstPath}`);
    expect(lines).toContain(`~ schema ${firstSchema}`);
    expect(lines.some((line) => line.startsWith("- schema "))).toBe(true);
  });
});
