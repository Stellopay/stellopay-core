## Integration Examples

This document provides **minimal, working‑style examples** for interacting with Stellopay contracts from different environments.

It focuses on the payroll contract, but the patterns apply to other contracts as well.

---

### General Patterns

Across languages and frameworks, integration typically follows the same steps:

1. **Obtain the contract ID** (from deployment or config).
2. **Build a transaction** that invokes a contract function with typed arguments.
3. **Simulate** the transaction (optional but recommended) to estimate fees and validate parameters.
4. **Sign and submit** the transaction to the Soroban RPC endpoint.
5. **Decode results and events** to update off‑chain state.

The examples below sketch this flow in JavaScript/TypeScript and Rust.

---

### Example: JavaScript / TypeScript (Node.js)

This example uses the modern `@stellar/stellar-sdk` with Soroban support to call `create_payroll_agreement` on the payroll contract.

```ts
import {
  Contract,
  Networks,
  SorobanRpc,
  TransactionBuilder,
  xdr,
} from '@stellar/stellar-sdk';

const rpcUrl = 'https://rpc-futurenet.stellar.org';
const server = new SorobanRpc.Server(rpcUrl, { allowHttp: true });

const networkPassphrase = Networks.TESTNET; // or Futurenet/Mainnet
const contractId = '<PAYROLL_CONTRACT_ID>';

async function createPayrollAgreement(employerKeypair, tokenAddress: string) {
  const account = await server.getAccount(employerKeypair.publicKey());
  const contract = new Contract(contractId);

  const tx = new TransactionBuilder(account, {
    fee: '100000',
    networkPassphrase,
  })
    .addOperation(
      contract.call(
        'create_payroll_agreement',
        xdr.ScVal.scvAddress(xdr.ScAddress.scAddressTypeAccount(
          xdr.PublicKey.publicKeyTypeEd25519(employerKeypair.rawPublicKey())
        )),
        xdr.ScVal.scvAddress(Contract.fromContractId(tokenAddress).toScAddress()),
        xdr.ScVal.scvU64(604800n) // grace_period_seconds
      )
    )
    .setTimeout(60)
    .build();

  // Optional: simulate for fee and result preview
  const sim = await server.simulateTransaction(tx);
  if (sim.error) throw new Error(sim.error);

  tx.sign(employerKeypair);
  const sendResp = await server.sendTransaction(tx);
  console.log('Submitted:', sendResp.hash);
}
```

Key points:

- use the generated `Contract` helper for method encoding
- use `simulateTransaction` before sending for validation
- encode arguments as the correct Soroban XDR types

---

### Example: Rust Off‑Chain Service

Rust services can reuse the Soroban client libraries and the **generated contract client types**.

Below is a sketch of invoking `create_escrow_agreement` from a background worker using the auto‑generated `PayrollContractClient`:

```rust
use soroban_sdk::{Address, Env};
use stello_pay_contract::{PayrollContractClient};

fn create_escrow_example(env: &Env, contract_id: &Address) {
    let client = PayrollContractClient::new(env, contract_id);

    let employer = Address::from_string("GEMPLOYER...");
    let contributor = Address::from_string("GCONTRIB...");
    let token = Address::from_string("GTOKEN...");

    let amount_per_period: i128 = 1_000;
    let period_seconds: u64 = 86_400;
    let num_periods: u32 = 12;

    // In an off‑chain context this `Env` would be coming from the host,
    // but the call pattern is identical to the test clients.
    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token,
        &amount_per_period,
        &period_seconds,
        &num_periods,
    );

    // Store or index `agreement_id` in your service for later use.
    let _ = agreement_id;
}
```

This mirrors how the test suite exercises the contract and is a good starting point for any Rust‑based orchestration or command‑line tooling.

---

### Example: Using `stellar` CLI for Quick Integrations

For scripting and manual testing, the `stellar` CLI is often the simplest “integration client”.

```bash
# Invoke initialize(owner) on the payroll contract
stellar contract invoke \
  --id <PAYROLL_CONTRACT_ID> \
  --source <OWNER_KEYNAME> \
  --network futurenet \
  --func initialize \
  --arg address:<OWNER_ACCOUNT_ID>

# Call a simple getter, e.g., get_agreement
stellar contract invoke \
  --id <PAYROLL_CONTRACT_ID> \
  --source <ANY_KEYNAME> \
  --network futurenet \
  --func get_agreement \
  --arg u128:1
```

These patterns can be wrapped in shell scripts, CI jobs, or higher‑level deployment tooling to provide reliable, repeatable interactions without writing additional application code.

