/**
 * `flowcatalyst init` — scaffold a new application on a local fc-dev.
 *
 * Steps:
 *   1. Parse CLI flags + resolve base URL.
 *   2. Localhost guard (`--allow-remote` to override).
 *   3. Log in to fc-dev as an anchor admin (default: the fc-dev bootstrap
 *      admin `admin@flowcatalyst.local` / `DevPassword123!`).
 *   4. Create a Default Client (skipped if `--client-identifier` resolves
 *      to an existing one).
 *   5. Create an Application with the prompted code/name/type/etc.
 *   6. Provision a service account on it — the platform's
 *      `provision-service-account` endpoint mints a CONFIDENTIAL OAuth
 *      client with `client_credentials` grant in the same tx and returns
 *      the plaintext `clientSecret` exactly once.
 *   7. Write the resulting credentials to `./.env` (interactive merge if
 *      the file already has FLOWCATALYST_* keys).
 *
 * Zero new package deps — interactive prompts use `node:readline/promises`,
 * HTTP uses the built-in `fetch`, file IO uses `node:fs/promises`.
 */

import { createInterface, type Interface } from "node:readline/promises";
import { readFile, writeFile } from "node:fs/promises";
import { resolve as resolvePath } from "node:path";
import { stdin as input, stdout as output } from "node:process";

const LOCAL_HOSTS = new Set(["localhost", "127.0.0.1", "::1", "0.0.0.0"]);
const DEFAULT_BASE_URL = "http://localhost:8080";
const DEFAULT_ADMIN_EMAIL = "admin@flowcatalyst.local";
const DEFAULT_ADMIN_PASSWORD = "DevPassword123!";
const DEFAULT_CLIENT_IDENTIFIER = "default";
const DEFAULT_CLIENT_NAME = "Default Client";
const ENV_PATH = ".env";

interface Args {
	baseUrl: string;
	allowRemote: boolean;
	email?: string;
	password?: string;
	code?: string;
	name?: string;
	type?: "APPLICATION" | "INTEGRATION";
	description?: string;
	defaultBaseUrl?: string;
	clientIdentifier: string;
	clientName: string;
	yes: boolean;
	help: boolean;
}

interface CookieJar {
	cookies: Map<string, string>;
}

interface ServiceAccountCredentials {
	principalId: string;
	name: string;
	oauthClient: {
		id: string;
		clientId: string;
		clientSecret: string;
	};
}

const HELP = `flowcatalyst init — scaffold a new application on fc-dev.

USAGE
  flowcatalyst init [options]

OPTIONS
  --base-url <url>           Platform base URL (env FLOWCATALYST_BASE_URL,
                             default ${DEFAULT_BASE_URL})
  --allow-remote             Permit non-localhost base URLs. Off by default
                             — init is intended for local fc-dev only.

  --email <addr>             Anchor-admin email to log in with
                             (default ${DEFAULT_ADMIN_EMAIL}).
  --password <pw>            Password (prompts if omitted). The fc-dev
                             default is "${DEFAULT_ADMIN_PASSWORD}".

  --code <slug>              Application code (URL-safe).
  --name <name>              Human-readable application name.
  --type <APPLICATION|INTEGRATION>   Defaults to APPLICATION.
  --description <text>       Optional description.
  --default-base-url <url>   The application's own deployed base URL
                             (filled into Application.defaultBaseUrl).

  --client-identifier <id>   Default client identifier (default "${DEFAULT_CLIENT_IDENTIFIER}").
  --client-name <name>       Default client name (default "${DEFAULT_CLIENT_NAME}").

  --yes                      Non-interactive — fail if any required value
                             is missing rather than prompting.
  --help                     Show this message.

ENV
  FLOWCATALYST_BASE_URL      Same as --base-url.

ON SUCCESS
  Writes / updates ./.env with:
    FLOWCATALYST_BASE_URL
    FLOWCATALYST_APP_CODE
    FLOWCATALYST_CLIENT_ID         (the OAuth client's public id)
    FLOWCATALYST_CLIENT_SECRET     (plaintext; only returned once)

Tip: the fc-dev bootstrap admin is created on first boot from
FLOWCATALYST_BOOTSTRAP_ADMIN_EMAIL/_PASSWORD env vars on fc-dev. With
zero config fc-dev seeds admin@flowcatalyst.local / DevPassword123!.
`;

