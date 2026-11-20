import assert from "node:assert/strict";
import { test } from "node:test";

import { clientPublisher, clientPublishRunner } from "../src/publish.ts";
import { buildAck } from "../src/ack.ts";

test("a record this session writes leaves through the client, whole", () => {
	// Presence and acknowledgements are records like any other: they are signed by the same key as the
	// messages, which is why nothing on the read side needs an exemption for them.
	const seen: string[][] = [];
	const publish = clientPublisher((args) => {
		seen.push(args);
		return "1\n";
	});

	const ack = buildAck({ from: "session/a", to: "session/b", replyTo: 7, phase: "received" });
	publish.publish("session/b", ack);

	assert.equal(seen.length, 1);
	const [verb, address, flag, payload] = seen[0];
	assert.equal(verb, "publish");
	assert.equal(address, "session/b");
	// --json is what says "this argument is a whole record", so the shape survives the round trip.
	assert.equal(flag, "--json");
	assert.deepEqual(JSON.parse(payload), ack);
});

test("the publisher talks to the bus the session is on", () => {
	const echoed = clientPublishRunner("/bin/echo", { db: "/tmp/some-bus.db" })([
		"publish",
		"group/a",
		"--json",
		'{"v":1}',
	]);
	assert.match(echoed, /^--db \/tmp\/some-bus\.db publish group\/a --json /);
});

test("a failing client is an error the caller can act on", () => {
	const publish = clientPublisher(() => {
		throw new Error("msgq: the credential is not recognised");
	});
	assert.throws(() => publish.publish("group/a", { v: 1 }), /not recognised/);
});
