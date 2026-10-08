/**
 * Generates `src/generated/hevy-api.ts` from the verbatim Hevy OpenAPI capture.
 *
 * The capture is malformed OpenAPI 3.0 in a few places that strict generators
 * reject (see docs/hevy-api-analysis.md, "Schema oddities"). Those defects are
 * repaired here, in memory, with each target asserted so that a changed upstream
 * document fails loudly instead of silently generating something else. The
 * committed JSON is never modified.
 *
 * Usage:
 *   tsx scripts/generate-api-types.ts           write the generated module
 *   tsx scripts/generate-api-types.ts --check   exit 1 if the module is stale
 */
import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

import openapiTS, { astToString, type OpenAPI3 } from "openapi-typescript";

export const SPEC_PATH = fileURLToPath(
  new URL("../docs/hevy-openapi.json", import.meta.url),
);
export const OUTPUT_PATH = fileURLToPath(
  new URL("../src/generated/hevy-api.ts", import.meta.url),
);

const HEADER = `// Code generated from docs/hevy-openapi.json by scripts/generate-api-types.ts. DO NOT EDIT.
// Regenerate with \`just api-types\`.
`;

type Schema = Record<string, unknown>;
interface OpenApiDocument {
  paths: Record<string, Record<string, { parameters?: Schema[] }>>;
  components: { schemas: Record<string, Schema> };
}

function requireObject(value: unknown, what: string): Schema {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`Spec defect repair target missing: ${what}`);
  }
  return value as Schema;
}

/** Repairs the documented defects so openapi-typescript accepts the spec. */
export function normalizeSpec(verbatim: OpenApiDocument): OpenApiDocument {
  const doc = structuredClone(verbatim);
  const schemas = doc.components.schemas;

  // Defect: `type: enum` is not an OpenAPI 3.0 primitive. Keep the enum values.
  for (const name of [
    "CustomExerciseType",
    "MuscleGroup",
    "EquipmentCategory",
  ]) {
    const schema = requireObject(schemas[name], `schema ${name}`);
    if (schema.type !== "enum") {
      throw new Error(`Expected ${name}.type to be "enum" in the capture.`);
    }
    schema.type = "string";
  }

  // Defect: `required: true` on a property. It must be listed on the parent.
  const envelope = requireObject(
    schemas.CreateCustomExerciseRequestBody,
    "schema CreateCustomExerciseRequestBody",
  );
  const properties = requireObject(envelope.properties, "exercise properties");
  const exercise = requireObject(properties.exercise, "property exercise");
  if (exercise.required !== true) {
    throw new Error("Expected exercise.required to be true in the capture.");
  }
  delete exercise.required;
  envelope.required = ["exercise"];

  // Defect: path parameters without a schema. Types follow the component
  // schemas: folder IDs are numbers, every other ID is a string.
  const pathParamTypes: Record<string, string> = {
    workoutId: "string",
    routineId: "string",
    exerciseTemplateId: "string",
    folderId: "number",
  };
  for (const item of Object.values(doc.paths)) {
    for (const operation of Object.values(item)) {
      for (const parameter of operation.parameters ?? []) {
        if (parameter.in !== "path" || parameter.schema !== undefined) continue;
        const type = pathParamTypes[String(parameter.name)];
        if (type === undefined) {
          throw new Error(
            `No type configured for path parameter ${String(parameter.name)}.`,
          );
        }
        parameter.schema = { type };
      }
    }
  }

  return doc;
}

/** Generates the module from verbatim spec text, without touching the disk. */
export async function generateApiTypesFrom(specText: string): Promise<string> {
  const verbatim = JSON.parse(specText) as OpenApiDocument;
  // The local type only models the fields the repairs touch, so it does not
  // overlap OpenAPI3 structurally. The repairs assert every shape they rely on.
  const repaired = normalizeSpec(verbatim) as unknown as OpenAPI3;
  const ast = await openapiTS(repaired, {
    alphabetize: true,
    immutable: false,
  });
  return HEADER + astToString(ast);
}

export async function generateApiTypes(): Promise<string> {
  return generateApiTypesFrom(await readFile(SPEC_PATH, "utf8"));
}

/** Regenerates `src/generated/hevy-api.ts` from the committed capture. */
export async function writeApiTypes(): Promise<void> {
  await writeFile(OUTPUT_PATH, await generateApiTypes());
}

async function main(): Promise<void> {
  const generated = await generateApiTypes();
  if (process.argv.includes("--check")) {
    const current = await readFile(OUTPUT_PATH, "utf8").catch(() => "");
    if (current !== generated) {
      console.error(
        "src/generated/hevy-api.ts is stale. Run `just api-types`.",
      );
      process.exitCode = 1;
    } else {
      console.log("src/generated/hevy-api.ts is up to date.");
    }
    return;
  }
  await writeApiTypes();
  console.log("Wrote src/generated/hevy-api.ts");
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  await main();
}
