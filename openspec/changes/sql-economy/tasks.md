# Tasks

## 1. The schema version guard

- [x] 1.1 Record `schema_version` in `msgq.meta` and skip the bootstrap when the recorded value matches the build. Verify a second command installs no schema.
- [x] 1.2 Refuse a connection to a bus whose schema version is newer, naming both versions. Verify the error and the exit code.
- [x] 1.3 Verify the guard runs the schema once after a version bump and not again on the next command.

## 2. The catalogue without counts

- [x] 2.1 Add a counts argument to `msgq.catalog` defaulting to true. Verify an address with many records lists as fast as an empty one when counts are off.
- [x] 2.2 Switch the names and card listings to request no counts. Verify the listings still print every address.

## 3. Waiting on a notification

- [x] 3.1 Make the extension subscription wait for a notification with a 30 s watchdog. Verify an idle subscription issues no read for 30 s and that a publish wakes it at once.
- [x] 3.2 Make `msgq follow` wait on a notification with a 10 s watchdog, shorter than the 30 s lock renewal. Verify an idle follower issues no read for 10 s and that a publish wakes it at once.
- [x] 3.3 Cache the queue name in the follow loop. Verify the follower issues no address lookup between reads.

## 4. Presence and the local log

- [x] 4.1 Slow the presence heartbeat to 40 s against a 120 s lifetime. Verify a day of idle presence publishes at most 2,160 records and that the roster still lists a live session.
- [x] 4.2 Checkpoint the local write-ahead log on close. Verify the database file holds the rows and the log does not grow without bound.

## 5. Measure it again

- [x] 5.1 Re-run the measurements: statements per command, idle queries per minute, bytes per presence row and WAL per command. Verify each number improves on the figures in the proposal.
