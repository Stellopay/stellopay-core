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
- **Tenure-based tier progression** - Each badge carries a **tier** field
  (`Bronze → Silver → Gold`) that serves as a monotonically non-decreasing
  record of tenure. Once a badge is minted or upgraded to a given tier, it
  can never be downgraded. Tier upgrades are owner-only and enforce the
  strict-ordering invariant at the contract level.
- **Per-token metadata URI** - `mint` stores the badge metadata URI at issuance
  time. The owner can later update the URI for a specific token id if hosting
  moves or metadata needs correction.
- **Scoped metadata updates** - `update_metadata_uri` mutates one existing
  badge by token id and leaves ownership, name, tier, and issuance timestamp
  unchanged.
- **Bounded owner queries** - `badges_of_paged` clamps page sizes to
  `MAX_PAGE_SIZE` to keep reads predictable for high-volume badge holders.
- **Event-driven indexing** - Metadata URI changes emit `MetadataUpdated` with
  the token id, old URI, and new URI. Tier upgrades emit `TierUpgraded` with
  the token id, old tier, and new tier.
  - **Revocable badges** - `burn` lets the owner permanently revoke a badge
    from a terminated employee or one issued in error, removing it from
    storage and from the owner's badge list.

### Data Model

#### `Badge`

- `id: u64` - unique badge token id.
- `owner: Address` - address that received the badge.
- `name: String` - human-readable badge name.
- `metadata_uri: String` - off-chain metadata URI, such as an HTTPS URL or IPFS
  URI.
- `issued_at: u64` - ledger timestamp when the badge was minted.
- `tier: Tier` - badge tenure tier (`Bronze`, `Silver`, or `Gold`). Set at
  mint time and can only increase via `upgrade_tier`.

#### `Tier`

Enum with three variants, ordered `Bronze < Silver < Gold`:
- `Bronze` — entry-level tenure.
- `Silver` — mid-level tenure.
- `Gold` — highest tenure tier.

#### `MetadataUpdated`

- `token_id: u64` - badge whose metadata URI changed.
- `old_uri: String` - URI stored before the update.
- `new_uri: String` - replacement URI.

#### `TierUpgraded`

- `token_id: u64` - badge whose tier changed.
- `old_tier: Tier` - tier value before the upgrade.
- `new_tier: Tier` - tier value after the upgrade.

#### `BadgeBurned`

- `token_id: u64` - badge that was revoked.
- `owner: Address` - address the badge was revoked from.

### Public API

Initialization:

- `initialize(owner)` - one-time setup. The owner must authorize the call.

Badge management:

- `mint(caller, recipient, name, metadata_uri, tier) -> u64`
  - Owner-only.
  - Mints a badge to `recipient` with the given `tier` and returns the new
    token id.
- `update_metadata_uri(caller, token_id, new_uri)`
  - Owner-only.
  - Updates the metadata URI for an existing badge.
  - Does not modify the badge's ownership, name, tier, or issuance timestamp.
  - Emits `MetadataUpdated`.
- `upgrade_tier(caller, token_id, new_tier)`
  - Owner-only.
  - Upgrades a badge to a strictly higher tier (`Bronze → Silver`,
    `Silver → Gold`, `Bronze → Gold`).
  - Panics if the badge does not exist.
  - Panics if `new_tier ≤ current_tier` (downgrade or no-op).
  - Emits `TierUpgraded`.
- `burn(caller, badge_id)`
  - Owner-only.
  - Revokes a badge: removes it from storage and from the owner's
    `badges_of` / `badges_of_paged` results, and decrements `badge_count`.
  - Emits `BadgeBurned`.
  - Panics if `badge_id` does not exist.

Read helpers:

- `get_badge(badge_id) -> Option<Badge>`
- `badges_of(owner) -> Vec<u64>`
- `badges_of_paged(owner, start, limit) -> PagedBadges`
- `badge_count(owner) -> u32`
- `get_owner() -> Option<Address>`

### Read-Query Error Semantics

`get_badge(badge_id)` returns `Option<Badge>`:

- `Some(Badge)` — the badge was minted and all fields are exactly as stored at
  mint time (or updated by `update_metadata_uri`).
- `None` — no badge with that ID has ever been minted. Callers **must not**
  treat a missing return as a zero-valued or default badge; the only valid
  interpretation is that the ID does not exist.

This means downstream consumers should always pattern-match or call `.expect()`
/ `.unwrap()` with a descriptive message, and must never assume that a `None`
response shares any fields with a real badge.

### Security Considerations

- Burn is restricted to the initialized owner. This prevents arbitrary callers
  from revoking employee badges.
- A burned badge id is never reused; `next_badge_id` always increments, so a
  subsequent mint for the same employee produces a fresh token id. Off-chain
  systems can safely reference a burned id without risk of collision.
- Metadata URI updates are restricted to the initialized owner. This prevents
  arbitrary callers from rewriting badge provenance data.
- The update path is token-scoped and requires the badge to exist.
- Tier upgrades enforce a strict monotonic non-decreasing constraint at the
  contract level. A badge's `tier` field can only move forward
  (`Bronze → Silver → Gold`); any call that would cause a regression panics
  before mutating state. This protects the tier as an immutable record of
  tenure.
- Each badge's tier is independently protected — upgrading or downgrading one
  badge has no effect on any other badge held by the same address.
- The owner address should be protected with an operational process such as
  multisig or governance when badge metadata carries compliance or payroll
  meaning.
- `badge_count` is read-only and requires no authorization, making it safe to
  call from any context. It cannot be manipulated by any address other than the
  contract owner (via `mint`).
