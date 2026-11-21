# Tasks

## 1. The pump retries instead of dying

- [x] 1.1 Add `countSince(address, offset)` to the `Bus` interface in `extension/src/consumer.ts` and implement it in `HonkerBus` as a count of the stream's offsets above the cursor; verify with a test in `extension/test/consumer.test.ts` that it counts the records after a saved offset and changes no cursor
- [x] 1.2 Turn the pump in `HonkerBus.subscribe` into a state machine that reports transitions (`onState` with `attached` / `retrying` plus a reason) instead of ending on `onError`; verify in `extension/test/consumer.test.ts` that a stream throwing twice then succeeding delivers every record exactly once and reports one `retrying` and one `attached` transition
- [x] 1.3 Keep the local cursor and the persisted offset untouched on the failure path, retrying from the local cursor; verify with a test that the stored offset does not move while reads fail and advances after each successful delivery
- [x] 1.4 Add bounded exponential backoff with jitter (100 ms doubling to a 5 s cap, reset on the first successful read) and a single report after 20 consecutive failures; verify with a fake clock test that the delays grow to the cap and never exceed it
- [x] 1.5 Expose `attachNow()` on `Subscription` so a caller can skip the remaining backoff, serialised against an in-flight attempt; verify with a test that a nudge during a retry attaches once, not twice
- [x] 1.6 Implement `countSince` and the same transition reporting in `extension/src/pg-bus.ts`; verify with `extension/test/pg-bus.test.ts` (or the shared-engine fixture set) that both engines report the same count for the same stream

## 2. The extension reports what it can no longer do

- [x] 2.1 Replace the `onError → disable()` wiring in `extension/msgq.ts` with state handling: a detachment updates status and scheduling, and only a configuration error (`ConfigError`, unknown address scheme) reaches `disable()`; verify in `extension/test/edge.test.ts` that a read failure leaves the extension running and that an unknown scheme still disables it
- [x] 2.2 Emit one `custom_message` (`customType: msgq`, `triggerTurn: false`) per transition, naming the address and the reason, with a suppression counter for repeated failures; queue the entry through the settle hook when a run is in progress; verify in `extension/test/edge.test.ts` that a detachment produces exactly one entry, the recovery one more, and that neither starts a turn
- [x] 2.3 Report the state machine in the status slot — `msgq: attached (N)`, `msgq: detached (retrying)`, `msgq: disabled`; verify with a test that the slot never reports the subscription count while no address is attached
- [x] 2.4 Couple presence to attachment: stop renewing while `retrying`, publish again on the transition to `attached`, and suppress the "presence was lost" warning during a detachment; verify in `extension/test/presence.test.ts` that renewals stop, resume, and produce no duplicate report for one cause
- [x] 2.5 Split `teardown()` into the shutdown path and a per-address `releaseSubscription()`, and narrow `disable()` to configuration errors; verify with a test that a rebuilt subscription leaves the bus handle, the presence clock, and the other subscriptions untouched, and that `teardown()` stays idempotent
- [x] 2.6 Attach on session start whether or not the previous process shut down cleanly, and nudge `attachNow()` for every unattached address in the settle hook; verify in `extension/test/edge.test.ts` that a start with a stale saved offset attaches and that a settle with a detached address attempts an attach before the run ends
- [x] 2.7 Document the attachment states, the transcript records they produce, and the retry policy in `extension/README.md`; verify the documented status strings match the ones the code sets

## 3. A replayed backlog is announced

- [x] 3.1 Count the records waiting with `countSince` before the first delivery after an attach, and emit one transcript line naming the count and the address; verify in `extension/test/edge.test.ts` that a backlog produces the line before the records and that an attach at the head produces nothing
- [x] 3.2 Prove the replay neither duplicates nor drops: a record delivered before a detachment, one delivered after, and one that arrived during the outage; verify none is delivered twice and none is missing

## 4. Recovery holds end to end

- [x] 4.1 Force a read failure against a live store (hold an exclusive transaction from a second handle), confirm the session keeps running, reports the detachment, and recovers by itself once the lock clears; verify the stored offset advanced only after delivery
- [x] 4.2 Kill a session with `kill -9` mid-turn, restart it against the same store, and verify the backlog is delivered as one batch with the replay count in the transcript and no `msgq sessions` gap afterwards
- [x] 4.3 Run `just extension-test`, `just lint`, and `openspec validate msgq-subscription-resilience --strict` and verify all three pass

## Workflow follow-up

- Comment on kata 4prf with the outcome and close it with the verification evidence.
- Archive this change once the extension is released.
