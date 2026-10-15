## ADDED Requirements

### Requirement: An answer that is owed
When a run is about to stop and the question this session was most recently asked, and which asked
for an answer, is still unanswered, the extension SHALL deliver one more entry naming that question
and telling the agent to answer it — or to reply that nothing was needed. It SHALL do this at most
once for that question, SHALL NOT do it once it has been answered, and SHALL NOT do it for a record
that asks for no answer. It SHALL report where the session has a UI when it does it, so that how
often the convention needs rescuing is knowable from use rather than assumed.

#### Scenario: The answer arrives after all
- **WHEN** the agent stops with a question it was asked still unanswered
- **THEN** one entry asks it to answer, and the answer it then sends reaches the asker by the ordinary reply path

#### Scenario: Declining is an answer
- **WHEN** the agent answers that nothing needed reporting
- **THEN** the exchange is finished, and no further entry is delivered for that question

#### Scenario: Once per question
- **WHEN** a question is still unanswered after the agent has been asked about it once
- **THEN** no second entry is delivered for that question
#### Scenario: Already answered
- **WHEN** the agent answered the question while it was working
- **THEN** nothing is delivered when the run stops

#### Scenario: Nothing was asked
- **WHEN** a record that asks for no answer is delivered, or only acknowledgements arrive
- **THEN** no entry is ever delivered on its account

#### Scenario: It says so
- **WHEN** the extension asks the agent for the answer it owes
- **THEN** it reports that it did, where the session has a UI

### Requirement: Completion is reported to the sender
A message the model sends to another session SHALL ask to be told when the run that included it
completes, so that a sender that chose not to wait still learns the peer finished — including when
the peer finished without answering. That report SHALL be delivered as a notification and SHALL NOT
start a turn. It SHALL be additional to the reply, not a substitute for it.

#### Scenario: The peer finishes without replying
- **WHEN** the peer's run that included the message completes and no answer came
- **THEN** the sender is told that the run completed

#### Scenario: It does not wake the sender
- **WHEN** that report arrives at a session that is idle
- **THEN** it is delivered without starting a turn

#### Scenario: The reply is unaffected
- **WHEN** the peer answers
- **THEN** the answer arrives as it always did, and the report neither replaces nor delays it

#### Scenario: A peer that never runs
- **WHEN** the peer never runs the message at all
- **THEN** no report arrives, and a sender that is waiting is bounded only by its own wait window
