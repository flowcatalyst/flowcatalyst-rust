#!/usr/bin/env node
/**
 * `flowcatalyst` CLI entry point. Routes to subcommands.
 *
 * Current subcommands:
 *   - `init` — scaffold a new application against a local fc-dev instance.
 *
 * Future subcommands (sync, etc.) plug in here. Keep this file tiny: route,
 * print help, delegate.
 */

import { runInit } from "./init.js";

const HELP = `flowcatalyst — scaffolding + utility CLI for the FlowCatalyst SDK.

USAGE
  flowcatalyst <command> [options]

COMMANDS
  init     Create an application + service account + OAuth client on a
           local fc-dev instance and write credentials to .env.

  help     Show this message.

Run \`flowcatalyst <command> --help\` for command-specific options.
`;

async function main(): Promise<number> {
	const [command, ...rest] = process.argv.slice(2);

	switch (command) {
		case "init":
			return await runInit(rest);
		case undefined:
		case "help":
		case "--help":
		case "-h":
			process.stdout.write(HELP);
			return 0;
		default:
			process.stderr.write(`flowcatalyst: unknown command "${command}"\n\n`);
			process.stderr.write(HELP);
			return 2;
	}
}

main().then(
	(code) => process.exit(code),
	(err) => {
		process.stderr.write(`\n${err instanceof Error ? err.stack ?? err.message : String(err)}\n`);
		process.exit(1);
	},
);
