## NFT Payroll Badge Contract

The `nft_payroll_badge` contract issues non-fungible payroll badges to
employee or employer addresses. Each badge is assigned a unique token id and
stores a human-readable name plus an off-chain metadata URI.

### Contract Location

- Contract: `onchain/contracts/nft_payroll_badge/src/lib.rs`
- Tests: `onchain/contracts/nft_payroll_badge/tests/test_badge.rs`

### Design Overview

- **Admin-controlled issuance** - A single owner address initializes the
  contract and is the only address allowed to mint and burn badges.
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
- `burn(caller, token_id)`
  - Owner-only.
  - Burns (revokes) a badge by token id.
  - The badge id is never reused; a subsequent mint always receives a fresh id.
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

### Security Considerations

- Burn is restricted to the initialized owner. This prevents arbitrary callers
  from revoking employee badges.
- A burned badge id is never reused; `next_badge_id` always increments, so a
  subsequent mint for the same employee produces a fresh token id. Off-chain
  systems can safely reference a burned id without risk of collision.
- Metadata URI updates are restricted to the initialized owner. This prevents
  arbitrary callers from rewriting badge provenance data.
- The update path is token-scoped and requires the badge to exist.
- The owner address should be protected with an operational process such as
  multisig or governance when badge metadata carries compliance or payroll
  meaning.
