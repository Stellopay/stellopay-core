## NFT-Based Payroll Badge Contract

The `nft_payroll_badge` contract issues non-fungible badges representing
payroll participation and verification status (e.g. verified employer or
employee). Badges are intended for use in integrations, dashboards, and other
UI surfaces that need a compact on-chain signal.

### Contract Location

- Contract: `onchain/contracts/nft_payroll_badge/src/lib.rs`
- Tests: `onchain/contracts/nft_payroll_badge/tests/test_badge.rs`

### Design Overview

- **Non-fungible badges** – Each badge has a unique `id` and is associated with
  a single owner address.
- **Role-typed badges** – Badges carry a `BadgeKind` describing their purpose:
  `Employer`, `Employee`, or `Custom(code)`.
- **Metadata support** – Arbitrary `Bytes` metadata field allows storing URIs,
  IPFS CIDs, or compact JSON-encoded descriptors for frontends.
- **Admin-controlled issuance** – A single `admin` address is responsible for
  minting and (optionally) burning badges.
- **Optional transfer restrictions** – Per-badge `transferable` flag controls
  whether holders are allowed to transfer a given badge.
- **Event-driven integrations** – Badge lifecycle emits events for indexing and
  off-chain processing.

### Data Model

#### Types

- `BadgeError`
  - `NotInitialized`, `AlreadyInitialized`, `NotAdmin`, `NotOwnerOrAdmin`,
    `BadgeNotFound`, `TransferNotAllowed`
- `BadgeKind`
  - `Employer` – Verified employer badge
  - `Employee` – Verified employee badge
  - `Custom(u32)` – Integration- or UI-specific badge type
- `Badge`
  - `id: u128`
  - `owner: Address`
  - `kind: BadgeKind`
  - `metadata: Bytes`
  - `transferable: bool`
  - `created_at: u64`

#### Storage

- `Initialized` – one-time initialization flag
- `Admin` – badge admin address
- `NextBadgeId` – monotonically increasing badge id counter
- `Badge(id)` – stored `Badge` struct
- `OwnerOf(id)` – owner address for a badge id
- `BadgesOf(owner)` – `Vec<u128>` of badge ids owned by `owner`

### Public API

Initialization:

- `initialize(admin) -> Result<(), BadgeError>`

Badge management:

- `mint(caller, to, kind, metadata, transferable) -> Result<u128, BadgeError>`
  - Admin-only; mints a new badge and emits:
    - `("badge_minted", id) -> (owner, kind, transferable)`
- `burn(caller, badge_id) -> Result<(), BadgeError>`
  - Admin or badge owner may burn; emits:
    - `("badge_burned", id) -> owner`
- `transfer(caller, badge_id, to) -> Result<(), BadgeError>`
  - Only the current owner may transfer; badge must be marked `transferable`.
  - Emits:
    - `("badge_transferred", id) -> (from, to)`

Read helpers:

- `get_badge(badge_id) -> Option<Badge>`
- `owner_of(badge_id) -> Option<Address>`
- `badges_of(owner) -> Vec<u128>`
- `get_admin() -> Option<Address>`

### Integration Patterns

- **Payroll verification in UIs**
  - Wallets or dashboards can:
    - Query `badges_of(address)` and then `get_badge(id)` to determine whether
      an address is a verified employer or employee.
  - Metadata can encode employer names, departments, or HR system references.
- **Off-chain indexing**
  - Indexers can subscribe to:
    - `badge_minted` and `badge_burned` for lifecycle tracking.
    - `badge_transferred` for history and current ownership.
- **Access control**
  - Other contracts can use `owner_of(badge_id)` + `get_badge(badge_id)` to
    gate features (e.g. only verified employers can call certain entrypoints).

### Transfer Restrictions

- For **identity-style badges** (e.g. KYC’d employer/employee), set
  `transferable = false` at mint time to keep badges non-transferable.
- For **achievement or completion badges** that can move between accounts, set
  `transferable = true` to allow standard `transfer` calls by the current
  owner.

### Security Considerations

- Badge issuance is centralized to the configured `admin`; this address should
  be protected via multisig or governance where appropriate.
- Non-transferable badges prevent unauthorized reassignment but do not enforce
  KYC/AML themselves; they reflect whatever off-chain checks the admin applies.
- Burning is allowed by admin or current owner; integrations relying on badge
  presence should handle the case where a badge is burned and no longer
  signals a role.

