// Publishing through the client.
//
// The extension writes two kinds of record of its own — presence and acknowledgements — and nothing
// else. Both go through `msgq publish`, so every record this session writes carries a signature from
// the same key as the messages it sends: a reader never has to special-case the bus's own progress,
// and a forged presence or acknowledgement is refused like anything else.
//
// The cost is a short-lived process per record. Presence renews on a timer and an acknowledgement is
// two or three per delivered message, so it is bounded by the same rate as the conversation.

import { execFileSync } from "node:child_process";

/** How the publisher reaches the client, so a test can answer without spawning one. */
export type PublishRunner = (args: string[]) => string;

/** The real runner: one client process per record. */
export function clientPublishRunner(
	bin: string,
	options: { db?: string; env?: Record<string, string | undefined> } = {},
): PublishRunner {
	const base = options.db ? ["--db", options.db] : [];
	return (args) =>
		execFileSync(bin, [...base, ...args], {
			encoding: "utf8",
			env: { ...process.env, ...options.env },
			maxBuffer: 8 * 1024 * 1024,
		});
}

/** What a caller needs from a publisher: an address and a record. */
export interface Publisher {
	publish(address: string, record: unknown): void;
}

/**
 * A publisher that signs what it writes.
 *
 * `--json` on publish means "this argument is a whole record", which is how a record built here keeps
 * its exact shape and picks up a signature on the way out.
 */
export function clientPublisher(run: PublishRunner): Publisher {
	return {
		publish(address: string, record: unknown): void {
			run(["publish", address, "--json", JSON.stringify(record)]);
		},
	};
}
