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

#### `MetadataUpdated`

- `token_id: u64` - badge whose metadata URI changed.
- `old_uri: String` - URI stored before the update.
- `new_uri: String` - replacement URI.

#### `BadgeBurned`

- `token_id: u64` - badge that was revoked.
- `owner: Address` - address the badge was revoked from.

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

### Security Considerations

- Metadata URI updates are restricted to the initialized owner. This prevents
  arbitrary callers from rewriting badge provenance data.
- The update path is token-scoped and requires the badge to exist.
- The owner address should be protected with an operational process such as
  multisig or governance when badge metadata carries compliance or payroll
  meaning.
  - Burning is owner-only, uses the same authorization check as mint and
    metadata updates, and uses swap-remove on the owner's badge list so cost
    doesn't grow with how many badges the owner holds.
