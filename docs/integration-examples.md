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
