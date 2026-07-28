## NFT Payroll Badge Contract

The `nft_payroll_badge` contract issues non-fungible payroll badges to
employee or employer addresses. Each badge is assigned a unique token id and
stores a human-readable name plus an off-chain metadata URI.

### Contract Location

- Contract: `onchain/contracts/nft_payroll_badge/src/lib.rs`
- Tests: `onchain/contracts/nft_payroll_badge/tests/test_badge.rs`

### Design Overview

- **Admin-controlled issuance** - A single owner address initializes the
  contract and is the only address allowed to mint badges.
- **Per-token metadata URI** - `mint` stores the badge metadata URI at issuance
  time. The owner can later update the URI for a specific token id if hosting
  moves or metadata needs correction.
- **Scoped metadata updates** - `update_metadata_uri` mutates one existing
  badge by token id and leaves ownership, name, and issuance timestamp
  unchanged.
- **Bounded owner queries** - `badges_of_paged` clamps page sizes to
  `MAX_PAGE_SIZE` to keep reads predictable for high-volume badge holders.
- **Event-driven indexing** - Metadata URI changes emit `MetadataUpdated` with
  the token id, old URI, and new URI.

### Data Model

#### `Badge`

- `id: u64` - unique badge token id.
- `owner: Address` - address that received the badge.
- `name: String` - human-readable badge name.
- `metadata_uri: String` - off-chain metadata URI, such as an HTTPS URL or IPFS
  URI.
- `issued_at: u64` - ledger timestamp when the badge was minted.

#### `MetadataUpdated`

- `token_id: u64` - badge whose metadata URI changed.
- `old_uri: String` - URI stored before the update.
- `new_uri: String` - replacement URI.

### Public API

Initialization:

- `initialize(owner)` - one-time setup. The owner must authorize the call.

Badge management:

- `mint(caller, recipient, name, metadata_uri) -> u64`
  - Owner-only.
  - Mints a badge to `recipient` and returns the new token id.
- `update_metadata_uri(caller, token_id, new_uri)`
  - Owner-only.
  - Updates the metadata URI for an existing badge.
  - Emits `MetadataUpdated`.

Read helpers:

- `get_badge(badge_id) -> Option<Badge>`
- `badges_of(owner) -> Vec<u64>`
- `badges_of_paged(owner, start, limit) -> PagedBadges`
- `badge_count(owner) -> u32`
- `get_owner() -> Option<Address>`

### `badge_count` Invariant

`badge_count(owner)` returns the number of badges that have been successfully
minted to `owner`. The value satisfies a strict invariant throughout the
contract's lifetime:

> **For every address `A`, `badge_count(A)` equals the total number of
> successful `mint` calls whose `recipient` argument was `A`.**

Key properties that follow from this invariant:

1. **Per-address isolation** — Minting to address B never changes
   `badge_count(A)`. Each address has its own independent counter stored under
   `StorageKey::OwnerBadgeCount(owner)`.

2. **Monotonically non-decreasing** — `badge_count` can only increase. There
   is no `burn` or transfer operation, so the count never decreases after a
   successful mint.

3. **Consistent with `badges_of`** — `badge_count(owner)` always equals
   `badges_of(owner).len()`. Both are derived from the same underlying
   `OwnerBadgeCount` and `OwnerBadgeAt` storage entries, updated atomically in
   `append_badge_to_owner`.

4. **Independent of the global token id sequence** — The global `NextBadgeId`
   counter produces unique token ids across all recipients. It is separate from
   `OwnerBadgeCount` and does not affect per-address counts.

#### Implementation

`badge_count` is a read-only method that directly reads `OwnerBadgeCount(owner)`
from persistent storage, returning 0 for any address that has never received a
badge. The counter is incremented exactly once per successful `mint` call inside
`append_badge_to_owner`, which also records the badge id at the new index.

```rust
// Simplified from src/lib.rs
fn append_badge_to_owner(env: &Env, owner: &Address, badge_id: u64) {
    let count = owner_badge_count(env, owner);          // current count (= next index)
    env.storage().persistent().set(
        &StorageKey::OwnerBadgeAt(owner.clone(), count), // store badge id at that index
        &badge_id,
    );
    env.storage().persistent().set(
        &StorageKey::OwnerBadgeCount(owner.clone()),     // increment count
        &(count + 1),
    );
}
```

#### Test Coverage

Two dedicated tests verify the invariant beyond the basic `test_badge_count`
case:

| Test | What it checks |
|------|----------------|
| `test_badge_count_sequential_distinct_recipients` | Mints to 30 distinct addresses one at a time. After each mint it asserts the new recipient has count 1, every prior recipient still has count 1, and every future recipient still has count 0. |
| `test_badge_count_combined_distinct_and_repeated_recipients` | Mints to 10 distinct addresses with non-uniform target counts (1–8 badges each), interleaved in round-robin order. After every individual mint it checks all 10 counters simultaneously, then does a final cross-check that `badge_count(r) == badges_of(r).len()` for each address. |

### Security Considerations

- Metadata URI updates are restricted to the initialized owner. This prevents
  arbitrary callers from rewriting badge provenance data.
- The update path is token-scoped and requires the badge to exist.
- The owner address should be protected with an operational process such as
  multisig or governance when badge metadata carries compliance or payroll
  meaning.
- `badge_count` is read-only and requires no authorization, making it safe to
  call from any context. It cannot be manipulated by any address other than the
  contract owner (via `mint`).
