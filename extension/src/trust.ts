// The session's trust surface: what this conversation believes about a peer, and how it asks the
// client what a record is worth.
//
// Two rules decide the shape:
//   - Trusting a peer is a command a person types, never a tool a model calls. A record that asks to
//     be trusted writes nothing.
//   - The verdict is not computed here. The client decides, once, and this module passes records to
//     it — a second implementation of the signature would be a second answer, and two answers is the
//     drift the protocol document forbids.

import { execFileSync } from "node:child_process";

/** What the client concluded about one record. */
export interface Verdict {
	state: "trusted" | "unverified" | "refused";
	principal?: string;
	fingerprint?: string;
	reason?: string;
}

/** What this session holds that the keyring on disk does not. */
export interface SessionTrust {
	/** Anchors recorded for this session only: principal -> fingerprint. Never written anywhere. */
	anchors: Map<string, string>;
	/** Records delivered in this session from senders with no anchor. */
	unverified: number;
}

export function emptySessionTrust(): SessionTrust {
	return { anchors: new Map(), unverified: 0 };
}

/**
 * The session layer, in the form the client reads it.
 *
 * The extension does not reimplement the precedence rule: it hands the client its own layer as
 * `MSGQ_TRUST_SESSION`, which the client ranks below `MSGQ_TRUST` and above the keyring.
 */
export function sessionEnv(trust: SessionTrust): Record<string, string> {
	if (trust.anchors.size === 0) return {};
	const entries = [...trust.anchors.entries()].map(([principal, fingerprint]) => `${principal}:${fingerprint}`);
	return { MSGQ_TRUST_SESSION: entries.join(",") };
}

/** A verdict as one line, for the transcript. */
export function verdictLine(verdict: Verdict): string {
	switch (verdict.state) {
		case "trusted":
			return `verdict: trusted ${verdict.principal ?? "?"} ${short(verdict.fingerprint)}`;
		case "refused":
			return `verdict: REFUSED ${short(verdict.fingerprint)} — ${verdict.reason ?? "no reason given"}`;
		default:
			return verdict.fingerprint
				? `verdict: unverified ${short(verdict.fingerprint)} — ${verdict.reason ?? "no anchor"}`
				: `verdict: unverified — ${verdict.reason ?? "no usable signature"}`;
	}
}

function short(fingerprint: string | undefined): string {
	if (!fingerprint) return "";
	const body = fingerprint.startsWith("SHA256:") ? fingerprint.slice(7) : fingerprint;
	return `SHA256:${body.slice(0, 8)}`;
}

/**
 * Whether a fingerprint is well formed.
 *
 * Checked here as well as by the client because a malformed entry in the session layer would poison
 * every later verification in the conversation, and a typo is the likeliest way to get one.
 */
export function looksLikeFingerprint(raw: string): boolean {
	const body = raw.startsWith("SHA256:") ? raw.slice(7) : raw;
	if (body.length === 64 && /^[0-9a-fA-F]+$/.test(body)) return true;
	return /^[A-Za-z0-9+/]{43}=?$/.test(body);
}

/** What a person asked the trust command to do. */
export type TrustRequest =
	| { action: "list"; json: boolean }
	| { action: "add"; principal: string; fingerprint: string; session: boolean }
	| { action: "revoke"; principal: string; session: boolean }
	| { action: "rotate"; principal: string; fingerprint: string }
	| { action: "error"; message: string };

/**
 * Read the command's arguments.
 *
 * `--session` is the only thing that writes to memory instead of the keyring, and it is a separate
 * word rather than a default because an anchor that dies with the session and an anchor that outlives
 * it are different promises.
 */
export function parseTrustRequest(raw: string): TrustRequest {
	const words = raw.trim().split(/\s+/).filter(Boolean);
	const session = words.includes("--session");
	const rest = words.filter((word) => word !== "--session");
	const json = rest.includes("--json");
	const args = rest.filter((word) => word !== "--json");

	if (args.length === 0) return { action: "list", json };
	const [verb, ...tail] = args;

	switch (verb) {
		case "list":
		case "status":
			return { action: "list", json };
		case "add": {
			if (tail.length !== 2) {
				return { action: "error", message: "usage: /msgq-trust add <principal> <fingerprint>" };
			}
			const [principal, fingerprint] = tail;
			if (!looksLikeFingerprint(fingerprint)) {
				return { action: "error", message: `'${fingerprint}' is not a fingerprint` };
			}
			return { action: "add", principal, fingerprint, session };
		}
		case "revoke":
		case "forget": {
			if (tail.length !== 1) {
				return { action: "error", message: "usage: /msgq-trust revoke <principal>" };
			}
			return { action: "revoke", principal: tail[0], session };
		}
		case "rotate": {
			if (tail.length !== 2) {
				return { action: "error", message: "usage: /msgq-trust rotate <principal> <fingerprint>" };
			}
			const [principal, fingerprint] = tail;
			if (!looksLikeFingerprint(fingerprint)) {
				return { action: "error", message: `'${fingerprint}' is not a fingerprint` };
			}
			return { action: "rotate", principal, fingerprint };
		}
		default:
			return {
				action: "error",
				message: `unknown trust action '${verb}'; try list, add, revoke or rotate`,
			};
	}
}

/** How the surface reaches the client, so a test can answer without spawning one. */
export type Runner = (args: string[], input: string | undefined, env: Record<string, string>) => string;

/** The real runner: one client process, with the session layer in its environment. */
export function clientRunner(bin: string): Runner {
	return (args, input, env) =>
		execFileSync(bin, args, {
			input,
			encoding: "utf8",
			env: { ...process.env, ...env },
			maxBuffer: 8 * 1024 * 1024,
		});
}

/**
 * Ask the client what one record is worth.
 *
 * A client that cannot answer is not a reason to withhold the message: the record is delivered and
 * labelled unverified, which is what the reader would conclude anyway.
 */
export function verifyRecord(
	runner: Runner,
	record: unknown,
	trust: SessionTrust,
): Verdict {
	try {
		const out = runner(["verify", "--json"], `${JSON.stringify(record)}\n`, sessionEnv(trust));
		const line = out.trim().split("\n").filter(Boolean).pop();
		if (!line) return { state: "unverified", reason: "the client said nothing" };
		return JSON.parse(line) as Verdict;
	} catch (error) {
		return { state: "unverified", reason: `the client could not be asked: ${String(error)}` };
	}
}

/** What the trust command shows: the client's listing, then this session's own layer. */
export function formatTrustReport(
	listing: string,
	trust: SessionTrust,
): string {
	const session =
		trust.anchors.size === 0
			? ["session:    none; nothing is trusted for this conversation alone"]
			: [
					"session:    (dies with this conversation)",
					...[...trust.anchors.entries()].map(
						([principal, fingerprint]) => `  ${principal.padEnd(18)} ${short(fingerprint)}`,
					),
				];
	const unverified =
		trust.unverified === 0
			? ["unseen:     every sender this session has read is anchored"]
			: [`unseen:     ${trust.unverified} record(s) came from senders with no anchor`];
	return [listing.trimEnd(), ...session, ...unverified].join("\n");
}