export async function runInit(argv: string[]): Promise<number> {
	const args = parseArgs(argv);
	if (args.help) {
		process.stdout.write(HELP);
		return 0;
	}

	// 1. Localhost guard.
	const url = new URL(args.baseUrl);
	if (!LOCAL_HOSTS.has(url.hostname) && !args.allowRemote) {
		process.stderr.write(
			`\nflowcatalyst init refuses to run against ${args.baseUrl} — \n` +
				`hostname "${url.hostname}" is not local. Pass --allow-remote to override\n` +
				`if you really want to scaffold against this environment.\n\n`,
		);
		return 2;
	}

	const rl = args.yes ? null : createInterface({ input, output });

	try {
		console.log(`flowcatalyst init → ${args.baseUrl}\n`);

		// 2. Resolve all the prompts up-front so the user sees the full
		// picture before any platform calls happen.
		const email =
			args.email ??
			(await prompt(rl, `Admin email`, DEFAULT_ADMIN_EMAIL));
		const password =
			args.password ?? (await promptPassword(rl, `Admin password`));
		const code =
			args.code ??
			(await prompt(rl, `Application code (slug, e.g. "orders")`));
		const name =
			args.name ?? (await prompt(rl, `Application name (e.g. "Orders")`));
		const type =
			args.type ??
			((await prompt(
				rl,
				`Application type [APPLICATION|INTEGRATION]`,
				"APPLICATION",
			)) as "APPLICATION" | "INTEGRATION");
		const description =
			args.description ?? (await prompt(rl, `Description (optional)`, ""));
		const defaultBaseUrl =
			args.defaultBaseUrl ??
			(await prompt(rl, `Application's deployed base URL (optional)`, ""));

		// 3. Login to capture a session cookie.
		const jar: CookieJar = { cookies: new Map() };
		console.log(`\n→ Logging in as ${email}...`);
		await api(args.baseUrl, jar, "POST", "/auth/login", { email, password });
		console.log(`  ok`);

		// 4. Create the Default Client (idempotent — skip if it already exists).
		const clientId = await ensureClient(args, jar);

		// 5. Create the Application.
		console.log(`→ Creating application "${code}"...`);
		const appCreate = (await api(args.baseUrl, jar, "POST", "/api/applications", {
			code,
			name,
			type,
			description: description || undefined,
			defaultBaseUrl: defaultBaseUrl || undefined,
		})) as { id: string };
		console.log(`  created app id=${appCreate.id}`);

		// 6. Provision SA + OAuth client (PR #6 flow — single PG tx, returns
		// the plaintext clientSecret exactly once).
		console.log(`→ Provisioning service account + OAuth client...`);
		const provision = (await api(
			args.baseUrl,
			jar,
			"POST",
			`/api/applications/${appCreate.id}/provision-service-account`,
			{},
		)) as { serviceAccount: ServiceAccountCredentials };
		const sa = provision.serviceAccount;
		console.log(`  ok — principal=${sa.principalId}`);

		// 7. Write .env.
		await writeEnvUpdates(rl, args.yes, {
			FLOWCATALYST_BASE_URL: args.baseUrl,
			FLOWCATALYST_APP_CODE: code,
			FLOWCATALYST_CLIENT_ID: sa.oauthClient.clientId,
			FLOWCATALYST_CLIENT_SECRET: sa.oauthClient.clientSecret,
		});

		console.log(`
✓ Application scaffolded.

  Application:     ${name} (code=${code})
  Service account: ${sa.principalId}
  OAuth client:    ${sa.oauthClient.clientId}
  Default client:  ${clientId}

  Credentials written to ${ENV_PATH}. The clientSecret is shown ONLY in
  the .env — the platform stores only the encrypted form and cannot
  return it again. Rotate via the OAuth Clients page if needed.
`);
		return 0;
	} finally {
		rl?.close();
	}
}

// ─── Helpers ────────────────────────────────────────────────────────────────

