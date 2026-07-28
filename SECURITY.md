# Security Policy

Stellopay Core manages payroll escrow and related Soroban contracts on Stellar. If you believe you have found a security vulnerability, please report it responsibly.

## Supported versions

Security fixes are applied to the `main` branch and released through reviewed pull requests. Use the latest `main` commit or the most recent tagged release when deploying to production networks.

## Scope

In scope:

- All Soroban contracts under [`onchain/contracts/`](onchain/contracts/), including payroll, escrow, governance, RBAC, compliance, payment scheduling, vesting, and supporting modules.
- Cross-contract integration behaviour covered by [`onchain/integration_tests/`](onchain/integration_tests/).
- [`tools/cli`](tools/cli/) when a finding affects deployment, upgrade, or contract interaction safety.

Out of scope:

- Third-party wallets, RPC providers, and Stellar network infrastructure outside this repository.
- Social engineering, physical attacks, or denial-of-service against public endpoints not maintained in this repo.
- Issues in forked or unpublished contract deployments that diverge from `main` without disclosure.

## Reporting a vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Report privately using one of these channels:

1. **GitHub private vulnerability reporting (preferred)**  
   [Open a private security advisory](https://github.com/Stellopay/stellopay-core/security/advisories/new) on this repository.

2. **Security issue template**  
   Use the [Security report](.github/ISSUE_TEMPLATE/security_report.md) template only if private advisory reporting is unavailable. Mark the issue as sensitive and avoid posting exploit details, keys, or mainnet transaction data in the body.

Include:

- A clear description of the issue and affected contract or tool path.
- Steps to reproduce, including network (testnet/mainnet) and contract IDs if relevant.
- Impact assessment (fund loss, auth bypass, upgrade abuse, etc.).
- Proof of concept if available, preferably against testnet.

## What to expect

- Acknowledgement within **5 business days** for valid reports.
- Status updates as the report is triaged and remediated.
- Coordination on disclosure timing so users can patch before public details are shared.

## Safe harbour

We support good-faith security research on **testnet** and local environments. Do not test against mainnet funds you do not own, do not exfiltrate user data, and do not degrade production services.

## Related documentation

- [Threat model](docs/threat-model.md)
- [Security testing tools](docs/security-testing-tools.md)
- [Emergency pause](docs/emergency-pause.md)
- [Upgrade entrypoint security](docs/upgrade-entrypoint.md)
