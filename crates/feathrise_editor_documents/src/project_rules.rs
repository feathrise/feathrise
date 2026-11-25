//! 保存和结构化表单走同一份工程规则校验。
pub(super) use feathrise_editor_documents_model::project_rules::validate;

#[cfg(test)]
mod tests {
    use super::validate;
    use feathrise_editor_documents_model::project_rules::{EMPTY, EXAMPLE};
    use serde_json::{Value, json};

    #[test]
    fn authors_can_start_empty_or_with_the_bundled_yarn_branch() {
        assert_eq!(validate(EMPTY), Ok(()));
        assert_eq!(validate(EXAMPLE), Ok(()));
        assert!(
            validate(&EXAMPLE.replace("\"set\": \"route\"", "\"set\": \"unknown\""))
                .unwrap_err()
                .contains("未声明")
        );
        assert!(validate(&EXAMPLE.replace("\"value\": 1", "\"value\": true")).is_err());
        assert!(validate(&EXAMPLE.replace("\"to\": \"smith\"", "\"to\": \"other\"")).is_err());
    }

    #[test]
    fn location_events_accept_valid_rooms_and_diagnose_invalid_bounds() {
        let mut file = json!({"version":1, "locations":{"east_gate":{"scene":"gate","min":[0,0,0],"max":[4,3,6]}},
            "rules":[{"id":"entered","when":{"event":"location.entered","target":"east_gate"},
                "actions":[{"request":"quest.start","target":"welcome"}]}]});
        assert_eq!(validate(&file.to_string()), Ok(()));
        for bad in [json!([0, 3, 6]), json!([1, 2]), Value::Null] {
            file["locations"]["east_gate"]["max"] = bad;
            assert!(validate(&file.to_string()).is_err());
        }
        assert!(validate(r#"{"version":1,"other":[]}"#).is_err());
    }
}
