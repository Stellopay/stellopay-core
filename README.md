# Stellopay Core

Stellopay Core is a Soroban-based payroll contract workspace for Stellar. It contains the on-chain contracts, documentation, and supporting scripts for payroll escrow, recurring salary disbursement, multi-currency payroll flows, governance, compliance, and related operational modules.

## Repository Layout

| Path | Purpose |
| --- | --- |
| `docs/` | Project documentation, API notes, integration guides, architecture docs, migration guidance, examples, and Windows build notes. |
| `onchain/` | Rust/Soroban workspace for smart contracts and integration tests. |
| `onchain/contracts/` | Individual contract crates for payroll, governance, compliance, payment scheduling, vesting, withdrawal controls, and related modules. |
| `scripts/` | Repository helper scripts, including migration/build helpers. |
| `tools/` | Supporting tooling for repository workflows. |

Start with the [documentation index](docs/README.md) for product and integration context, then use the [on-chain workspace guide](onchain/README.md) for contract build and test details.

## On-Chain Workspace

The Soroban workspace is defined in [`onchain/Cargo.toml`](onchain/Cargo.toml). It includes all crates under `onchain/contracts/*` plus `onchain/integration_tests`.

### Core payroll

| Contract | Focus |
| --- | --- |
| [`stello_pay_contract`](onchain/contracts/stello_pay_contract/) | Core contract: payroll agreements, milestone escrow, multi-currency, disputes. |
| [`payroll_escrow`](onchain/contracts/payroll_escrow/) | Escrowed salary funding and release flows. |
| [`payment_scheduler`](onchain/contracts/payment_scheduler/) | Scheduled and recurring payment support. |
| [`payment_retry`](onchain/contracts/payment_retry/) | Retry handling for failed payment attempts. |
| [`payment_splitter`](onchain/contracts/payment_splitter/) | Split-payment logic for multi-recipient payouts. |
| [`payment_history`](onchain/contracts/payment_history/) | Immutable on-chain payment history log. |

### Compliance and reporting

| Contract | Focus |
| --- | --- |
| [`compliance_checker`](onchain/contracts/compliance_checker/) | Rule-based action compliance checks. |
| [`compliance_reporting`](onchain/contracts/compliance_reporting/) | Structured compliance report emission. |
| [`tax_withholding`](onchain/contracts/tax_withholding/) | On-chain tax-withholding deductions. |
| [`audit_logger`](onchain/contracts/audit_logger/) | Cross-contract audit-trail emission. |

### Access control and governance

| Contract | Focus |
| --- | --- |
| [`rbac`](onchain/contracts/rbac/) + [`rbac-interface`](onchain/contracts/rbac-interface/) | Role-based access control and typed cross-contract interface. |
| [`governance`](onchain/contracts/governance/) | On-chain proposal and voting system. |
| [`multisig`](onchain/contracts/multisig/) | Multi-signature approval for high-stakes operations. |
| [`employee_roles`](onchain/contracts/employee_roles/) | Per-employee role and permission registry. |
| [`department_manager`](onchain/contracts/department_manager/) | Org-unit grouping for payroll operations. |

### Financial controls

| Contract | Focus |
| --- | --- |
| [`price_oracle`](onchain/contracts/price_oracle/) | FX rates and pricing for multi-currency flows. |
| [`fee_collector`](onchain/contracts/fee_collector/) | Protocol fee collection and routing. |
| [`rate_limiter`](onchain/contracts/rate_limiter/) | Per-caller claim rate limiting. |
| [`salary_adjustment`](onchain/contracts/salary_adjustment/) | Dynamic salary override hooks. |
| [`bonus_system`](onchain/contracts/bonus_system/) | On-chain bonus calculation and distribution. |
| [`expense_reimbursement`](onchain/contracts/expense_reimbursement/) | Employee expense claim and approval. |

### Vesting and lifecycle

| Contract | Focus |
| --- | --- |
| [`token_vesting`](onchain/contracts/token_vesting/) | Time-based and cliff vesting schedules. |
| [`withdrawal_timelock`](onchain/contracts/withdrawal_timelock/) | Withdrawal delay enforcement. |
| [`slashing_penalty`](onchain/contracts/slashing_penalty/) | Penalty slashing on policy violations. |
| [`dispute_escalation`](onchain/contracts/dispute_escalation/) | Escalated dispute handling beyond the core arbiter. |
| [`nft_payroll_badge`](onchain/contracts/nft_payroll_badge/) | NFT badge issuance for payroll milestones. |

### Tooling crates (rlib only)

| Crate | Purpose |
| --- | --- |
| [`rbac-interface`](onchain/contracts/rbac-interface/) | Typed cross-contract RBAC client (no cdylib dependency). |
| [`milestone-interface`](onchain/contracts/milestone-interface/) | Typed cross-contract milestone query client. |
| [`template_versioning`](onchain/contracts/template_versioning/) | Contract schema versioning utilities. |

## Documentation Map

- [API documentation](docs/api/README.md)
- [Integration guide](docs/integration/README.md)
- [Architecture](docs/architecture.md)
- [Examples](docs/examples/README.md)
- [Best practices](docs/best-practices/README.md)
- [Developer tools](docs/dev-tools/README.md)
- [Migrations](docs/migrations.md)
- [Upgrade and migration strategy](docs/upgrade-migration-strategy.md)
- [Windows build notes](docs/windows-build.md)

## Build And Test

Install Rust and the Soroban CLI before running contract commands locally. The on-chain README pins the Soroban CLI example to `20.0.0-rc.1`:

