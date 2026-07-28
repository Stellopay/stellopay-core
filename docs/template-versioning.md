# Payroll template versioning

The `template_versioning` contract (`onchain/contracts/template_versioning`) stores **immutable** payroll template revisions and binds **agreements** to an exact `(template_id, version)` pair.

---

## Concepts

- **Template**: A logical payroll template identified by `template_id` (assigned at registration). The authenticated registrant becomes the **owner** who may publish versions.
- **Version**: A monotonically increasing number per template. Each version stores a `schema_hash` (typically a SHA-256 of the canonical schema or ABI), optional migration notes, and a `deprecated` flag.
- **Agreement**: A record that references a specific template version. Agreements are immutable with respect to the template version they were created with.

---

## Naming-Collision Policy (issue #940)

Template names form a **shared namespace** visible to all agreement creators. Registering a new template under a name that already belongs to an **active** (non-deprecated) template is rejected with `VersioningError::NameCollision (9)`.

### Why

An agreement creator selecting a template by name must be able to trust that the name resolves unambiguously to the schema they intend to use. Allowing two templates to share a name while one is active would silently shadow the existing one, causing new agreements to bind to a different schema than expected.

### When a name is available

| Situation | Name available? |
|-----------|----------------|
| Name never registered | ✅ Yes |
| Name registered but no version ever published | ✅ Yes (template is inert) |
| Name registered, latest version is deprecated | ✅ Yes |
| Name registered, latest version is **not** deprecated | ❌ No — `NameCollision` |

The check inspects the **latest published version** of every template ever registered under the name. If that version is non-deprecated, the name is blocked.

### Freeing a name

Deprecate every published version of every template that uses the name via
`deprecate_version`. Once the most-recently-published version is marked deprecated,
the name is available for a new registration.

```
register_template("Payroll")        → tid_a  (active)
publish_template_version(tid_a, …)  → v1     (not deprecated)
register_template("Payroll")        → NameCollision ❌

deprecate_version(tid_a, v1)        (all versions now deprecated)
register_template("Payroll")        → tid_b  ✅
```

### Collision-check algorithm

`register_template` consults an **append-only** index key
`TemplateNameIndex(name)` that stores the ordered list of all template IDs ever
registered under a given name. For each ID:

1. Read `TemplateLatest(id)`. If `0`, the template is versionless (inert) — skip.
2. Read `TemplateVersion(id, latest)`. If the record is not deprecated, reject with `NameCollision`.
3. If all IDs pass, write the new ID to the index and proceed with registration.

The index is append-only and written exclusively by `register_template` under
caller authentication. Entries are never removed, so the check is exhaustive
across all historical registrations.

---

## API overview

| Function | Purpose |
|----------|---------|
| `initialize` | One-time admin (deployer) setup. |
| `register_template` | Create a new `template_id` and display name. Rejects if an active template with the same name exists. |
| `publish_template_version` | Append a new immutable version (schema hash + notes). |
| `latest_version` | Return the highest published version number. |
| `get_version` | Load metadata for `(template_id, version)`. |
| `deprecate_version` | Mark a version deprecated; new agreements cannot use it. Frees the name once all versions are deprecated. |
| `create_agreement` | Create an agreement bound to a **non-deprecated** version. |
| `get_agreement` | Fetch agreement by id. |
| `get_templates_by_name` | Return all template IDs ever registered under a name (including deprecated). Useful for audit/history. |

---

## Migration when template structure changes

1. Publish a new version with a new `schema_hash` and migration notes describing field changes.
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
| 7 | `InvalidData` | Empty template name or agreement label |
| 8 | `AgreementNotFound` | No agreement for the given id |
| 9 | `NameCollision` | A non-deprecated template with this name already exists. Deprecate all its versions before re-registering. |

---

## Security notes

- Only the template **owner** can publish or deprecate versions.
- Deprecated versions cannot receive new `create_agreement` calls.
- Empty `label` or template `name` is rejected (`InvalidData`).
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
