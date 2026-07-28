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

### Orphaned `docs/*.md` files

The checker also builds a link-reachability graph over the repository's
documentation and flags any `docs/*.md` file that is not reachable from it,
reported as `docs/<path>.md: orphaned doc - not reachable from README.md or
any docs index file`.

The graph is seeded from:
- the repository root `README.md`, and
- every `README.md` found anywhere under `docs/` (e.g. `docs/README.md`,
  `docs/api/README.md`, `docs/best-practices/README.md`, ...), treated as a
  documentation index in its own right even if the top-level README does not
  (yet) link to it.

Starting from those entry points, the checker follows markdown link targets
(`[text](target)`) that resolve to a local file, resolving each link relative
to the directory of the file that contains it. External links (`http://`,
`https://`, `mailto:`, `file://`), pure same-page anchors (`#section`), and
already-visited files are not followed further, so link cycles terminate
safely. Any `docs/*.md` file left unvisited once the graph is fully explored
is reported as orphaned — it exists on disk but a reader browsing the docs
top-down via README links would never find it.

This rule is enabled by default and can be turned off with
`--no-orphaned-docs`. It shares the same severity as the other newer checks
(see below), so it is possible to introduce new orphaned docs without
immediately failing CI while the backlog of pre-existing orphaned files (if
any) is cleaned up.

**Security notes**: this check only reads files already committed to the
repository — it makes no network requests and never treats a link target as
anything other than a relative filesystem path to resolve and read. A link
target that does not resolve to an existing file is simply not traversed
further (fails closed); it cannot be used to escape `repo_root` into an
"always reachable" result, since only files that exist on disk are ever
inserted into the reachable set.

### Severity (incremental rollout)

The three newer rules (undocumented functions, error-enum variants, and
orphaned `docs/*.md` files) default to **warnings**: they are printed but do
not fail the run, allowing incremental adoption. Pass `--strict` to promote
every finding to an **error** that fails the process with a non-zero exit
code:

```bash
cargo run -- --strict
```

The original section-based function checks and event checks always fail the run.

## Tests

Internal verification of the `doc_checker` rules is available through standard `cargo` testing capabilities.

```bash
cargo test
```
