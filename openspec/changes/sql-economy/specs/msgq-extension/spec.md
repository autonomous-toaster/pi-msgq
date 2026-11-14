# Spec Delta

## ADDED Requirements

### Requirement: The subscription waits on a notification

T2.6 ALWAYS, a shared subscription SHALL wait for a notification rather than polling.
T2.6 ALWAYS, a shared subscription SHALL read again when the bounded wait elapses without a notification. T3.1

#### Scenario: An idle subscription is quiet
- **WHEN** a session subscribes to an address that receives nothing
- **THEN** the session SHALL issue no read until the bound elapses

#### Scenario: A publication wakes the subscription at once
- **WHEN** a record is published to a subscribed address
- **THEN** the session SHALL receive the record without waiting for the bound

### Requirement: Presence renews a third of its lifetime

T2.7 ALWAYS, a session SHALL renew presence at one third of the declared lifetime.
T2.7 ALWAYS, the declared lifetime SHALL be 120 seconds unless configured otherwise. T4.1

#### Scenario: A day of idle presence
- **WHEN** a session idles for one day
- **THEN** the session SHALL publish at most 2,160 presence records
