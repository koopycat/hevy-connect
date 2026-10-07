import { basename } from "node:path";

import { runAxiCli } from "axi-sdk-js";

import { HevyClient } from "./client.js";
import {
  commandHelp,
  createCommands,
  formatCliError,
  homeCommand,
  TOP_LEVEL_HELP,
  type OutputFormat,
} from "./commands.js";
import { requireApiKey } from "./config.js";
import { validationError } from "./errors.js";
import { VERSION } from "./version.js";

const DESCRIPTION = "Agent-ergonomic access to the Hevy Public API.";

function requestedFormat(argv: readonly string[]): OutputFormat {
  if (argv.includes("--json") || argv.includes("--format=json")) {
    return "json";
  }
  const formatIndex = argv.lastIndexOf("--format");
  return formatIndex >= 0 && argv[formatIndex + 1] === "json" ? "json" : "toon";
}

export async function main(argv = process.argv.slice(2)): Promise<void> {
  const dependencies = {
    clientFactory: (config: Parameters<typeof requireApiKey>[0]) =>
      new HevyClient({
        apiKey: requireApiKey(config),
        baseUrl: config.baseUrl,
      }),
    ...(process.argv[1] === undefined ? {} : { execPath: process.argv[1] }),
  };
  const format = requestedFormat(argv);

  await runAxiCli({
    argv,
    description: DESCRIPTION,
    version: VERSION,
    packageName: "hevy-axi",
    topLevelHelp: TOP_LEVEL_HELP,
    commands: createCommands(dependencies),
    home: homeCommand(dependencies),
    getCommandHelp: commandHelp,
    formatError: (error) => {
      const formatted = formatCliError(error, format);
      return {
        output: `${formatted.output}\n`,
        exitCode: formatted.exitCode,
      };
    },
    renderUnknownCommand: (command) => {
      const formatted = formatCliError(
        validationError(`Unknown command: ${command}.`, [
          `Run \`${basename(process.argv[1] ?? "hevy-axi")} --help\` to see available commands.`,
        ]),
        format,
      );
      return `${formatted.output}\n`;
    },
  });
}
