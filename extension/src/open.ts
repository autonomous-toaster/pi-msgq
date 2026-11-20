// Opening a record through the client.
//
// The encoding is decided in one place. A record sealed to recipients needs X25519, HKDF and the
// envelope rules; implementing that here as well would be a second answer to the same question, and
// the two would drift the first time either changed. So the extension asks, exactly as it asks for a
// verdict, and keeps no cryptography of its own.

/** What the client concluded about a sealed record. */
export type OpenOutcome =
	| { kind: "opened"; record: Record<string, unknown> }
	| { kind: "elsewhere"; sealedTo: string[] }
	| { kind: "cannot"; alg: string; reason: string };

/** How the opener reaches the client, so a test can answer without spawning one. */
export type OpenRunner = (record: unknown) => string;

/** Whether this record declares an encoding at all. */
export function isSealed(record: unknown): boolean {
	if (record === null || typeof record !== "object") return false;
	const meta = (record as { meta?: unknown }).meta;
	if (meta === null || typeof meta !== "object") return false;
	return (meta as { enc?: unknown }).enc !== undefined;
}

/** Read one outcome line from the client. */
export function parseOpenOutcome(line: string): OpenOutcome {
	let parsed: {
		opened?: unknown;
		sealed_to?: unknown;
		cannot_open?: { alg?: unknown; reason?: unknown };
	};
	try {
		parsed = JSON.parse(line);
	} catch {
		return { kind: "cannot", alg: "", reason: "the client's answer was not a record" };
	}

	if (parsed.opened !== undefined && parsed.opened !== null && typeof parsed.opened === "object") {
		return { kind: "opened", record: parsed.opened as Record<string, unknown> };
	}
	if (Array.isArray(parsed.sealed_to)) {
		return {
			kind: "elsewhere",
			sealedTo: parsed.sealed_to.filter((entry): entry is string => typeof entry === "string"),
		};
	}
	const alg = typeof parsed.cannot_open?.alg === "string" ? parsed.cannot_open.alg : "";
	const reason =
		typeof parsed.cannot_open?.reason === "string"
			? parsed.cannot_open.reason
			: "the client could not open it and did not say why";
	return { kind: "cannot", alg, reason };
}

/** Ask the client to open one record. */
export function openWithClient(run: OpenRunner, record: unknown): OpenOutcome {
	// A plaintext record is already what it says it is, and costs no process.
	if (!isSealed(record)) {
		return { kind: "opened", record: record as Record<string, unknown> };
	}
	try {
		const line = run(record)
			.split("\n")
			.map((entry) => entry.trim())
			.filter(Boolean)
			.pop();
		if (line === undefined) {
			return { kind: "cannot", alg: "", reason: "the client said nothing" };
		}
		return parseOpenOutcome(line);
	} catch (error) {
		return { kind: "cannot", alg: "", reason: `the client could not be asked: ${String(error)}` };
	}
}
