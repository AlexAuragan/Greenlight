//! Optional model intent annotations. These never authorize work or establish outcomes.
use anyhow::{bail, Result};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Title(String);

impl Title {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn parse(raw: &str) -> Result<Self> {
        // Reject rather than silently reword an agent annotation. In particular, a
        // multiline value must not masquerade as a single-line factual card.
        if raw.chars().any(|c| {
            c.is_control()
                || matches!(c,
                    '\u{ad}' | '\u{61c}' | '\u{180e}' | '\u{200b}'..='\u{200f}'
                    | '\u{2028}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}')
        }) {
            bail!(
                "activity_title must be plain text without control or invisible format characters"
            );
        }
        let text = raw.trim();
        if !(1..=80).contains(&text.chars().count()) {
            bail!("activity_title must contain 1–80 Unicode characters after trimming");
        }
        Ok(Self(text.to_owned()))
    }
}

impl<'de> Deserialize<'de> for Title {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Value preserves YAML scalar types; String's YAML deserializer otherwise
        // accepts numbers and booleans by coercion.
        let value = serde_json::Value::deserialize(deserializer)?;
        let raw = value
            .as_str()
            .ok_or_else(|| serde::de::Error::custom("activity_title must be a string"))?;
        Self::parse(raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Annotation {
    pub model_event: i64,
    pub title: Title,
}

#[cfg(test)]
mod tests {
    use crate::tools::parse_action;
    use crate::{
        activity_cards::{Projection, Tone},
        store::Store,
    };

    fn annotation(model_event: i64, title: &str) -> String {
        serde_json::json!({"model_event": model_event, "title": title}).to_string()
    }

    #[test]
    fn projection_requires_matching_visible_response_and_keeps_claims_informational() {
        let source = "action: finish\nactivity_title: All tests passed\nsummary: Agent claim only";
        let valid = vec![
            (1, "work".into(), "model.output".into(), source.into()),
            (
                2,
                "work".into(),
                "activity.title".into(),
                annotation(1, "All tests passed"),
            ),
        ];
        let p = Projection::build(&valid);
        assert!(p.malformed.is_empty());
        assert_eq!(p.titles[&2].response, source);
        assert_eq!(p.cards[&2].title, "Agent intent · All tests passed");
        assert_eq!(p.cards[&2].tone, Tone::Info);
        assert!(p.cards[&2].summary.contains("not execution evidence"));
        // Historical raw output alone is not backfilled into an annotation.
        assert!(Projection::build(&valid[..1]).titles.is_empty());
        for (kind, step, body, reference, title) in [
            ("tool.result", "work", source, 1, "All tests passed"),
            ("model.output", "other", source, 1, "All tests passed"),
            ("model.output", "work", source, 99, "All tests passed"),
            ("model.output", "work", source, 1, "Changed claim"),
            (
                "model.output",
                "work",
                "action: finish",
                1,
                "All tests passed",
            ),
            (
                "model.output",
                "work",
                "action: invalid\nactivity_title: All tests passed",
                1,
                "All tests passed",
            ),
        ] {
            let history = vec![
                (1, step.into(), kind.into(), body.into()),
                (
                    2,
                    "work".into(),
                    "activity.title".into(),
                    annotation(reference, title),
                ),
            ];
            let p = Projection::build(&history);
            assert!(p.titles.is_empty());
            assert!(p.malformed.contains_key(&2));
        }
        let mut duplicate = valid.clone();
        duplicate.push((
            3,
            "work".into(),
            "activity.title".into(),
            annotation(1, "All tests passed"),
        ));
        let p = Projection::build(&duplicate);
        assert_eq!(p.titles.len(), 1);
        assert!(p.malformed[&3].contains("duplicate"));
        for raw in [
            "not json",
            r#"{"model_event":1,"title":42}"#,
            r#"{"model_event":1,"title":"bad\u001b"}"#,
            r#"{"model_event":1,"title":"All tests passed","verified":true}"#,
        ] {
            let history = vec![
                valid[0].clone(),
                (2, "work".into(), "activity.title".into(), raw.into()),
            ];
            assert!(Projection::build(&history).malformed.contains_key(&2));
        }
        let mut future = valid.clone();
        future[0].0 = 3;
        future[1].3 = annotation(3, "All tests passed");
        assert!(Projection::build(&future).malformed[&2].contains("order"));
    }

    #[test]
    fn title_links_cannot_cross_attempt_or_branch_boundaries() {
        for boundary in ["step.started", "branch.forked"] {
            let history = vec![
                (
                    1,
                    "work".into(),
                    "model.output".into(),
                    "action: finish\nactivity_title: Reviewing".into(),
                ),
                (2, "work".into(), boundary.into(), "boundary".into()),
                (
                    3,
                    "work".into(),
                    "activity.title".into(),
                    annotation(1, "Reviewing"),
                ),
            ];
            let p = Projection::build(&history);
            assert!(p.titles.is_empty());
            assert!(p.malformed[&3].contains("boundary"));
        }
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut s = store.create("demo", "Goal", "work").unwrap();
        let before = store
            .event(&s, Some("work"), "user.instruction", "before source")
            .unwrap();
        let source = store
            .event(
                &s,
                Some("work"),
                "model.output",
                "action: finish\nactivity_title: Reviewing",
            )
            .unwrap();
        let title = store
            .event(
                &s,
                Some("work"),
                "activity.title",
                &annotation(source, "Reviewing"),
            )
            .unwrap();
        store.fork(&mut s, title, "work").unwrap();
        let inherited = Projection::build(&store.history(&s).unwrap());
        assert!(inherited.malformed.is_empty());
        assert!(inherited.titles.contains_key(&title));
        store.fork(&mut s, before, "work").unwrap();
        assert!(Projection::build(&store.history(&s).unwrap())
            .titles
            .is_empty());
        let forged = store
            .event(
                &s,
                Some("work"),
                "activity.title",
                &annotation(source, "Reviewing"),
            )
            .unwrap();
        let p = Projection::build(&store.history(&s).unwrap());
        assert!(p.malformed[&forged].contains("branch-visible"));
    }

    #[test]
    fn titles_are_optional_single_action_metadata_with_strict_bounded_unicode_text() {
        for action in [
            "action: tool\ntool: read\npath: src/main.rs",
            "action: artifact\nname: report.md\ncontent: report",
            "action: deviation\ndeviation_id: blocked\nsummary: evidence",
            "action: finish\nsummary: done",
        ] {
            let legacy = parse_action(action).unwrap();
            assert!(legacy.activity_title.is_none());
            assert!(!serde_yaml::to_string(&legacy)
                .unwrap()
                .contains("activity_title"));
            let titled = parse_action(&format!(
                "```yaml\n{action}\nactivity_title: '  Exploring 漢字  '\n```"
            ))
            .unwrap();
            assert_eq!(titled.activity_title.unwrap().as_str(), "Exploring 漢字");
        }
        let max = "漢".repeat(80);
        assert!(parse_action(&format!("action: finish\nactivity_title: {max}")).is_ok());
        for value in [
            "''",
            "'   '",
            "true",
            "42",
            "[Exploring]",
            "{title: Exploring}",
            "\"first\\nsecond\"",
            "\"test\\tfile\"",
            "\"bad\\u001b[2J\"",
            "\"bad\\u0085\"",
            "\"bad\\u202e\"",
            "\"bad\\u200b\"",
        ] {
            assert!(
                parse_action(&format!("action: finish\nactivity_title: {value}")).is_err(),
                "{value}"
            );
        }
        assert!(parse_action(&format!("action: finish\nactivity_title: {max}x")).is_err());
        assert!(parse_action("action: title\nactivity_title: Exploring").is_err());
        assert!(parse_action("action: finish\ntitle: Exploring").is_err());
        assert!(
            parse_action("action: finish\nactivity_title: ok\nactivity_title: duplicate").is_err()
        );
        assert!(parse_action("action: finish\nactivity_title: null")
            .unwrap()
            .activity_title
            .is_none());
    }
}
