//! Small, dependency-free unified diff for native UTF-8 file writes.
//! Keeps unchanged context and treats the edited middle as one hunk.
pub fn unified(path: &str, before: Option<&str>, after: &str) -> String {
    let old: Vec<&str> = before.unwrap_or("").lines().collect();
    let new: Vec<&str> = after.lines().collect();
    let mut prefix = 0;
    while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < old.len() - prefix
        && suffix < new.len() - prefix
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }
    if old == new && before.is_some() {
        return format!("(no line changes in {path})\n");
    }
    let left = prefix.saturating_sub(3);
    let old_end = (old.len() - suffix + 3).min(old.len());
    let new_end = (new.len() - suffix + 3).min(new.len());
    let old_start_line = if old_end == left { left } else { left + 1 };
    let new_start_line = if new_end == left { left } else { left + 1 };
    let original = if before.is_some() {
        format!("a/{path}")
    } else {
        String::from("/dev/null")
    };
    let mut out = format!(
        "--- {original}\n+++ b/{path}\n@@ -{},{} +{},{} @@\n",
        old_start_line,
        old_end - left,
        new_start_line,
        new_end - left
    );
    for line in &old[left..prefix] { out.push_str(&format!(" {line}\n")); }
    for line in &old[prefix..old.len() - suffix] { out.push_str(&format!("-{line}\n")); }
    for line in &new[prefix..new.len() - suffix] { out.push_str(&format!("+{line}\n")); }
    for line in &old[old.len() - suffix..old_end] { out.push_str(&format!(" {line}\n")); }
    if before.is_some_and(|s| !s.is_empty() && !s.ends_with('\n'))
        || (!after.is_empty() && !after.ends_with('\n')) {
        out.push_str("\\ No newline at end of file (one or both versions)\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::unified;
    #[test]
    fn edit_and_context() {
        let diff = unified("calculator.py", Some("a\nb\nc\n"), "a\nx\nc\n");
        assert!(diff.contains("-b\n+x\n"));
        assert!(diff.contains(" a\n"));
    }
    #[test]
    fn new_file() {
        let diff = unified("new.txt", None, "hello\n");
        assert!(diff.contains("--- /dev/null"));
        assert!(diff.contains("+hello"));
    }
    #[test]
    fn no_change() {
        assert!(unified("a", Some("same\n"), "same\n").contains("no line changes"));
    }
}
