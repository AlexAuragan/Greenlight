//! Shared request examples and actionable format-repair feedback. Never dispatches tools.
pub const SYSTEM: &str = include_str!("../docs/prompts/coding-agent-v1.md");
pub fn tool_example(tool: &str) -> Option<&'static str> {
    match tool {
        "read" => Some("action: tool\ntool: read\npath: path/to/file"),
        "write" => Some(
            "action: tool\ntool: write\npath: path/to/file\ncontent: |\n  Replacement file content",
        ),
        "shell" => Some("action: tool\ntool: shell\ninput: pwd"),
        "web" => Some("action: tool\ntool: web\ninput: search terms"),
        _ => None,
    }
}
pub fn tool_help(tools: &[String]) -> String {
    let mut text = format!("Workflow-enabled tool names: {}. These are NOT action names.\nUse action: tool plus a separate tool: NAME field. Host permissions, path checks and sandbox restrictions still apply.\n",if tools.is_empty(){"none".into()}else{tools.join(", ")});
    for tool in tools {
        if let Some(example) = tool_example(tool) {
            text.push_str(&format!("\nIllustrative {tool} syntax; substitute your intended fields:\n```yaml\n{example}\n```\n"));
        }
    }
    if tools.iter().any(|t|t=="read") {text.push_str("\nNative file discovery: read with path: . recursively lists project paths without shell. To find filenames/paths, add input: clock (literal case-sensitive substring). Read a returned file path separately to inspect its contents. Directory listings are bounded and report truncation; narrow path/input when needed.\n");}
    text
}
pub fn correction(error: &anyhow::Error, tools: &[String]) -> String {
    let detail=format!("{error:#}");
    let mut excerpt=detail.chars().take(1200).collect::<String>();
    if excerpt.len()<detail.len() {excerpt.push_str(" [error excerpt; original response remains recorded]");}
    format!("Invalid YAML action (protocol format error): {excerpt}\nThis response was rejected before tool dispatch; no tool was attempted. This is NOT evidence that tools are unavailable. Valid action names: tool, artifact, deviation, finish. Tool names read/write/shell/web must appear under tool:, never action:.\nRe-emit exactly one valid YAML action for your original intended operation, using its original arguments/path/content. Return no commentary or extra action blocks. For values containing colon-space or multiline text, use YAML literal block scalars such as input: | followed by the indented original text. Do not copy example fields blindly or bypass a denial.\n\n{}",tool_help(tools))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_prompt_example_is_a_separate_accepted_mapping() {
        let examples = SYSTEM
            .split("```yaml\n")
            .skip(1)
            .map(|text| text.split("```").next().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(examples.len(), 8);
        for example in examples {
            crate::tools::parse_action(example).unwrap();
        }
        for tool in ["read", "write", "shell", "web"] {
            let example = tool_example(tool).unwrap();
            assert!(SYSTEM.contains(example));
            let parsed = crate::tools::parse_action(example).unwrap();
            assert_eq!(parsed.action, "tool");
            assert_eq!(parsed.tool.as_deref(), Some(tool));
        }
    }
    #[test]
    fn context_examples_are_scoped_to_workflow_tools() {
        let help = tool_help(&["read".into()]);
        assert!(help.contains("tool: read"));
        assert!(!help.contains("tool: shell"));
        assert!(!help.contains("tool: write"));
        let help = tool_help(&[]);
        assert!(help.contains("tool names: none"));
        assert!(!help.contains("```yaml"));
    }
    #[test]
    fn tool_named_actions_remain_invalid_and_receive_specific_corrections() {
        for tool in ["read", "write", "shell", "web"] {
            let error = crate::tools::parse_action(&format!("action: {tool}")).unwrap_err();
            assert!(error.to_string().contains(tool_example(tool).unwrap()));
            let reply = correction(&error, &[tool.into()]);
            assert!(reply.contains("no tool was attempted"));
            assert!(reply.contains("NOT evidence that tools are unavailable"));
        }
    }
}
