import assert from "node:assert/strict";
import { test } from "node:test";

import { isState, PRESENCE_STATES } from "../src/presence-text.ts";

test("the three states are the ones a record may carry", () => {
	assert.deepEqual(PRESENCE_STATES, ["idle", "busy", "blocked"]);
	for (const state of PRESENCE_STATES) assert.equal(isState(state), true);
});

test("anything else is not a state", () => {
	assert.equal(isState("gone"), false, "gone retracts presence, it is not a state a session holds");
	assert.equal(isState("IDLE"), false);
	assert.equal(isState(""), false);
	assert.equal(isState("busy "), false);
});
