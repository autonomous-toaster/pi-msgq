## 1. Extension — lease primitives

- [x] 1.1 Add a presence module that builds and parses the lease name (`msgq:participant:<kind>:<state>:<id>`) and derives `session/<alias-or-id>` from an owner; verify unit tests for build/parse round-trip, an aliased and an unaliased owner, and rejection of a malformed name.
- [x] 1.2 Wrap the engine lease calls (acquire, renew, release) in that module; verify with two handles on a temporary bus that acquiring makes the lease visible, renewing extends its expiry, and releasing removes it.

## 2. Extension — lifecycle

- [x] 2.1 Register presence on session start for every reason and release it inside the existing `teardown()`; verify that a start creates a lease, a shutdown removes it, and a second shutdown is a no-op.
- [x] 2.2 Add the presence opt-out (`--msgq-presence` / `MSGQ_PRESENCE`), defaulting to on; verify a disabled run creates no lease and still subscribes.
- [x] 2.3 Start the renewal interval in session start, clear it in `teardown()`, and unref it; verify no timer survives shutdown and a print-mode process still exits.

## 3. Extension — state

- [x] 3.1 Derive the initial state from `isIdle()` and add `agent_start` / `agent_settled` handlers for busy and idle; verify a running session reports busy during a turn and idle after it settles.
- [x] 3.2 Map `ui_prompt_start` / `ui_prompt_end` to blocked; verify the state becomes blocked while a blocking prompt is open and returns afterward.
- [x] 3.3 Handle session rename by re-acquiring under the new owner and name; verify the reported address reflects the new alias without a restart.
- [x] 3.4 On every state change, release the old lease before acquiring the new one; verify exactly one lease row remains after a transition.

## 4. Extension — resilience

- [x] 4.1 Treat a failed renewal as a lost lease: attempt re-acquire, report it, and keep consuming; verify a lease taken by another owner does not stop message delivery.
- [x] 4.2 Treat a failed acquire as non-fatal: start consumption anyway and report the condition; verify the session still receives a published message when it holds no lease.

## 5. CLI — sessions listing

- [x] 5.1 Add the `sessions` subcommand and a store query over the engine lease table that filters unexpired rows; verify against a seeded temporary bus.
- [x] 5.2 Parse the lease name and owner into kind, state, id, alias, and remaining lifetime; verify unit tests for the parse and for address derivation.
- [x] 5.3 Deduplicate by participant id, preferring busy over idle over blocked; verify with two seeded rows for one participant.
- [x] 5.4 Add `--json` output; verify each line parses and carries address, kind, state, and expiry.
- [x] 5.5 Handle the empty case and keep the existing exit-code contract; verify exit status 0 with no participants.

## 6. Verification

- [x] 6.1 End-to-end: start a session with `pi -e ./extension` and confirm `msgq sessions` lists it, then run a turn and confirm the state flips idle to busy and back.
- [x] 6.2 Confirm the scoped write exception: the extension publishes no message record, and no stream offset or consumer cursor changes when presence is written.
- [x] 6.3 Confirm crash expiry: with a short TTL, a session that stops without releasing disappears from `msgq sessions` on its own.
- [x] 6.4 Update both component READMEs with a presence section and verify the documented commands match observed behavior.
