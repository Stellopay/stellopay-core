# Contract event schema and compatibility policy

The `stello_pay_contract` event catalog is defined in `onchain/contracts/stello_pay_contract/src/events.rs`. Every `#[contractevent]` declaration keeps its existing event-name topic at topic index 0 and adds `event_schema_v1` at topic index 1. Consumers can continue dispatching on the existing first topic and inspect the second topic before decoding the payload.

## Compatibility rules

- Adding an optional data-map field is additive. Keep the current schema topic; consumers should ignore fields they do not recognize.
- Removing a field, changing its Soroban type, changing a topic's position or type, or changing the meaning of an existing field is breaking. Bump the schema topic to `event_schema_v2` for the affected event and publish the updated fixture after review.
- A new event starts at `event_schema_v1` and must be added to the fixture.
- Never reuse a schema version for a breaking payload change.

The exact current event field names, order, and Soroban types are recorded in `docs/event-schema-v1.fixture`. `event_schema_fixtures.rs` compares the fixture with every contract event declaration. CI runs this test with the workspace test suite; removed or retyped fields, untracked events, and missing version topics fail the check. Any payload edit requires an explicit fixture update, making additive changes reviewable; breaking changes additionally require a version bump under this policy.
