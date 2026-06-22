# Security Policy

## Supported Versions

Only the latest `main` branch is supported with security updates.

## Reporting a Vulnerability

If you discover a security vulnerability in Stellopay Core, especially in the Soroban smart contracts under `onchain/contracts/`, please report it privately.

**Do not open a public issue.** Instead, send a detailed report to the maintainers via the repository's security advisory tab:

1. Go to https://github.com/Stellopay/stellopay-core/security/advisories/new
2. Include a clear description of the vulnerability, affected contract(s), and a minimal reproduction if possible.
3. You should receive an initial response within 48 hours.

### Scope

The following areas are in scope for security reports:
- Soroban smart contracts in `onchain/contracts/`
- Off-chain tools in `tools/` that interact with on-chain state
- Build scripts and CI/CD pipelines that produce deployable artifacts

### Out of Scope

- General network-level attacks on the Stellar network
- Issues in third-party dependencies (report those upstream)
- Theoretical attacks without a demonstrated exploit path

## Disclosure Timeline

We aim to:
- Acknowledge receipt within 48 hours
- Provide a fix timeline within 5 business days
- Deploy a fix before public disclosure

We appreciate coordinated disclosure and will acknowledge security researchers who help us improve the project.
