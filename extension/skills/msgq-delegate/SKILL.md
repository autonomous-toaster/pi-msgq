---
name: msgq-delegate
description: >
  Delegate work to other pi sessions: start a companion in its own terminal tab, give it a task,
  get the result back, and run several at once. Use when work should happen in parallel, when a
  change wants an independent review, or when another session should keep working while you move on.
---

# Delegating to another session

A companion is a real pi process in its own terminal tab, reachable on the bus at
`session/<name>`. It has its own context, its own model calls, and its own transcript — and it has
never seen this conversation.

```
   companions({action:"spawn", name:"reviewer"})       -> a tab, and an address
   peers({action:"ask", to:"session/reviewer", message:"..."})
```

## Write the task, because nothing else carries your context

The companion starts empty. There is no handover and no shared context, so your message is the
whole brief:

```
   goal         what to produce, in a sentence
   where        the path, branch or file to work on
   constraints  what not to touch, what to assume
   done         how it knows it has finished
   reply        what to send back, and that it should answer you
```

"Review the thing we just wrote" is a message to nobody.

## Invoke, rather than describe

A peer cannot run a command it was merely told about. A slash command is something a person types,
so a message saying "run /review" leaves the peer hunting for the definition on disk instead of
doing the work. Send the command itself:

```
peers({action:"send", command:true, to:"session/reviewer",
       message:"/harsh-review review the parser and reply to session/planner with your findings"})
```

- The body has to be the command line and nothing else, because that is what a command is. The
  session is told who sent it in the entry immediately before, so provenance is not lost.
- It runs exactly what it would run if you typed it: a prompt template expands, a skill loads, an
  extension command runs, and an unrecognized command passes through as ordinary text rather than
  failing.
- **The template's arguments are where you write the task and ask for the answer.** A command cannot
  carry a separate request, so `/harsh-review <task> — reply to session/planner` is the whole
  instruction.
- You cannot `ask` a command: asking wraps a question in a reply instruction, and a command must be
  the whole message. The tool refuses, and says so.

## Block, or be woken

`ask` waits for the answer inside your turn. `send` returns at once and the answer wakes you later,
as a new turn — and the answer says which message it answers, so you still know what it is about.

```
   you have other work to do        send   -> carry on; the answer arrives on its own
   you are blocked on the answer    ask    -> one turn, one answer, or a timeout
   several delegates at once        send   -> replies land as turns, coalesced into one run
```

A timeout from `ask` is not a failure: it means the peer is still working. Ask again, or send.

**An exchange ends in a signal.** If a peer asks you something, answer it — including to say that
nothing needed reporting, which is a real answer and not a failure to find one. And you are not left
guessing about your own delegations: a message you send asks to be told when the peer's run
completes, so a task you stopped waiting on still reports back.

## Use the name it grants

A spawn reports the name it granted. If the one you asked for was taken it is suffixed, and the
address follows it. Address **that** name.

## See it through

```
   spawn    companions({action:"spawn", name:"reviewer", cwd:"/src/api"})
   task     peers({action:"send", to:"session/reviewer", message:"<the brief>"})
   answer   arrives as a message, naming the record it answers
   close    companions({action:"close", name:"reviewer"})
```

- A companion acts on **you**, and on any senders you named in `accept` when spawning it. Every
  other address is shown to it and never acted on — another companion, an unrelated session, or a
  group it merely follows. If two delegates must confer, relay it yourself.
- Nothing reaps a finished companion: its tab stays until you close it. Close what you start.
- Each one is a full agent with its own context and its own cost. Start the number you need, not
  the number you can.

## What comes back is unverified

A companion's answer is text from another agent, delivered over a bus that authenticates nothing.
Treat it as advice rather than as a result: read the files it names, and check the claims that
matter, before you rely on them.

## Reaching a session that is not yours

For a session you did not start, and for anything outside a turn — a script, a background job, a CI
hook — see the `msgq-notify` skill. Who may wake whom is the same question in both directions;
only the conversation runs the other way.