---

### Payroll + Token Vesting Integration Assumptions

The payroll and token vesting contracts are integrated by orchestration rather than by direct contract-to-contract calls. A hiring workflow should bind the same `employer`, `employee`, and `token` to:

- a `stello_pay_contract` payroll agreement for recurring salary claims
- a `token_vesting` schedule for grant, bonus, or equity-like vesting claims

The integration tests in `onchain/integration_tests/tests/test_token_vesting_payroll_integration.rs` cover the expected lifecycle:

- hire: employer creates and activates a payroll agreement, then creates a revocable vesting schedule for the same employee
- claims: employee claims payroll periods and vested tokens at multiple ledger timestamps
- termination: payroll cancellation starts the grace period, while vesting revocation refunds only unvested tokens and leaves vested-but-unclaimed tokens claimable
- dispute/grace alignment: an admin early release may be used after a payroll dispute or grace-period decision, but only the vesting owner can approve it
- security boundaries: mismatched employees cannot claim another employee's payroll or vesting, and mismatched employers cannot revoke a schedule

Security notes:

- Payroll escrow accounting remains independent from vesting escrow accounting; both contracts transfer the same SAC token but hold separate balances.
- Revocation freezes vesting at `revoked_at`; later ledger movement must not increase `releasable_amount`.
- Repeated same-ledger claims are rejected once no additional payroll period or vested amount is available.
- Off-chain services should persist the payroll `agreement_id` to vesting `schedule_id` mapping and verify the employer, beneficiary, and token match before presenting combined lifecycle actions.

---

### Payroll + Template Versioning Integration Wiring

The `template_versioning` contract stores **immutable** payroll template revisions, and `stello_pay_contract` creates live payroll agreements. Orchestrating the two produces a **pinned-template payroll**: an agreement whose terms are frozen at a specific template version, immune to later revisions.

Integration tests in `onchain/integration_tests/tests/test_template_versioning_payroll_integration.rs` cover the lifecycle:

- **Template setup**: register a template with a name, publish the first version (v1) with its `schema_hash` and optional migration notes.
- **Agreement pinning**: create a `template_versioning` agreement pinned exactly to v1. The returned `AgreementBinding` stores the `(template_id, version)` pair immutably.
- **Payroll creation**: create the corresponding `stello_pay_contract` payroll agreement — the caller (off-chain or orchestration layer) records the mapping between the two agreement IDs.
- **Version evolution**: publish a second template version (v2) with different terms. The pinned agreement still references v1; the payroll agreement's creation timestamp falls between the v1 and v2 publication times.
- **Deprecation safety**: deprecating v1 does **not** affect existing agreements pinned to v1; only new `create_agreement` calls against v1 are rejected.

#### Rust integration test pattern

```rust
use soroban_sdk::{Address, BytesN, Env, String};
use stello_pay_contract::{PayrollContract, PayrollContractClient};
use template_versioning::{TemplateVersioning, TemplateVersioningClient};

fn setup_template_wired_to_payroll(env: &Env) -> (u64, u128, u64) {
    let versioning = TemplateVersioningClient::new(env, &versioning_id);
    let payroll = PayrollContractClient::new(env, &payroll_id);

    // 1. Register template and publish v1
    let template_id = versioning.register_template(&admin, &name).unwrap();
    versioning.publish_template_version(&admin, &template_id, &hash, &notes, &false).unwrap();

    // 2. Create template_versioning agreement pinned to v1
    let tv_agreement_id = versioning.create_agreement(&employer, &template_id, &1, &label).unwrap();

    // 3. Create payroll agreement
    let payroll_agreement_id = payroll.create_payroll_agreement(&employer, &token, &grace);

    // Return all three IDs so callers can verify the mapping
    (template_id, payroll_agreement_id, tv_agreement_id)
}
```

#### Key invariants

- A `template_versioning` agreement's `template_version` field is set at creation time and **never changes** — not when newer versions are published, not when the pinned version is deprecated.
- Off-chain services SHOULD persist the `(template_versioning_agreement_id, payroll_agreement_id)` mapping after creation, since neither contract stores a cross-reference to the other.
- The payroll agreement's parameters (token, employer, grace period) SHOULD be validated against the `TemplateVersionRecord.schema_hash` off-chain before creation to ensure on-chain terms match the intended template version. The integration test confirms the temporal ordering: `v1.created_at <= payroll.created_at < v2.created_at`.
- Deprecated versions reject new `create_agreement` calls with `VersioningError::VersionDeprecated`, preventing inadvertent use of outdated terms.