function parseArgs(argv: string[]): Args {
	const args: Args = {
		baseUrl: process.env["FLOWCATALYST_BASE_URL"] ?? DEFAULT_BASE_URL,
		allowRemote: false,
		clientIdentifier: DEFAULT_CLIENT_IDENTIFIER,
		clientName: DEFAULT_CLIENT_NAME,
		yes: false,
		help: false,
	};

	const take = (i: number): string => {
		const v = argv[i + 1];
		if (v === undefined)
			throw new Error(`flag ${argv[i]} requires a value`);
		return v;
	};

	for (let i = 0; i < argv.length; i++) {
		const a = argv[i];
		switch (a) {
			case "--help":
			case "-h":
				args.help = true;
				break;
			case "--allow-remote":
				args.allowRemote = true;
				break;
			case "--yes":
			case "-y":
				args.yes = true;
				break;
			case "--base-url":
				args.baseUrl = take(i++);
				break;
			case "--email":
				args.email = take(i++);
				break;
			case "--password":
				args.password = take(i++);
				break;
			case "--code":
				args.code = take(i++);
				break;
			case "--name":
				args.name = take(i++);
				break;
			case "--type": {
				const v = take(i++).toUpperCase();
				if (v !== "APPLICATION" && v !== "INTEGRATION") {
					throw new Error(`--type must be APPLICATION or INTEGRATION (got ${v})`);
				}
				args.type = v;
				break;
			}
			case "--description":
				args.description = take(i++);
				break;
			case "--default-base-url":
				args.defaultBaseUrl = take(i++);
				break;
			case "--client-identifier":
				args.clientIdentifier = take(i++);
				break;
			case "--client-name":
				args.clientName = take(i++);
				break;
			default:
				if (a?.startsWith("--")) throw new Error(`unknown flag ${a}`);
		}
	}
	return args;
}

async function prompt(
	rl: Interface | null,
	question: string,
	def?: string,
): Promise<string> {
	if (rl === null) {
		if (def !== undefined) return def;
		throw new Error(
			`${question} required (in --yes mode every value must come from a flag or env)`,
		);
	}
	const suffix = def !== undefined ? ` [${def}]` : "";
	const answer = (await rl.question(`${question}${suffix}: `)).trim();
	return answer.length === 0 && def !== undefined ? def : answer;
}

async function promptPassword(rl: Interface | null, question: string): Promise<string> {
	if (rl === null) {
		throw new Error(`${question} required (pass --password in --yes mode)`);
	}
	// Best-effort: turn off echo for the duration of the password prompt.
	// readline doesn't expose a clean masking API, so we toggle the raw mode
	// manually and write \n at the end. If the TTY isn't a TTY (piped input),
	// we just read the line normally.
	if (!input.isTTY) {
		return prompt(rl, question);
	}
	process.stdout.write(`${question}: `);
	const original = input.isRaw ?? false;
	input.setRawMode?.(true);
	let pw = "";
	for await (const chunk of input) {
		const s = chunk.toString("utf8");
		for (const ch of s) {
			if (ch === "\n" || ch === "\r") {
				input.setRawMode?.(original);
				process.stdout.write("\n");
				return pw;
			}
			if (ch === "") {
				input.setRawMode?.(original);
				process.stdout.write("\n");
				throw new Error("aborted");
			}
			if (ch === "" || ch === "\b") {
				if (pw.length > 0) pw = pw.slice(0, -1);
			} else if (ch >= " ") {
				pw += ch;
			}
		}
	}
	input.setRawMode?.(original);
	process.stdout.write("\n");
	return pw;
}

