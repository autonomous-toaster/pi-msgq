# Spec Delta

## Purpose

The function surface a shared deployment exposes, and what a runtime role may not do.

## ADDED Requirements

### Requirement: Every operation passes through a scoped function

T2.2 ALWAYS, an operation on a shared engine SHALL pass through a function that takes the credential.
T2.2 ALWAYS, the function SHALL resolve the credential at the start of the call.
T2.2 ALWAYS, the function SHALL check the address against the scopes of the resolved principal.

#### Scenario: The read function refuses another tenant
- **WHEN** a client calls the read function for an address outside the scopes of the client
- **THEN** the function SHALL refuse the call

### Requirement: The runtime role holds no direct access

T3.1 ALWAYS, the runtime role SHALL hold no privilege on a queue table.
T3.1 ALWAYS, the runtime role SHALL hold no privilege to create a queue.
T3.1 ALWAYS, the runtime role SHALL hold execute privilege on the function surface only.

#### Scenario: A direct read is refused
- **WHEN** the runtime role reads a queue table directly
- **THEN** the engine SHALL refuse the read

#### Scenario: A direct delete is refused
- **WHEN** the runtime role deletes from a queue table directly
- **THEN** the engine SHALL refuse the delete

### Requirement: Prune owns the watermark

T3.2 IF a prune removes records, THEN the bus SHALL write the retention watermark for that address.
T3.2 IF a caller writes a retention watermark without a prune, THEN the bus SHALL refuse the write.
T3.2 IF a prune names an address outside the scopes of the caller, THEN the bus SHALL refuse the prune.

#### Scenario: A forged watermark is refused
- **WHEN** a caller writes a retention watermark without removing records
- **THEN** the bus SHALL refuse the write

#### Scenario: A scoped prune removes only the caller's records
- **WHEN** a caller prunes an address inside the scopes of the caller
- **THEN** the prune SHALL remove records from that address alone

### Requirement: A cursor is owned

T3.3 ALWAYS, the bus SHALL stamp the principal of the caller as the owner of a cursor row.
T3.3 ALWAYS, a cursor operation SHALL address a cursor row owned by the caller.

#### Scenario: One consumer name in two tenants
- **WHEN** two principals each save a cursor under one consumer name
- **THEN** the bus SHALL store two cursor rows owned by the two principals
