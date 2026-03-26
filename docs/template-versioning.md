# Payroll template versioning

The `template_versioning` contract (`onchain/contracts/template_versioning`) stores **immutable** payroll template revisions and binds **agreements** to an exact `(template_id, version)` pair.

## Concepts

- **Template**: A logical payroll template identified by `template_id` (assigned at registration). The authenticated registrant becomes the **owner** who may publish versions.
- **Version**: A monotonically increasing number per template. Each version stores a `schema_hash` (typically a SHA-256 of the canonical schema or ABI), optional migration notes, and a `deprecated` flag.
- **Agreement**: A record that references a specific template version. Agreements are immutable with respect to the template version they were created with.

## API overview

| Function | Purpose |
|----------|---------|
| `initialize` | One-time admin (deployer) setup. |
| `register_template` | Create a new `template_id` and display name. |
| `publish_template_version` | Append a new immutable version (schema hash + notes). |
| `latest_version` | Return the highest published version number. |
| `get_version` | Load metadata for `(template_id, version)`. |
| `deprecate_version` | Mark a version deprecated; new agreements cannot use it. |
| `create_agreement` | Create an agreement bound to a **non-deprecated** version. |
| `get_agreement` | Fetch agreement by id. |

## Migration when template structure changes

1. Publish a new version with a new `schema_hash` and migration notes describing field changes.
2. Create new agreements against the new version (or `latest_version` after publishing).
3. Deprecate old versions once no new payrolls should use them.
4. Existing agreements remain valid; off-chain systems resolve the schema using `schema_hash` stored on-chain for that version.

## Security notes

- Only the template **owner** can publish or deprecate versions.
- Deprecated versions cannot receive new `create_agreement` calls.
- Empty `label` or template `name` is rejected (`InvalidData`).

## Tests

```bash
cd onchain
cargo test -p template_versioning
```
