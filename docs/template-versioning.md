# Payroll template versioning

The `template_versioning` contract (`onchain/contracts/template_versioning`) stores **immutable** payroll template revisions and binds **agreements** to an exact `(template_id, version)` pair.

---

## Concepts

- **Template**: A logical payroll template identified by `template_id` (assigned at registration). The authenticated registrant becomes the **owner** who may publish versions.
- **Version**: A strictly-increasing number per template (`version > latest_version`). Each version stores a `schema_hash` (typically a SHA-256 of the canonical schema or ABI), optional migration notes, and a `deprecated` flag.
- **Agreement**: A record that references a specific template version. Agreements are immutable with respect to the template version they were created with.

## Invariants

### 1. Strictly-Increasing Version Invariant
Every newly published version number for a template must strictly increase over the current `latest_version` (`version > latest_version`).
- Publishing version $N+1$ after version $N$ succeeds and updates `latest_version` to $N+1$.
- Attempting to publish duplicate version $N$, a lower version $< N$, or version $0$ is strictly rejected with `VersioningError::InvalidData`.

### 2. Version Pinning Guarantee
Once an agreement is created with `create_agreement`, it is **permanently pinned** to the exact `(template_id, template_version)` pair specified at creation time. This is a critical security invariant:

- Publishing a new template version via `publish_template_version` does **not** affect existing agreements
- Existing agreements continue to resolve to their originally pinned version, even after newer versions become the latest
- To use a new template version, a new agreement must be explicitly created with the new version number
- This prevents silent schema migrations that could break agreement validation logic

This guarantee is tested in `test_publish_template_version_rejects_duplicate_or_out_of_order_version`, `test_publish_template_version_strictly_increasing_succeeds`, `agreement_pinned_to_version_n_after_version_n_plus_one_published`, `new_agreement_uses_latest_version_after_publish`, and `deprecated_template_remains_listable_not_creatable_and_preserves_existing_agreements`.

## API overview

| Function | Purpose |
|----------|---------|
| `initialize` | One-time admin (deployer) setup. |
| `register_template` | Create a new `template_id` and display name. Rejects if an active template with the same name exists. |
| `publish_template_version` | Append a new immutable version with explicit strictly-increasing version number (version, schema hash + notes). Rejects if `version <= latest_version`. |
| `latest_version` | Return the highest published version number. |
| `get_version` | Load metadata for `(template_id, version)`, including deprecated historical records. |
| `get_template` | Load the latest published template record, including if it is deprecated; intended for historical lookup/audit. |
| `deprecate_version` | Mark a version deprecated; new agreements cannot use it. Frees the name once all versions are deprecated. |
| `create_agreement` | Create an agreement bound to a **non-deprecated** version. |
| `get_agreement` | Fetch agreement by id. |
| `get_templates_by_name` | Return all template IDs ever registered under a name (including deprecated). Useful for audit/history. |

---

## Migration when template structure changes

1. Publish a new version ($N+1$) with a new `schema_hash` and migration notes describing field changes.
2. Create new agreements against the new version (or `latest_version` after publishing).
3. Deprecate old versions once no new payrolls should use them.
4. Existing agreements remain valid; off-chain systems resolve the schema using `schema_hash` stored on-chain for that version.
5. To **rename** or **replace** a template under the same name, deprecate all its versions first, then register a new template with that name.

---

## Error Reference

| Code | Name | Cause |
|------|------|-------|
| 1 | `AlreadyInitialized` | `initialize` called more than once |
| 2 | `NotInitialized` | Contract not yet initialised |
| 3 | `Unauthorized` | Caller is not the template owner |
| 4 | `TemplateNotFound` | No template exists for the given id |
| 5 | `VersionNotFound` | No version record for `(template_id, version)` |
| 6 | `VersionDeprecated` | Cannot create an agreement against a deprecated version |
| 7 | `InvalidData` | Empty template name, empty agreement label, version $0$, or non-increasing version ($version \le latest\_version$) |
| 8 | `AgreementNotFound` | No agreement for the given id |
| 9 | `NameCollision` | A non-deprecated template with this name already exists. Deprecate all its versions before re-registering. |

---

## Security notes

- Only the template **owner** can publish or deprecate versions.
- Newly published version numbers must strictly increase over `latest_version`. Out-of-order, duplicate, or non-positive version numbers are rejected (`InvalidData`).
- Deprecated versions cannot receive new `create_agreement` calls, but `get_template` and `get_version` intentionally continue to return their complete immutable records for auditing and historical agreement resolution.
- Empty `label`, empty template `name`, or invalid/non-increasing `version` is rejected (`InvalidData`).
- The name index is append-only: once a template ID is associated with a name, that association is permanent and always inspected by the collision check.
- A caller cannot "skip" the collision check by re-using a template ID from a different name; the check is keyed on the requested name, not on the caller.

---

## Tests

```bash
cd onchain
cargo test -p template_versioning
```

### Test coverage

- End-to-end lifecycle: register, publish, bind agreement, deprecate blocks new binds
- **Strictly-increasing version enforcement**:
  - `test_publish_template_version_rejects_duplicate_or_out_of_order_version` — publishing duplicate $N$ or lower version $< N$ returns `InvalidData`
  - `test_publish_template_version_strictly_increasing_succeeds` — publishing $N+1$ after $N$ succeeds and updates `latest_version`
- Deprecated-template invariant: `get_template` retains full metadata, `create_agreement` returns `VersionDeprecated`, and pre-existing agreements remain unchanged
- Non-owner cannot publish or deprecate
- Deprecation event emission (with and without reason, idempotent, non-owner blocked)
- **Naming-collision policy (issue #940)**:
  - `test_register_template_rejects_collision_with_active_template` — active name → `NameCollision`
  - `test_register_template_allowed_after_all_versions_deprecated` — fully-deprecated name → success, distinct template_id
  - `test_register_template_rejects_when_earlier_version_not_deprecated` — latest version deprecated → name freed
  - `test_register_template_allowed_when_prior_has_no_published_versions` — inert template does not block name
  - `test_register_template_different_names_do_not_collide` — distinct names are independent
  - `test_register_template_rejects_empty_name` — empty name → `InvalidData`
  - `test_get_templates_by_name_returns_full_history` — index contains full registration history
  - `test_register_template_collision_after_reuse` — second-generation active template still blocks name