async function api(
	baseUrl: string,
	jar: CookieJar,
	method: string,
	path: string,
	body: unknown,
): Promise<unknown> {
	const url = new URL(path, baseUrl).toString();
	const headers: Record<string, string> = {
		"content-type": "application/json",
	};
	if (jar.cookies.size > 0) {
		headers["cookie"] = [...jar.cookies.entries()]
			.map(([k, v]) => `${k}=${v}`)
			.join("; ");
	}
	const res = await fetch(url, {
		method,
		headers,
		body: method === "GET" ? undefined : JSON.stringify(body),
	});
	// Capture Set-Cookie. Browsers' fetch hides this; Node's fetch via undici
	// exposes headers.getSetCookie() (Node ≥18.14).
	const setCookies = (res.headers as Headers & {
		getSetCookie?: () => string[];
	}).getSetCookie?.();
	if (setCookies) {
		for (const sc of setCookies) {
			const m = sc.match(/^([^=;]+)=([^;]*)/);
			if (m && m[1] !== undefined && m[2] !== undefined) {
				jar.cookies.set(m[1], m[2]);
			}
		}
	}
	if (!res.ok) {
		let detail = "";
		try {
			detail = await res.text();
		} catch {
			/* ignore */
		}
		throw new Error(
			`${method} ${path} → ${res.status} ${res.statusText}${detail ? `\n  ${detail}` : ""}`,
		);
	}
	if (res.status === 204) return null;
	const ct = res.headers.get("content-type") ?? "";
	if (ct.includes("application/json")) return await res.json();
	return await res.text();
}

async function ensureClient(args: Args, jar: CookieJar): Promise<string> {
	// Check whether a client with this identifier already exists. If yes,
	// reuse it (idempotent re-run); if no, create.
	console.log(`→ Resolving default client "${args.clientIdentifier}"...`);
	try {
		const existing = (await api(
			args.baseUrl,
			jar,
			"GET",
			`/api/clients/by-identifier/${encodeURIComponent(args.clientIdentifier)}`,
			null,
		)) as { id: string };
		console.log(`  reusing existing client id=${existing.id}`);
		return existing.id;
	} catch (e) {
		// 404 means we need to create it. Anything else is fatal.
		if (!(e instanceof Error) || !e.message.includes("404")) throw e;
	}
	const created = (await api(args.baseUrl, jar, "POST", "/api/clients", {
		identifier: args.clientIdentifier,
		name: args.clientName,
	})) as { id: string };
	console.log(`  created client id=${created.id}`);
	return created.id;
}

async function writeEnvUpdates(
	rl: Interface | null,
	yes: boolean,
	updates: Record<string, string>,
): Promise<void> {
	const path = resolvePath(process.cwd(), ENV_PATH);
	let original = "";
	try {
		original = await readFile(path, "utf8");
	} catch {
		// no existing .env — fine, we'll create one
	}

	const lines = original === "" ? [] : original.split(/\r?\n/);
	const seen = new Set<string>();

	for (let i = 0; i < lines.length; i++) {
		const m = lines[i]?.match(/^\s*([A-Z_][A-Z0-9_]*)\s*=/);
		const key = m?.[1];
		if (key !== undefined && updates[key] !== undefined) {
			lines[i] = `${key}=${quoteEnvValue(updates[key])}`;
			seen.add(key);
		}
	}

	const toAppend = Object.entries(updates).filter(([k]) => !seen.has(k));
	if (toAppend.length > 0) {
		if (lines.length > 0 && lines[lines.length - 1] !== "") lines.push("");
		lines.push("# FlowCatalyst (added by `flowcatalyst init`)");
		for (const [k, v] of toAppend) {
			lines.push(`${k}=${quoteEnvValue(v)}`);
		}
	}

	const next = lines.join("\n").replace(/\n*$/, "\n");
	if (next === original) {
		console.log(`→ ${ENV_PATH} already has these values, no update needed.`);
		return;
	}

	if (original !== "" && !yes && rl !== null) {
		const ok = await prompt(rl, `Update ${ENV_PATH}? [y/N]`, "n");
		if (!/^y(es)?$/i.test(ok)) {
			console.log(`✗ skipped writing ${ENV_PATH}; credentials below:`);
			for (const [k, v] of Object.entries(updates)) {
				console.log(`    ${k}=${quoteEnvValue(v)}`);
			}
			return;
		}
	}

	await writeFile(path, next, "utf8");
	console.log(`→ ${ENV_PATH} ${original === "" ? "created" : "updated"}.`);
}

function quoteEnvValue(value: string): string {
	// Single-quote anything with whitespace, '#', or empty values. Base64
	// secrets are URL-safe so they're fine bare, but quote for robustness
	// against future value shapes.
	if (value === "" || /[\s#'"`$]/.test(value)) {
		return `'${value.replace(/'/g, "'\\''")}'`;
	}
	return value;
}
