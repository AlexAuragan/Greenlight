//! Dependency-free, line-based unified diffs for native UTF-8 file writes.
const CONTEXT: usize = 3;
// Keep the shortest-edit-path trace bounded for unusually divergent files.
const MAX_EDIT_DISTANCE: usize = 2_048;

#[derive(Clone, Copy)]
enum Edit<'a> {
    Equal(&'a str),
    Delete(&'a str),
    Insert(&'a str),
}

struct Frontier {
    depth: usize,
    xs: Vec<usize>,
}

impl Frontier {
    fn get(&self, diagonal: isize) -> usize {
        self.xs[((diagonal + self.depth as isize) / 2) as usize]
    }
}

fn backtrack<'a>(old: &[&'a str], new: &[&'a str], trace: &[Frontier]) -> Vec<Edit<'a>> {
    let mut x = old.len();
    let mut y = new.len();
    let mut reversed = Vec::new();
    for depth in (1..trace.len()).rev() {
        let diagonal = x as isize - y as isize;
        let previous = &trace[depth - 1];
        let down = diagonal == -(depth as isize)
            || (diagonal != depth as isize
                && previous.get(diagonal - 1) < previous.get(diagonal + 1));
        let previous_diagonal = if down { diagonal + 1 } else { diagonal - 1 };
        let previous_x = previous.get(previous_diagonal);
        let previous_y = (previous_x as isize - previous_diagonal) as usize;
        while x > previous_x && y > previous_y {
            reversed.push(Edit::Equal(old[x - 1]));
            x -= 1;
            y -= 1;
        }
        if down {
            reversed.push(Edit::Insert(new[y - 1]));
            y -= 1;
        } else {
            reversed.push(Edit::Delete(old[x - 1]));
            x -= 1;
        }
    }
    while x > 0 && y > 0 {
        reversed.push(Edit::Equal(old[x - 1]));
        x -= 1;
        y -= 1;
    }
    reversed.reverse();
    reversed
}

// Correct, memory-bounded fallback for exceptionally large edit distances.
fn replacement<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<Edit<'a>> {
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
    let mut result = Vec::with_capacity(old.len() + new.len());
    result.extend(old[..prefix].iter().copied().map(Edit::Equal));
    result.extend(
        old[prefix..old.len() - suffix]
            .iter()
            .copied()
            .map(Edit::Delete),
    );
    result.extend(
        new[prefix..new.len() - suffix]
            .iter()
            .copied()
            .map(Edit::Insert),
    );
    result.extend(
        old[old.len() - suffix..]
            .iter()
            .copied()
            .map(Edit::Equal),
    );
    result
}

fn edit_script<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<Edit<'a>> {
    let max_depth = old
        .len()
        .saturating_add(new.len())
        .min(MAX_EDIT_DISTANCE);
    let mut trace: Vec<Frontier> = Vec::new();
    for depth in 0..=max_depth {
        let mut xs = Vec::with_capacity(depth + 1);
        for index in 0..=depth {
            let diagonal = (2 * index) as isize - depth as isize;
            let mut x = if depth == 0 {
                0
            } else {
                let previous = &trace[depth - 1];
                if diagonal == -(depth as isize)
                    || (diagonal != depth as isize
                        && previous.get(diagonal - 1) < previous.get(diagonal + 1))
                {
                    previous.get(diagonal + 1)
                } else {
                    previous.get(diagonal - 1) + 1
                }
            };
            let mut y = x as isize - diagonal;
            while x < old.len()
                && y >= 0
                && y < new.len() as isize
                && old[x] == new[y as usize]
            {
                x += 1;
                y += 1;
            }
            xs.push(x);
            if x == old.len() && y == new.len() as isize {
                trace.push(Frontier { depth, xs });
                return backtrack(old, new, &trace);
            }
        }
        trace.push(Frontier { depth, xs });
    }
    replacement(old, new)
}

fn emit_line(out: &mut String, marker: char, line: &str) {
    out.push(marker);
    out.push_str(line);
    if !line.ends_with('\n') {
        out.push('\n');
        out.push_str("\\ No newline at end of file\n");
    }
}

