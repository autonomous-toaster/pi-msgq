## 1. Scaffold and spike

- [x] 1.1 Create `extension/package.json` declaring `@russellthehippo/honker-node` and run `npm install` in `extension/`; verify `extension/node_modules/@russellthehippo/` contains the binding and its platform binary.
- [x] 1.2 Add a minimal `extension/index.ts` that imports the binding and opens the bus; verify the import interop works under pi's TypeScript loader (fall back to `createRequire` if ESM default interop fails), loading it with `pi -e ./extension` and confirming a startup notice.
- [x] 1.3 Extend the root `.gitignore` to cover `extension/node_modules/`; verify `git status` shows no installed dependencies.

## 2. Configuration and addressing

- [x] 2.1 Implement bus path resolution (`MSGQ_DB` else `~/.pi/agent/msgq/msgq.db`); verify with a unit test covering both cases.
- [x] 2.2 Implement address validation for the reserved schemes (`session/`, `cwd/`, `group/`, `cli/`) with bare names rejected; verify unit tests for accept and reject.
- [x] 2.3 Implement configuration parsing for policy and extra addresses from both the registered flag and the environment variable; verify a unit test that the default policy is `display`, that a comma-separated list parses, and that an unknown policy or address is rejected.
- [x] 2.4 Derive the session's own address `session/<self>` and a unique consumer identity (alias joined with session id when an alias exists); verify a unit test that two sessions sharing an alias produce different consumer identities.

## 3. Record decoding

- [x] 3.1 Implement envelope decoding that extracts `v`, `type`, `from`, `to`, `message`, and tolerates unknown types and unknown fields; verify unit tests for a known record, an unknown type, and a missing required field.
- [x] 3.2 Make an undecodable payload and an unsupported protocol version skip-with-report rather than throw; verify a unit test that decoding returns a skip result for both.

## 4. Bus consumer

- [x] 4.1 Open the engine handle on session start with a raised watcher interval; verify it opens against both an existing bus and a missing one (engine creates it).
- [x] 4.2 Subscribe to each configured address with single-event cursor saving; verify by delivering one record and then reading the consumer offset from the CLI, confirming it advanced before the delivery was observable.
- [x] 4.3 Release subscriptions and the engine handle on session shutdown, idempotently; verify a second shutdown produces no error and the process exits cleanly.

## 5. Delivery

- [x] 5.1 Implement display-only delivery as a visible custom message that carries sender and receiving address and never starts a turn; verify manually that a note arrives while idle and while streaming without queuing a turn.
- [x] 5.2 Implement policy gating so `display` downgrades every record and `honor` maps `prompt`, `steer`, `follow_up`, and `note` to their pi delivery modes; verify each mapping with a published record of that type.
- [x] 5.3 Implement coalescing so turn-bearing records that arrive during a run deliver as one message with exactly one continuation at the agent settle boundary; verify by publishing several records during one run and confirming a single continuation.
- [x] 5.4 Implement the self-trigger guard so a record whose sender equals the session's own address is delivered display-only; verify with a self-addressed record under the `honor` policy.
- [x] 5.5 Register a message renderer that presents each delivered record with its type, sender, address, and body, numbers coalesced batches, and falls back to plain content when details are absent; verify with unit tests over the composed text and an end-to-end delivery.

## 6. Observability and degradation

- [x] 6.1 Report active subscriptions through a status line and a startup notice, guarded so UI is only used when the session has a UI; verify in interactive and in non-interactive mode.
- [x] 6.2 Handle an unreadable bus, a dead watcher, and an unsupported record version by stopping or skipping with a report and never crashing the session; verify each by simulating the condition and confirming the session continues.

## 7. Verification

- [x] 7.1 End-to-end under the default policy: start a session with `pi -e ./extension`, publish a note with the CLI to the session's address, and confirm it appears with provenance and starts no turn.
- [x] 7.2 End-to-end under `honor`: publish a prompt to an idle session (turn starts), a steer mid-run, and several records during one run (single coalesced continuation).
- [x] 7.3 Confirm the extension is read-only and non-invasive: no publish call exists, `cli/` is unchanged, and no bus schema change is introduced.
