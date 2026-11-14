# Spec Delta

## ADDED Requirements

### Requirement: A follower waits on a notification

T2.2 ALWAYS, `msgq follow` SHALL wait for a notification rather than polling.
T2.2 ALWAYS, `msgq follow` SHALL look again when the bounded wait elapses without a notification.
T2.2 ALWAYS, the bounded wait SHALL remain shorter than the interval between lock renewals. T3.2

#### Scenario: An idle follower is quiet
- **WHEN** a follower holds an address that receives nothing
- **THEN** the follower SHALL issue no read until the bound elapses

#### Scenario: A publication wakes the follower at once
- **WHEN** a record is published to a followed address
- **THEN** the follower SHALL read the record without waiting for the bound

### Requirement: A names or card listing requests no counts

T2.5 ALWAYS, a names listing SHALL request addresses without record counts.
T2.5 ALWAYS, a card listing SHALL request addresses without record counts. T2.1, T2.2

#### Scenario: A listing of a long stream
- **WHEN** a caller lists names of an address that holds thousands of records
- **THEN** the listing SHALL take no longer than the same listing on an empty address
