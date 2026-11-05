## 1. Protocol and locator derivation

- [x] 1.1 Add `cli/src/card.rs` with `derive_card(cwd, paths)`, including git root, normalized origin, branch, worktree, and path modes; verify with unit tests over git, subdirectory, non-git, detached HEAD, linked worktree, HTTPS/SSH remotes, and all three path modes.
- [x] 1.2 Export the module and add `msgq card [--cwd PATH] [--paths relative|full|none]` in `cli/src/main.rs`; verify `cargo run -- card --json` and the integration test in `cli/tests/cli.rs`.
- [x] 1.3 Update `PROTOCOL.md` with the optional flat `card`, field semantics, path modes, and privacy warning; verify the presence sample and wording match the specs.

## 2. CLI roster

- [x] 2.1 Parse `event.card` in `cli/src/presence.rs` as an optional JSON object without invalidating the presence record; verify unit tests for a card, no card, and a non-object card.
- [x] 2.2 Carry the card into `sessions` and emit it in `--json`; verify integration tests assert the field is present only when supplied.
- [x] 2.3 Add `sessions --wide`, keeping the default text columns unchanged; verify integration tests assert default output is unchanged and `--wide` appends project, branch, worktree, and path.
- [x] 2.4 Document `msgq card`, the four fields, `--wide`, and a direct `msgq publish group/presence --json` example in `cli/README.md`; verify the command names and fields appear in the document.

## 3. Pi extension

- [x] 3.1 Add `card` support to `extension/src/presence.ts` and `setCard()`; verify with presence unit tests that the record includes the card and a change publishes once.
- [x] 3.2 Add `extension/src/card.ts` with validation, shallow override merge, null removal, display formatting, and the 512-byte cap; verify with unit tests for each behavior.
- [x] 3.3 Add `--msgq-context` and `--msgq-card-paths` plus environment parsing; verify config unit tests and that invalid values disable only the card.
- [x] 3.4 Derive the card on session start with `pi.exec(msgqBin, ["card", ...])`, refresh on input/agent start and a slow cadence, and republish only on change; verify with lifecycle tests or the e2e scenario.
- [x] 3.5 Add the `card` action to the `peers` tool limited to the four locator keys; verify unit tests for read, override, clear, and refusal of unknown keys.
- [x] 3.6 Preserve `card` in `parseSessions` and show project/branch in roster labels; verify `commands-text` tests.
- [x] 3.7 Use `sessions --wide` for the model’s list action with a fallback to plain `sessions`; verify the built command and fallback behavior.
- [x] 3.8 Add the `peer-locator` e2e scenario and update `extension/README.md`; verify the scenario passes and the README names the flags and action.

## 4. Maki extension

- [x] 4.1 Add `context` and `card_paths` options, `S.card`, and `refresh_card()` using `msgq card`; verify the generated presence payload includes the derived card.
- [x] 4.2 Refresh on start and `SessionCwdChanged`, publish only on change, and publish presence without a card on derivation failure; verify with a manual check or a Lua-level harness if available.
- [x] 4.3 Add the narrow `card` action to the `msgq` tool with override/clear semantics; verify the tool returns the current card and republishes on change.
- [x] 4.4 Show project/branch in `/peers me` and the roster tail, then update `maki/README.md`; verify the documented options and output match the implementation.

## 5. Full verification

- [x] 5.1 Run `cargo test --manifest-path cli/Cargo.toml` and verify it passes.
- [x] 5.2 Run `node --test extension/test/` and verify it passes.
- [x] 5.3 Run the new `peer-locator` e2e scenario and the existing command-surface scenario; verify both pass.
- [x] 5.4 Run a manual maki check in a git repository: `/peers me`, the `card` action, and `msgq sessions --wide`; verify the card appears and updates.
- [x] 5.5 Run `openspec validate --all --strict` and `git diff --check`; verify both pass.
