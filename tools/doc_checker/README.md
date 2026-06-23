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

## Tests

Internal verification of the `doc_checker` rules is available through standard `cargo` testing capabilities.

```bash
cargo test
```
