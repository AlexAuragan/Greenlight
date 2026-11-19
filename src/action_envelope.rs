//! Extract one explicit response envelope, never guess an action from prose.
use anyhow::{bail, Result};

pub fn yaml(text: &str) -> Result<&str> {
    let mut text = text.trim();
    // A leading orphan closing marker can be adjacent to the explicit fence.
    // Only remove known closing markers at the start, never a thinking body.
    while let Some(rest) = ["</think>", "</analysis>", "</reasoning>"]
        .iter()
        .find_map(|tag| text.strip_prefix(tag))
    {
        text = rest.trim_start();
    }
    let mut lines = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        lines.push((offset, line.trim_end_matches(['\r', '\n'])));
        offset += line.len();
    }
    let mut opening = None;
    let mut closing = None;
    for (i, (_, line)) in lines.iter().enumerate() {
        if let Some((_, width)) = opening {
            let delimiter = line.trim_end();
            let is_closing = delimiter.len() >= width && delimiter.bytes().all(|byte| byte == b'`');
            if closing.is_none() && is_closing {
                closing = Some(i);
            } else if closing.is_some() && fence_start(line).is_some() {
                bail!("multiple fenced blocks are ambiguous; return exactly one YAML action");
            }
            // Indented Markdown fences inside YAML literal content are data.
        } else if let Some((width, language)) = fence_start(line) {
            if !matches!(language, "" | "yaml" | "yml") {
                bail!("the action fence must be yaml or yml, not {language}");
            }
            opening = Some((i, width));
        }
    }
    let Some((start, _)) = opening else {
        return Ok(text);
    };
    let end = closing.ok_or_else(|| anyhow::anyhow!("missing closing YAML fence"))?;
    let (body_start, _) = lines
        .get(start + 1)
        .ok_or_else(|| anyhow::anyhow!("empty action fence"))?;
    let body_end = lines[end].0;
    let prefix = &text[..lines[start].0];
    let suffix_start = lines.get(end + 1).map(|l| l.0).unwrap_or(text.len());
    let suffix = &text[suffix_start..];
    for tag in ["think", "analysis", "reasoning"] {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let mut depth = 0usize;
        let mut cursor = prefix;
        loop {
            match (cursor.find(&open), cursor.find(&close)) {
                (Some(a), Some(b)) if a < b => {
                    depth += 1;
                    cursor = &cursor[a + open.len()..];
                }
                (_, Some(b)) => {
                    depth = depth.saturating_sub(1);
                    cursor = &cursor[b + close.len()..];
                }
                (Some(a), None) => {
                    depth += 1;
                    cursor = &cursor[a + open.len()..];
                }
                (None, None) => break,
            }
        }
        if depth > 0 {
            bail!("an action inside an unfinished {tag} block is not an explicit action response");
        }
    }
    for outside in [prefix, suffix] {
        if outside.lines().any(|line| {
            let line = line.trim_start().trim_start_matches("- ");
            line.split_once(':')
                .is_some_and(|(key, _)| key.trim().trim_matches(['\'', '"']) == "action")
                || line.starts_with('{')
                || line.starts_with('[')
                || line.starts_with("---")
        }) || serde_yaml::from_str::<serde_yaml::Value>(outside.trim())
            .is_ok_and(|v| v.get("action").is_some())
        {
            bail!("action-like content outside the fence is ambiguous; return exactly one action");
        }
    }
    Ok(&text[*body_start..body_end])
}
fn fence_start(line: &str) -> Option<(usize, &str)> {
    let width = line.bytes().take_while(|b| *b == b'`').count();
    (width >= 3).then(|| (width, line[width..].trim()))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::parse_action;
    #[test]
    fn adjacent_orphan_thinking_closer_preserves_explicit_action() {
        assert_eq!(
            parse_action("</think>```yaml\naction: tool\ntool: read\npath: widgets/bar.py\n```")
                .unwrap()
                .path
                .as_deref(),
            Some("widgets/bar.py")
        );
        assert!(parse_action("<think>```yaml\naction: finish\n```").is_err());
    }
    #[test]
    fn unambiguous_wrappers_are_accepted_without_changing_action_content() {
        let raw="I will inspect the source.</think>\n```yaml\naction: tool\ntool: read\npath: widgets/bar.py\n```\nThen report the findings.";
        assert_eq!(
            parse_action(raw).unwrap().path.as_deref(),
            Some("widgets/bar.py")
        );
        assert!(
            parse_action("<think>Consider options.</think>\n```yml\naction: finish\n```").is_ok()
        );
        let raw="```yaml\naction: artifact\nname: evidence.md\ncontent: |\n  ```python\n  print('example')\n  ```\n```";
        assert_eq!(
            parse_action(raw).unwrap().content.as_deref(),
            Some("```python\nprint('example')\n```\n")
        );
    }
    #[test]
    fn multiple_actions_thought_fences_and_unclosed_fences_are_never_guessed() {
        for raw in [
            "```yaml\naction: finish\n```\n```yaml\naction: tool\ntool: shell\ninput: pwd\n```",
            "action: finish\n```yaml\naction: finish\n```",
            "```yaml\naction: finish\n```\naction: finish",
            "<think>\n```yaml\naction: finish\n```\n</think>",
            "<think><think>nested</think>\n```yaml\naction: finish\n```\n</think>",
            "<analysis>\n```yaml\naction: finish\n```\n</analysis>",
            "```yaml\naction: finish\n```\naction : finish",
            "```yaml\naction: finish\n```\n- action: finish",
            "```yaml\naction: finish",
            "```python\nprint('not an action')\n```",
            "Explain first\naction: finish",
            "action: finish\naction: finish",
            "action: finish\n---\naction: finish",
        ] {
            assert!(parse_action(raw).is_err(), "accepted {raw}");
        }
        assert!(yaml("action: finish").is_ok());
    }
}
