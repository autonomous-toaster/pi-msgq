# Tasks

## 1. The schema in order

- [x] 1.1 Create `msgq.identity` beside the other tables before the migration block runs. Verify a database with no schema accepts a command and holds the table afterwards.
- [x] 1.2 Verify a record published on the freshly bootstrapped database reads back.

## 2. The read path without a credential

- [x] 2.1 Let a read proceed when no identity exists and refuse when one does. Verify `topics` on a bus with no identity prints an empty listing and exits zero.
- [x] 2.2 Verify a read without a credential is refused once an identity is registered.

## 3. The regression test

- [ ] 3.1 Add a shared-engine test that starts from a database with no bus schema. BLOCKED: the bus needs
  pgmq, which the image installs only in its own database; copying it needs an idle source, and the
  parallel runner is never idle. Verified by hand on a fresh container instead: dropped `msgq`, ran
  `topics` (exit 0), published and read. Unblock by vendoring the pgmq SQL the image downloads, or by
  giving the runner a serialised slot.
