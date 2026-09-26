use std::collections::BTreeMap;

const EVENT_SOURCE: &str = include_str!("../src/events.rs");
const SCHEMA_FIXTURE: &str = include_str!("../../../../docs/event-schema-v1.fixture");

fn snake_case(name: &str) -> String {
    let mut result = String::new();
    for (index, character) in name.chars().enumerate() {
        if character.is_ascii_uppercase() && index > 0 {
            result.push('_');
        }
        result.push(character.to_ascii_lowercase());
    }
    result
}

fn fixture_entries() -> BTreeMap<String, (String, String)> {
    let mut entries = BTreeMap::new();
    for line in SCHEMA_FIXTURE
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let parts: Vec<_> = line.split('|').collect();
        assert_eq!(parts.len(), 3, "malformed event schema fixture row: {line}");
        assert!(
            entries
                .insert(
                    parts[0].to_owned(),
                    (parts[1].to_owned(), parts[2].to_owned())
                )
                .is_none(),
            "duplicate fixture entry for {}",
            parts[0]
        );
    }
    entries
}

#[test]
fn every_contract_event_matches_its_versioned_schema_fixture() {
    let fixture = fixture_entries();
    let mut seen = BTreeMap::new();
    let lines: Vec<_> = EVENT_SOURCE.lines().collect();
    let mut index = 0;

    while index < lines.len() {
        let attribute = lines[index].trim();
        if !attribute.starts_with("#[contractevent") {
            index += 1;
            continue;
        }

        let mut declaration = index + 1;
        while declaration < lines.len() {
            if lines[declaration].trim_start().starts_with("pub struct ") {
                break;
            }
            declaration += 1;
        }
        assert!(
            declaration < lines.len(),
            "contractevent attribute has no struct declaration"
        );
        let struct_line = lines[declaration].trim();
        let name = struct_line
            .strip_prefix("pub struct ")
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .trim_end_matches('{');
        let expected_topic = format!("\"{}\"", snake_case(name));
        assert!(
            attribute.contains(&expected_topic),
            "{name} must preserve its event-name topic"
        );
        assert!(
            attribute.contains("\"event_schema_v1\""),
            "{name} is missing the schema-version topic"
        );

        let mut fields = Vec::new();
        let mut field_line = declaration + 1;
        while field_line < lines.len() && lines[field_line].trim() != "}" {
            let field = lines[field_line].trim();
            if let Some(field) = field.strip_prefix("pub ") {
                let (field_name, field_type) = field
                    .trim_end_matches(',')
                    .split_once(':')
                    .unwrap_or_else(|| panic!("unparseable field in {name}: {field}"));
                fields.push(format!("{}:{}", field_name.trim(), field_type.trim()));
            }
            field_line += 1;
        }
        assert!(field_line < lines.len(), "unterminated event struct {name}");
        let shape = fields.join(",");
        let (fixture_shape, fixture_version) = fixture
            .get(name)
            .unwrap_or_else(|| panic!("missing fixture for event {name}"));
        assert_eq!(
            &shape, fixture_shape,
            "payload shape changed for {name}; review the fixture and version policy"
        );
        assert_eq!(
            fixture_version, "event_schema_v1",
            "fixture version drift for {name}"
        );
        assert!(
            seen.insert(name.to_owned(), ()).is_none(),
            "duplicate event declaration {name}"
        );
        index = field_line + 1;
    }

    assert_eq!(
        seen.len(),
        fixture.len(),
        "fixture must cover every contract event and no unknown event"
    );
}
