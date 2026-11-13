# Tasks

## 1. Principal-keyed identity

- [x] 1.1 Re-key `msgq.identity` to a principal. The row names a user. `scopes` holds the namespace of the user. Drop the address-shaped validation on registration. Verify a registration of `alice` succeeds with scopes `session/alice` and `group/alice`.
- [x] 1.2 Refuse a sender claim outside the scopes. Keep a claim inside the scopes. Refuse the write outside the scopes. Store nothing on a refusal. Verify the error names the refused address.

## 2. Scoped reads

- [x] 2.1 Add the read function surface. `msgq.read_since` returns records. `msgq.stream_of` returns head, oldest and dropped. `msgq.catalog` returns addresses and cursors. Resolve the credential first in every function. Verify a cross-tenant read is refused.
- [x] 2.2 Grant the runtime role the function surface only. Grant execute on the read functions. Revoke select on the engine schema. Verify a direct select as the runtime role is refused.
- [x] 2.3 Move the extension's shared reads onto the read function. Replace the direct select in the subscription pump. Verify the extension holds no direct select.
- [x] 2.4 Report a refused read in the extension. Report the refusal. Stop reading the address. Keep the session running. Verify a refused read delivers nothing.

## 3. Tenant-owned cursors and prune

- [x] 3.1 Add an owner to cursors. Stamp the owner from the credential on every cursor write. Verify two principals saving one consumer name produce two rows.
- [x] 3.2 Implement the prune function. `msgq.prune` deletes in batches inside one scoped address. The prune writes the retention watermark itself. The prune returns the total removed. Verify that a watermark written without a prune is refused.
- [x] 3.3 Keep the runtime role unable to delete. Revoke delete on the queue tables. Perform every delete inside the prune function. Grant execute on the prune function. Verify a direct delete as the runtime role is refused.
- [x] 3.4 Scope the consumer surface in the CLI. The listing names only the caller's rows. The listing shows the owner. Every cursor command acts on a row owned by the caller. Verify another principal's consumer is absent.
- [x] 3.5 Scope the prune command and its reporting. Prune removes only addresses inside the caller's scopes. Prune reports the total removed. Verify a refused prune exits non-zero and removes no record.

## 4. Owner stamp

- [x] 4.1 Stamp the writing principal in the headers of every authorized record. Place the value next to the address. Expose the value to readers. Verify a forged `meta` field is not the source.
- [x] 4.2 Report the caller in `db info`. Name the principal and the scopes. Keep the credential line. Keep the local trust statement unchanged. Verify the shared report names the principal.

## 5. Credential lease

- [ ] 5.1 Make the lease renewable. Add `msgq.renew_credential` that extends the same row. Keep the digest on renewal. Accept a renewal once the lease elapsed. Renew at session start. Renew at half the lease. Verify sibling sessions keep working. Verify a resumed session renews.

## 6. The teams door

- [x] 6.1 Record the room rule. A room is an address listed in more than one principal's scopes. Prune authority follows ownership rather than access. Name read scopes as the later addition for a guest. Verify a shared-room member can write and cannot prune.
