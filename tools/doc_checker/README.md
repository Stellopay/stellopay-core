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

### Baseline (enforcing CI despite a pre-existing backlog)

CI runs the checker with `--strict` **and** a committed baseline,
`tools/doc_checker/baseline.json`, which records the violations that existed
when the check was made enforcing:

* A finding whose identity appears in the baseline only **warns**.
* A finding with **no** baseline entry **fails** the run. This is what keeps
  the baseline from growing silently: every new violation has a new identity
  and turns CI red.
* Identities are line-number-independent (`file | rule | item`), so fixing an
  unrelated violation above a baselined one does not re-fail CI.
* Entries that no longer match any finding are reported as stale warnings —
  the backlog shrank. They stay in the file until it is regenerated, so the
  shrink shows up as an explicit, reviewable diff.

The baseline is fail-closed: a finding message the checker cannot classify
into an identity is never recorded and never matches, so unknown finding
kinds always fail. A corrupt or unknown-version baseline file is also a hard
error, never a silent bypass.

Regenerate after fixing violations (or, deliberately, to add new ones):

```bash
cargo run --manifest-path tools/doc_checker/Cargo.toml -- \
  --strict --events --update-baseline
```

The `--baseline <PATH>` flag points at a specific baseline file; CI passes
`tools/doc_checker/baseline.json` via `run_ci.py`.

Run the checker exactly as CI does:

```bash
python3 tools/doc_checker/run_ci.py
```

## Tests

Internal verification of the `doc_checker` rules is available through standard `cargo` testing capabilities.

```bash
cargo test
```