```sh
rustup install stable
cargo install --locked --version 20.0.0-rc.1 soroban-cli
```

Common local checks from the on-chain workspace:

```sh
cd onchain
cargo build
cargo test
```

For Soroban contract builds, use:

```sh
cd onchain
stellar contract build
```

On Windows GNU/MinGW, the repository documents a WASM-only path for the known `export ordinal too large` linker issue:

```powershell
rustup target add wasm32-unknown-unknown
.\scripts\migrations\build_wasm_only.ps1
```

See [Building on Windows](docs/windows-build.md) for the full Windows guidance.

## CI

The on-chain workspace uses GitHub Actions ([`.github/workflows/contracts.yml`](.github/workflows/contracts.yml)) to format-check, build, and test all Soroban contracts on every push and pull request targeting `main`.

Run the same checks locally before opening a PR:

```sh
cd onchain
cargo fmt --all -- --check   # formatting
cargo build --workspace      # build
cargo test --workspace       # tests
```

See [`docs/ci.md`](docs/ci.md) for the full local-run guide, prerequisite setup, and details on what CI does and does not check.

## Contributing and security

- [Contributing guide](CONTRIBUTING.md) — workspace layout, build/test workflow, and PR expectations
- [Security policy](SECURITY.md) — responsible disclosure for contracts under `onchain/contracts/`
- [Open an issue](.github/ISSUE_TEMPLATE/) — bug, feature, or security report templates

## Safety Notes

This repository contains smart contract code. Review migrations, upgrades, and deployment steps carefully before using any live network or production asset. Keep private keys, RPC credentials, wallet secrets, and production database or ledger data out of commits, issue comments, and logs.

### Dispute payout conservation

`resolve_dispute` / `resolve_dispute_multisig` (in `onchain/contracts/stello_pay_contract`) conserve funds deterministically:

- `pay_employee` is split equally across employees; the integer-division remainder (dust) is added to the **last** employee so the employee transfers sum to `pay_employee` exactly and no tokens are stranded.
- `pay_employee` and `refund_employer` must be non-negative, and their sum must not exceed the agreement's `total_amount` nor (when tracked) its real per-agreement escrow balance; the escrow balance is decremented by the distributed total after transfers. Out-of-range or negative payouts return `PayrollError::InvalidPayout`.

For upgrade and migration planning, start with [Migrations](docs/migrations.md) and [Upgrade and migration strategy](docs/upgrade-migration-strategy.md).

## License

This project is licensed under the MIT License. See [`onchain/README.md`](onchain/README.md#license) for the existing license note.


## Frontend — Landing Page

The `frontend/` directory contains a Next.js 15 (App Router) landing page for Stellopay.

### Quick start

```sh
cd frontend
npm install
npm run dev   # http://localhost:3000
```

Run tests:

```sh
cd frontend
npm test
```

### Structured data (JSON-LD)

The landing page emits two [schema.org](https://schema.org) schemas as a server-rendered
`<script type="application/ld+json">` tag, enabling Google to build a knowledge-panel entry
and a Sitelinks Searchbox:

| Schema | Purpose |
|---|---|
| `Organization` | Registers name, logo, URL, and `sameAs` social profiles with search engines |
| `WebSite` | Declares a `SearchAction` for the Sitelinks Searchbox feature |

Both schemas share a single `@context` declaration via the JSON-LD `@graph` pattern.

#### Source files

| File | Role |
|---|---|
| `frontend/app/metadata-constants.ts` | Single source of truth — all string constants (URL, name, logo, description) and the `buildJsonLdGraph()` factory |
| `frontend/app/components/JsonLd.tsx` | React Server Component that renders the `<script>` tag |
| `frontend/app/layout.tsx` | Root layout — exports Next.js `Metadata` (Open Graph, Twitter) using the same constants |
| `frontend/app/page.tsx` | Landing page — renders `<JsonLd graph={buildJsonLdGraph()} />` |

#### Updating the payload

All JSON-LD values come from `metadata-constants.ts`. To change the site URL, name, logo,
or social links, edit that file only — both the Metadata API tags and the JSON-LD output
update automatically.

```ts
// frontend/app/metadata-constants.ts
export const SITE_URL  = "https://stellopay.xyz";
export const SITE_NAME = "Stellopay";
export const LOGO_URL  = `${SITE_URL}/logo.png`;
```

#### Validation

Paste the production URL into [Google's Rich Results Test](https://search.google.com/test/rich-results)
to verify the structured data is recognised.  The expected output is:

- **Organization** — detected with name, logo, and sameAs links
- **WebSite** — detected with potentialAction SearchAction

#### Accessibility

- `<script type="application/ld+json">` is invisible to assistive technology; no ARIA
  attributes are required.
- The landing page uses semantic HTML5 landmarks (`<header>`, `<main>`, `<nav>`, `<footer>`).
- A skip-navigation link (`Skip to main content`) is provided for keyboard users
  (WCAG 2.1 SC 2.4.1).
- Text contrast meets WCAG 2.1 AA (≥ 4.5:1 for normal text, ≥ 3:1 for large text).
- Responsive grid layout tested at sm 640, md 768, lg 1024, xl 1280 breakpoints.

#### Tests

```sh
cd frontend && npm test
# 34 tests covering:
#   - constant shape and non-empty values
#   - buildJsonLdGraph() schema fields for Organization and WebSite
#   - JSON serialization round-trip and idempotency
#   - <JsonLd> component rendering
```
