# Tasks

## 1. Vocabulary the CLI already enforces

- [x] 1.1 Turn `--type` and `--paths` into closed sets over `record::KNOWN_TYPES` and `card::PathMode`. Verify `msgq publish x --type nosuch` exits with a usage error and stores no record, and that `msgq publish x --type note` still publishes.
- [x] 1.2 Give `--ack` a value parser over the protocol phases. Verify `--ack started,completed` and bare `--ack` both publish, and that `--ack nosuch` exits with a usage error and stores no record.
- [x] 1.3 Verify the help of every command with a closed-set argument names its members. Record the exit-code change (invalid closed value: 1 → 2, different wording) in README's troubleshooting table.

## 2. Enumerations a shell can afford

- [x] 2.1 Add `--names` to `topics`, `sessions` and `consumers`. Verify each prints one name per line with no header, no count, no offset and no JSON separator.
- [x] 2.2 Verify `--names` conflicts with `--json`. Verify `consumers --names` omits another principal's cursor on the shared engine, and that `topics --names` costs one catalogue call however many addresses the bus holds.

## 3. The emitter and its providers

- [x] 3.1 Add `msgq completions zsh` emitting a script from `Cli::command()`: every subcommand, every long flag, `--db` as files, nested subcommands as their own dispatch. Verify the command exits zero and that two runs print identical output.
- [x] 3.2 Emit closed-set values as restricted completions and pattern values (`--ack`, `--older-than`, `--reply-to`) as suggestions that accept typed text. Verify the emitted script offers all six record types and all three path modes, and that `6h` stays acceptable.
- [x] 3.3 Declare the dynamic providers in Rust, keyed by command path and argument: addresses for `read`, `follow` and `publish` from `topics --names` and `sessions --names`; consumer names and cursor topics for `cursor get`, `set` and `reset`; topics for `consumers --topic`. Verify a key that names no real command or argument fails the emitter test.
- [x] 3.4 Verify a tab stores nothing. Complete an address on a fresh bus. Check afterwards that the bus holds no record and no cursor. Verify that `--db` and `MSGQ_DB` both reach the bus the completion enumerates.
- [x] 3.5 Verify the script keeps describing the CLI: walk the command tree and fail when a subcommand, a long flag or a member of a closed set is absent from the emitted text. Add `zsh -n` as a lint over the output.

## 4. Getting it into a shell

- [x] 4.1 Provide one command that writes the emitted script to `_msgq` in a directory on `fpath`. Verify the written file equals the emitter's output byte for byte.
- [x] 4.2 Refuse a shell other than zsh with a usage error, and name `zsh` in the help of `msgq completions`. Verify `msgq completions bash` prints no script and exits non-zero.
- [x] 4.3 Document the install in README: write `_msgq` into `fpath`, run `compinit`, keep `msgq completions zsh > …` as the manual form. State that a tab creates the default bus when nothing is configured, that only zsh is offered, and that the directory-derived address is not completed. Verify the documented commands run as written.

## Workflow follow-up

- Archive the change once the completion has been used in a real shell and the emitted script has not
  drifted from `--help`.