/// Generate unified hunks with up to three lines of surrounding context.
/// The original newline terminators are retained to represent EOF-only changes.
pub fn unified(path: &str, before: Option<&str>, after: &str) -> String {
    if before == Some(after) {
        return format!("(no line changes in {path})\n");
    }
    let old: Vec<&str> = before.unwrap_or("").split_inclusive('\n').collect();
    let new: Vec<&str> = after.split_inclusive('\n').collect();
    let script = edit_script(&old, &new);
    let original = if before.is_some() {
        format!("a/{path}")
    } else {
        String::from("/dev/null")
    };
    let mut out = format!("--- {original}\n+++ b/{path}\n");
    let mut hunks: Vec<(usize, usize)> = Vec::new();
    for (index, edit) in script.iter().enumerate() {
        if matches!(edit, Edit::Equal(_)) {
            continue;
        }
        let start = index.saturating_sub(CONTEXT);
        let end = index.saturating_add(CONTEXT + 1).min(script.len());
        if let Some((_, last_end)) = hunks.last_mut() {
            if start <= *last_end {
                *last_end = (*last_end).max(end);
                continue;
            }
        }
        hunks.push((start, end));
    }
    let mut old_at = vec![0; script.len() + 1];
    let mut new_at = vec![0; script.len() + 1];
    for (index, edit) in script.iter().enumerate() {
        old_at[index + 1] = old_at[index] + usize::from(!matches!(edit, Edit::Insert(_)));
        new_at[index + 1] = new_at[index] + usize::from(!matches!(edit, Edit::Delete(_)));
    }
    for (start, end) in hunks {
        let old_count = old_at[end] - old_at[start];
        let new_count = new_at[end] - new_at[start];
        let old_start = old_at[start] + usize::from(old_count > 0);
        let new_start = new_at[start] + usize::from(new_count > 0);
        out.push_str(&format!(
            "@@ -{old_start},{old_count} +{new_start},{new_count} @@\n"
        ));
        for edit in &script[start..end] {
            match *edit {
                Edit::Equal(line) => emit_line(&mut out, ' ', line),
                Edit::Delete(line) => emit_line(&mut out, '-', line),
                Edit::Insert(line) => emit_line(&mut out, '+', line),
            }
        }
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
        assert_eq!(diff.matches("@@ -").count(), 1);
    }

    #[test]
    fn new_file() {
        let diff = unified("new.txt", None, "hello\n");
        assert!(diff.contains("--- /dev/null"));
        assert!(diff.contains("@@ -0,0 +1,1 @@"));
        assert!(diff.contains("+hello\n"));
    }

    #[test]
    fn no_change() {
        assert!(unified("a", Some("same\n"), "same\n").contains("no line changes"));
    }

    #[test]
    fn separated_edits_produce_two_hunks() {
        let before = (1..=24).map(|n| format!("line {n}\n")).collect::<String>();
        let after = before
            .replace("line 3\n", "edited 3\n")
            .replace("line 22\n", "edited 22\n");
        let diff = unified("file.txt", Some(&before), &after);
        assert_eq!(diff.matches("@@ -").count(), 2, "{diff}");
        assert!(diff.contains("@@ -1,6 +1,6 @@"));
        assert!(diff.contains("@@ -19,6 +19,6 @@"));
        assert!(!diff.contains(" line 12\n"), "{diff}");
    }

    #[test]
    fn close_edits_share_one_hunk() {
        let before = (1..=20).map(|n| format!("line {n}\n")).collect::<String>();
        let after = before
            .replace("line 8\n", "edited 8\n")
            .replace("line 14\n", "edited 14\n");
        let diff = unified("file.txt", Some(&before), &after);
        assert_eq!(diff.matches("@@ -").count(), 1, "{diff}");
        assert!(diff.contains("-line 8\n+edited 8\n"));
        assert!(diff.contains("-line 14\n+edited 14\n"));
    }

    #[test]
    fn insertion_and_deletion_have_correct_ranges() {
        let inserted = unified("a", Some("one\ntwo\n"), "one\nmiddle\ntwo\n");
        assert!(inserted.contains("@@ -1,2 +1,3 @@"), "{inserted}");
        assert!(inserted.contains("+middle\n"));
        let deleted = unified("a", Some("one\nmiddle\ntwo\n"), "one\ntwo\n");
        assert!(deleted.contains("@@ -1,3 +1,2 @@"), "{deleted}");
        assert!(deleted.contains("-middle\n"));
    }

    #[test]
    fn missing_final_newline_is_preserved() {
        let diff = unified("file.txt", Some("last\n"), "last");
        assert!(diff.contains("-last\n+last\n\\ No newline at end of file\n"), "{diff}");
    }

    #[test]
    fn repeated_lines_remain_aligned() {
        let before = "alpha\nsame\nsame\nbeta\n";
        let after = "alpha\nsame\nupdated\nbeta\n";
        let diff = unified("file.txt", Some(before), after);
        assert_eq!(diff.matches("@@ -").count(), 1);
        assert!(diff.contains("-same\n+updated\n"), "{diff}");
        assert!(diff.contains(" beta\n"));
    }

    #[test]
    fn empty_creation_shows_file_headers() {
        let diff = unified("empty.txt", None, "");
        assert_eq!(diff, "--- /dev/null\n+++ b/empty.txt\n");
    }
}
