## 1. Address resolution

- [x] 1.1 Add a pure resolver for command addresses: `me` resolves to this session's own address, a bare name resolves to `session/<name>`, and an address that already carries a scheme is used as given. Verify with unit tests covering all three, plus a session with no address yet.
- [x] 1.2 Add a pure parser for the roster, turning `msgq sessions --json` output into entries of address, kind, state, alias, and expiry. Verify with unit tests over a well-formed line, a malformed line, and an empty result.

## 2. Registration and reporting

- [x] 2.1 Register the four commands with a description each and no argument completions, so nothing runs per keystroke. Verify over RPC with `get_commands` that all four appear with the source `extension` and their descriptions.
- [x] 2.2 Add one shared runner the handlers use: it builds the plan, runs it, reports the outcome, reports the address used and whether a recipient was absent, reports a plan that could not be built, and never throws. Verify with unit tests covering a plan error, a zero exit, and a non-zero exit.

## 3. The roster

- [x] 3.1 Make `/peers` read the roster and report each reachable address with its state. Verify with unit tests for the rendering and an e2e run where nothing else is reachable, asserting the command says so and does not fail.
- [x] 3.2 Add the picker: choosing a peer composes a message and sends it, and abandoning the composition sends nothing. Verify with an e2e run that answers the picker and the compose dialog, asserting the record, and a second run that abandons it, asserting no record.
- [x] 3.3 Report that a dialog is needed when none can be shown, instead of failing obscurely. Verify by running the command where no dialog can be shown and asserting the report and that nothing was sent.

## 4. The identity report

- [x] 4.1 Add a pure formatter for the identity block: address, bus location, subscribed addresses, delivery policy, presence state, and trusted senders. Verify with unit tests for presence on and presence off.
- [x] 4.2 Wire `/peers me` to that formatter, reading configuration and the presence handle and never the bus. Verify by running it with the bus pointed at an unreadable path and asserting the address, policy, and trusted senders are still reported.

## 5. Sending

- [x] 5.1 Make `/peer <address> <message>` send as this session with the asking type, and never fall back to a configured reporting address. Verify with unit tests over the built plan, and an e2e run asserting the record's sender, recipient, and type.
- [x] 5.2 Compose before sending when the message was not given, and send nothing when the composition is abandoned. Verify with an e2e run of each path.
- [x] 5.3 Offer the same picker `/peers` uses when no address was given. Verify with an e2e run that gives a message, picks a peer, and asserts the record reached that peer.
- [x] 5.4 Make `/reply [message]` address the sender of the most recently received message and carry that message's offset as the reply reference, and report that there is nothing to answer when no message has been received. Verify with an e2e run for each path.

## 6. Companions

- [x] 6.1 Add `companion list`: the named herdr agents that are also participants on the bus, joined by name, each with its name, address, herdr status, and working directory; one JSON object under `--json`; a report rather than a failure where herdr is unavailable. Update its usage text and README. Verify with a fake `herdr` on `PATH` against a temporary bus, asserting the join, the JSON shape, and the outside-herdr message.
- [x] 6.2 Add `/companions`, running that verb and rendering the entries, and reporting when herdr is unavailable. Verify with the same fake `herdr`, and with herdr removed from `PATH`.
- [x] 6.3 Confirm `/peers`, `/peer`, and `/reply` still work where herdr is unavailable, so the failure is contained to the one command. Verify with an e2e run without herdr on `PATH`.

## 7. The reply affordance

- [x] 7.1 Add the affordance to the rendered transcript entry, naming the sender an answer would go to, so a human can reply without looking anything up. Verify with unit tests over a single message and a coalesced batch, asserting each row names its own sender.
- [x] 7.2 Confirm the affordance stays out of the model's context. Verify with unit tests asserting the plain content of a delivered message is unchanged and contains no affordance, including when details are missing.

## 8. End-to-end

- [x] 8.1 Add an e2e scenario where one session sends `/peer` to another and verify the second receives a turn-bearing record from the first.
- [x] 8.2 Add an e2e scenario where a session receives a message and then runs `/reply`, and verify the sender receives a record carrying that message's offset as its reply reference.
- [x] 8.3 Add an e2e scenario sending to an address nobody holds and verify the record is published, the recipient is reported absent, and the session is unaffected.
- [x] 8.4 Add an e2e scenario where a session runs `/peer me` and verify the record is published to its own address and starts no turn.

## 9. Documentation and full verification

- [x] 9.1 Document the four commands, `me`, and bare-name resolution in `extension/README.md`, noting that `me` means nothing to the CLI. Verify the document names every command and matches the observed behavior.
- [x] 9.2 Document `companion list` in `companion/README.md` and the human command surface in the root `README.md`. Verify both name the verb and the commands.
- [x] 9.3 Run the extension unit suite and the CLI test suite, and verify both pass.
- [x] 9.4 Run every e2e scenario, existing and new, and verify all pass.
- [x] 9.5 Run `openspec validate --strict` for this change and verify it passes.
