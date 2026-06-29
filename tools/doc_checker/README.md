# doc_checker

A simple linting tool that enforces documentation rules across Soroban smart contracts.

## Usage

```bash
cd tools/doc_checker
cargo run
```

By default, the checker statically analyzes all `.rs` files under `../../onchain/contracts` and flags any public function inside a `#[contractimpl]` that does not provide comprehensive Rustdoc comments. It verifies:
- Core docs are present
- `param` / `arguments` are documented (if any)
- `return` value is documented (if any)
- Access control / `require_auth` notes are present

### Event Documentation Rule

To enforce documentation parity for generated events, the `--events` (or `-e`) flag is available:

```bash
cargo run -- --events
```

When enabled, `doc_checker` additionally locates structs and enums annotated with `#[contracttype]` whose name acts as an event or payload (containing "Event" or "Payload"). It ensures that all structural fields and variants are documented with at least one doc comment (`///`). 

Undocumented events will result in errors detailing the specific struct/enum and missing field/variant.

### Undocumented public functions

In addition to the section-based checks above, the checker flags public
`#[contractimpl]` functions that have **no doc comment at all**. These are
reported as `... fn <name> has no doc comment at all`.

This rule is enabled by default and can be turned off with
`--no-undocumented-fns`.

### Undocumented error-enum variants

The checker also flags variants of `#[contracterror]` enums that lack a doc
comment, so each contract failure mode is described. These are reported as
`... error enum <Enum> variant <Variant> has no doc comment`.

This rule is enabled by default and can be turned off with `--no-error-enums`.
It is independent of the `--events` flag.

### Severity (incremental rollout)

The two newer rules (undocumented functions and error-enum variants) default to
**warnings**: they are printed but do not fail the run, allowing incremental
adoption. Pass `--strict` to promote every finding to an **error** that fails
the process with a non-zero exit code:

```bash
cargo run -- --strict
```

The original section-based function checks and event checks always fail the run.

## Tests

Internal verification of the `doc_checker` rules is available through standard `cargo` testing capabilities.

```bash
cargo test
```
