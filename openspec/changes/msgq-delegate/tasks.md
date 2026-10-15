## 1. The identity report

- [x] 1.1 Extend the pure identity formatter with the recipient the session defaults to, and its delegation depth and cap. Verify with unit tests for each fact present, each absent, and a session that was never delegated to.
- [x] 1.2 Report those facts in `/peers me`. Verify that a session started with a parent and a depth shows both, and that a session with neither still shows every other fact.
- [x] 1.3 Lead `peers list` with the identity, before the roster. Verify with an end-to-end run that the model's list result names the session's own address and that the roster follows it unchanged, and with a second run where the bus cannot be read.

## 2. Depth is legible

- [x] 2.1 Read the delegation depth and the cap the session was started with, and report them. Verify with unit tests over the parsing, and an end-to-end run in a session started with both set.
- [x] 2.2 Export the cap into the spawned tab alongside the depth. Verify with `--dry-run` that both variables are passed, and with a live spawn whose child reports the cap it is under.

## 3. The trusted set

- [x] 3.1 Add `--trust <addresses>` to `companion spawn`, defaulting to the parent alone, and keep that default when the option is absent. Verify with `--dry-run` for both cases.
- [x] 3.2 Let the `companions` tool pass the trusted set through. Verify with unit tests over the built command line.
- [x] 3.3 Verify the behaviour end to end: a companion spawned trusting a second sender acts on a message from it, and one trusting only its parent receives that message and starts no turn from it.
- [x] 3.4 Verify that following is not trusting: a companion given an extra address to follow but not to trust receives its messages and acts on none of them.

## 4. The skill

- [x] 4.1 Write `extension/skills/msgq-delegate/SKILL.md`: delegating work to another session, writing a task a fresh process can act on, when to block and when to be woken, running several delegates, the granted name, that a delegate's answer is unverified, that following is not trusting, that nothing reaps a finished companion, and what a companion costs. Verify each point is present in the file and that it loads without a diagnostic.
- [x] 4.2 Check the two skill descriptions against each other so they route apart. Verify both are advertised by name and description in a session that loads the extension, and that neither description would cover the other's task.

## 5. Documentation and full verification

- [x] 5.1 Document the identity header, the second skill, and the trusted set in `extension/README.md`. Verify the document names all three and matches the observed behaviour.
- [x] 5.2 Document `--trust` and the cap that reaches the companion in `companion/README.md`. Verify the section names both and the option table lists the new flag.
- [x] 5.3 Update `DEVELOPMENT.md`, where the check for what loaded names one skill. Verify it names both and still describes the manifest trap.
- [x] 5.4 Run the CLI suite and the extension unit suite, and verify both pass.
- [x] 5.5 Run every end-to-end scenario, existing and new, and verify all pass.
- [x] 5.6 Run `openspec validate --strict` for this change and verify it passes.
