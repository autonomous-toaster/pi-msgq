---
name: msgq-notify
description: >
  Reach a pi session from outside its turn: wake it with a message, run a command in the
  background and have it report when it finishes, or ask a session and wait for the answer.
  Use for scripts, jobs, CI hooks and watchers, or any work that must continue after the turn
  ends.
---

# Reaching a session from outside

A pi session on the message bus answers at `session/<name>`. Anything that can run a shell can put
a message in front of it — including a job that finishes long after the turn that started it ended.

## Find the address

```bash
msgq sessions      # who is around, and what they are doing
msgq consumers     # which addresses are being followed
```

The session you want is on one of those lines. An unnamed session answers at
`session/<session-id>`.

## Wake a session

```bash
msgq publish session/planner "the build is green" --type steer
```

| `--type` | the session |
|---|---|
| `note` | shows it, and does nothing else. Safe while it is busy. |
| `prompt` | starts a turn |
| `steer` | redirects the turn already running, or starts one if it is idle |
| `follow_up` | queues behind whatever is running |

Use `steer` for "the thing you were waiting on has finished", and `note` for something the session
should merely know.

## The two rules that fail silently

**Never send as the session you are waking.** A record whose `from` is the receiving session's own
address never starts a turn — a deliberate guard, so that a session cannot wake itself. A job that
publishes with `--from session/<that-session>` looks like it worked and is ignored.

**Know what the session trusts.** Under `honor`, a session with a trust list acts only on senders in
it. `--from` defaults to `cli/<user>@<host>`, which is fine for a session with no list; a companion
started with `--msgq-trust` will show the message and never act on it. If a companion ignores you,
this is why.

## Work that outlives the turn

Run it detached, and let it speak for itself when it is done:

```bash
nohup bash -c 'sleep 1200 && msgq publish session/planner "the migration finished" --type steer --from cli/bg@host' &
```

Nothing needs to watch it. The session is woken by the message, on the bus, under the same rules as
any other sender.

## Ask, and be answered

`msgq ask` publishes a question and waits for the answer. It ends on a reply, on a terminal
acknowledgement, or on its timeout, and a timeout is not failure.

```bash
msgq ask session/planner "which tests should I run?"
```

To answer something a session asked you, reference the record you are answering:

```bash
msgq publish session/planner "on it" --reply-to session/planner#12 --from cli/bg@host
```

A record's id is `<address>#<offset>`, and `msgq publish --print-id` prints it. A reference is an
offset in the stream named by the sending address, so the address inside the id has to be the one
you publish as. The session is then told which record you are answering, and knows what you mean.

## Know that it arrived

```bash
msgq publish session/planner "run the suite" --type prompt --ack completed --await
```

`--ack` asks to be told what happened to the record; `--await` stays until it is told. The phases
are `received` (responsibility taken, with the delivery mode), `started`, and `completed` (with an
outcome). A message that was only shown, never acted on, stops at `received`.

## What will not happen

Nothing waits for you, and nothing retries. There is no deadline and no reminder.

But a message that arrives does not vanish when it is ignored. A session that was asked a question
is asked once more, at the moment it stops, to answer it or to say that nothing needed reporting —
so a question that would once have gone quietly unanswered now ends in words. And a message you send
to another session asks to be told when that session's run completes, so a delegation you stopped
waiting on does not go silent either.
