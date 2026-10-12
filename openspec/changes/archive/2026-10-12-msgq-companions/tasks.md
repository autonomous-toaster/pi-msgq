## 1. CLI — the wire

- [x] 1.1 Add `--reply-to <offset>` to `msgq publish` so a record can reference the message it answers; verify an integration test that the stored record carries it.
- [x] 1.2 Add `msgq ask <address> [message]` that publishes a message asking for an answer and waits for one; verify an integration test where a reply published on the reply stream is printed and the command exits 0.
- [x] 1.3 Make `ask` end on a terminal acknowledgement, reporting that the recipient finished without replying; verify with a seeded completion acknowledgement and no reply.
- [x] 1.4 Make `ask` report "still running" when the wait window closes without a reply or a terminal acknowledgement, and let the window be disabled; verify both an expiring window and a disabled one.

## 2. companion command — spawning

- [x] 2.1 Create `companion/companion` with `spawn` and `close` dispatch, argument parsing, and `--dry-run`; verify the script is executable and a dry run prints commands without executing.
- [x] 2.2 Implement name deduplication against live companions, bounded, so the granted name never collides; verify that a taken name yields a suffixed name and that the returned address matches it.
- [x] 2.3 Create the tab with the companion's working directory, defaulting to the caller's, and with the parent address in the environment; verify a dry run carries both.
- [x] 2.4 Start `pi` in that tab with the granted name, the parent address as an argument, the trust choice, the topic passthrough, and the delegation depth; verify a dry run shows every argument, and that injection of a development extension works through the override variable.
- [x] 2.5 Block until the companion registers on the bus, and fail naming the tab and pane when it never does; verify against a live herdr, including the failure path when the extension cannot load.

## 3. companion command — closing and guards

- [x] 3.1 Implement `close`, reporting an unknown companion without failing destructively; verify closing a live companion closes its tab and a repeat reports that it is gone.
- [x] 3.2 Enforce the delegation depth cap, refusing to spawn at the cap unless explicitly overridden; verify a spawn at the cap is refused and one with the override proceeds as far as a dry run.
- [x] 3.3 Fail with a clear message naming the missing requirement when herdr or jq is absent; verify by running with an emptied path.

## 4. Extension — the tools

- [x] 4.1 Add a `peers` tool with `list`, `send`, `ask`, and `reply` that delegates to the CLI; verify each action against a live bus, and that `ask` returns the peer's reply as the tool result.
- [x] 4.2 Make `reply` default to the sender of the most recently received message, including its offset; verify an unnamed reply reaches that sender with the right reply reference.
- [x] 4.3 Add a `companions` tool with `spawn` and `close` that delegates to the companion command; verify a spawn reports the granted name and the address, and that a failed spawn is reported rather than returning a name.
- [x] 4.4 Add the reporting address from `--msgq-report-to` and the environment, used as the default recipient when a tool call names none; verify both sources and the precedence between them.
- [x] 4.5 Add a trusted sender allowlist that gates the `honor` policy; verify an untrusted sender is delivered display-only while a trusted one starts a turn.

## 5. Verification

- [x] 5.1 End to end: a parent session spawns a companion and asks it a question, and the companion's answer arrives as the parent's tool result.
- [x] 5.2 End to end: a spawn with `--dry-run` prints exactly the commands a real spawn performs, verified by comparing them against a capture of the real invocation.
- [x] 5.3 End to end: a companion running under `honor` does not start a turn for a sender outside its trusted set.
- [x] 5.4 End to end from a shell without an agent: spawn a companion, ask it something, read the reply, and close it.

## 6. Documentation

- [x] 6.1 Add `companion/README.md` covering spawn and close, every flag, the spawn result, and the herdr and jq requirements.
- [x] 6.2 Update the CLI and extension READMEs for `ask`, `--reply-to`, the two tools, the trusted sender allowlist, and the reporting address.

## 7. Follow-up, from a real session

- [x] 7.1 Add a root `README.md` with development instructions, including the three variables that wire a checkout together and the reason a companion cannot see a shell's environment.
- [x] 7.2 Make a missing helper binary actionable: a tool failure with no output now names the variable to set, because `companion exited with 1` told an agent nothing.
- [x] 7.3 Follow a rename: the session's own address and its subscription move with its presence lease, so it still receives at the name the roster advertises; verified by delivering to the new address after a rename.
- [x] 7.4 Refuse to send or to spawn without a bus address, because a parentless spawn carries no trusted-sender allowlist and would answer anyone.
