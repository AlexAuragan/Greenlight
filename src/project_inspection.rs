//! A deliberately small argv language, executed against a private filtered view.
//! Unsupported input remains an exact-command approval, never a prefix grant.
use anyhow::{bail, Context, Result};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

const MAX_FILE: u64 = 1024 * 1024;
const MAX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ENTRIES: usize = 2000;
const SHELL_SYNTAX: &str = r#"$`|&;<>()[]{}*?~\"#;

#[derive(Debug)]
pub(crate) struct Inspection {
    argv: Vec<String>,
    paths: Vec<PathBuf>,
    names_only: bool,
    recursive: bool,
}

fn allowed(path: &Path) -> bool {
    crate::file_discovery::path_in_scope(path)
        && !path
            .components()
            .any(|part| crate::sandbox::is_sensitive(&part.as_os_str().to_string_lossy()))
}

fn relative(root: &Path, argument: &str) -> Option<PathBuf> {
    if argument.is_empty() || argument == "-" {
        return None;
    }
    let path = Path::new(argument);
    let path = if path.is_absolute() {
        path.strip_prefix(root)
            .or_else(|_| path.strip_prefix("/workspace"))
            .ok()?
    } else {
        path
    };
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Normal(p) => result.push(p),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if result.components().count() > 32 || !allowed(&result) {
        return None;
    }
    Some(result)
}

/// Reject expansion/operators even when quoted: false negatives prompt, rather
/// than guessing shell semantics. shlex only removes ordinary argument quotes.
pub(crate) fn parse(root: &Path, command: &str) -> Option<Inspection> {
    if command.len() > 16 * 1024
        || command
            .chars()
            .any(|c| c.is_control() || SHELL_SYNTAX.contains(c))
    {
        return None;
    }
    let words = shlex::split(command)?;
    if words.len() > 128 {
        return None;
    }
    let name = words.first()?.as_str();
    let flags = match name {
        "pwd" => "",
        "ls" => "aA1Rdp",
        "cat" => "nbsvETA",
        "head" | "tail" => "qv",
        "wc" => "lwcmL",
        "grep" => "nHiIivwxlcrFEohs",
        "rg" => "nHiIvwxlcFs",
        "sort" => "nrufbdhV",
        "uniq" => "cdui",
        "cut" => "s",
        "diff" => "uqsr",
        "cmp" => "sl",
        _ => return None,
    };
    let program = format!("/usr/bin/{name}");
    if !Path::new(&program).is_file() {
        return None;
    }
    let mut options = Vec::new();
    let mut operands = Vec::new();
    let mut end = false;
    let mut index = 1;
    let mut recursive = false;
    while index < words.len() {
        let word = &words[index];
        if !end && word == "--" {
            end = true;
            index += 1;
            continue;
        }
        if !end && word.starts_with('-') {
            if word == "-" {
                return None;
            }
            let takes_number = matches!(name, "head" | "tail")
                && matches!(word.as_str(), "-n" | "-c")
                || name == "diff" && word == "-U"
                || name == "cmp" && word == "-n";
            let takes_selection = name == "cut" && matches!(word.as_str(), "-f" | "-b" | "-c");
            if takes_number || takes_selection || name == "cut" && word == "-d" {
                let value = words.get(index + 1)?;
                if takes_number
                    && (value.is_empty()
                        || !value.chars().all(|c| c.is_ascii_digit())
                        || value.parse::<u64>().ok()? > 1_000_000)
                {
                    return None;
                }
                if takes_selection
                    && (value.is_empty()
                        || !value
                            .chars()
                            .all(|c| c.is_ascii_digit() || matches!(c, ',' | '-')))
                {
                    return None;
                }
                if name == "cut" && word == "-d" && value.len() != 1 {
                    return None;
                }
                options.extend([word.clone(), value.clone()]);
                index += 2;
                continue;
            }
            if !word.starts_with("--")
                && word.len() > 1
                && word[1..].chars().all(|c| flags.contains(c) && c != ' ')
            {
                if matches!(name, "grep" | "diff") && word.contains('r')
                    || name == "ls" && word.contains('R')
                {
                    recursive = true;
                }
                options.push(word.clone());
                index += 1;
                continue;
            }
            return None;
        }
        operands.push(word.clone());
        index += 1;
    }
    let mut argv = vec![program];
    // No config/preprocessor/pager/environment arguments are accepted.
    if name == "rg" {
        argv.push("--no-config".into());
    }
    argv.extend(options);
    let pattern = if matches!(name, "grep" | "rg") {
        if operands.is_empty() {
            return None;
        }
        Some(operands.remove(0))
    } else {
        None
    };
    if name == "pwd" {
        return (words.len() == 1).then_some(Inspection {
            argv,
            paths: vec![],
            names_only: true,
            recursive: false,
        });
    }
    if operands.is_empty() && matches!(name, "ls" | "rg") {
        operands.push(".".into());
    }
    if operands.is_empty()
        || matches!(name, "diff" | "cmp") && operands.len() != 2
        || name == "uniq" && operands.len() != 1
    {
        return None;
    }
    // rg recurses by default; grep and diff recurse only through supported -r.
    recursive |= name == "rg";
    let mut paths = Vec::new();
    argv.push("--".into());
    if let Some(pattern) = pattern {
        argv.push(pattern);
    }
    for operand in operands {
        let path = relative(root, &operand)?;
        argv.push(if path.as_os_str().is_empty() {
            ".".into()
        } else {
            format!("./{}", path.to_str()?)
        });
        paths.push(path);
    }
    Some(Inspection {
        argv,
        paths,
        names_only: name == "ls",
        recursive,
    })
}

impl Inspection {
    pub(crate) fn argv(&self) -> &[String] {
        &self.argv
    }
    /// New private files only. Source reads are pinned, non-following directory
    /// capabilities; the child never receives access to the original project.
    pub fn snapshot(&self, root: &Path) -> Result<tempfile::TempDir> {
        let root = crate::sandbox::checked_project_root(root)?;
        let source = crate::anchored_fs::Dir::open(&root)?;
        let snapshot = tempfile::tempdir()?;
        let mut pending = self
            .paths
            .iter()
            .cloned()
            .map(|p| (p, 0usize))
            .collect::<Vec<_>>();
        let mut visited = std::collections::HashSet::new();
        let mut entries = 0;
        let mut bytes = 0;
        while let Some((path, depth)) = pending.pop() {
            if !visited.insert(path.clone()) {
                continue;
            }
            if !allowed(&path) {
                bail!("Inspection path is outside permitted project scope");
            }
            entries += 1;
            if entries > MAX_ENTRIES || depth > 16 {
                bail!("Inspection snapshot exceeds entry/depth limits; narrow the command or use an explicit user rerun");
            }
            let destination = snapshot.path().join(&path);
            if let Ok(directory) = source.open_dir(&path) {
                fs::create_dir_all(&destination)?;
                for entry in directory.entries()? {
                    entries += 1;
                    if entries > MAX_ENTRIES {
                        bail!("Inspection snapshot exceeds 2000 entries; narrow the command");
                    }
                    let entry = entry?;
                    let child = path.join(entry.name);
                    if !allowed(&child) {
                        continue;
                    }
                    if entry.regular || entry.directory && self.recursive {
                        pending.push((child, depth + 1));
                    } else if entry.directory {
                        fs::create_dir_all(snapshot.path().join(child))?;
                    }
                    // Symlinks, FIFOs, devices and sockets are never reproduced.
                }
                continue;
            }
            fs::create_dir_all(destination.parent().context("Inspection parent missing")?)?;
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)?;
            if self.names_only {
                source
                    .open_file(&path)
                    .context("Inspection operand is not a regular project file")?;
                continue;
            }
            let input = source.open_file(&path).context(
                "Inspection requires an existing regular file/directory without symlink traversal",
            )?;
            use std::os::unix::fs::MetadataExt;
            let before = input.metadata()?;
            if before.len() > MAX_FILE {
                bail!("Inspection file exceeds 1 MiB; narrow the request or use an explicit user rerun");
            }
            let mut content = Vec::new();
            (&input).take(MAX_FILE + 1).read_to_end(&mut content)?;
            let after = input.metadata()?;
            if content.len() as u64 > MAX_FILE
                || before.len() != after.len()
                || before.modified()? != after.modified()?
                || before.ctime() != after.ctime()
                || before.ctime_nsec() != after.ctime_nsec()
                || content.len() as u64 != after.len()
            {
                bail!("Inspection source changed while reading; retry explicitly");
            }
            bytes += content.len() as u64;
            if bytes > MAX_BYTES {
                bail!("Inspection snapshot exceeds 8 MiB; narrow the request");
            }
            output.write_all(&content)?;
        }
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_supported_argv_forms_can_bypass_exact_approval() {
        let root = tempfile::tempdir().unwrap();
        for command in [
            "pwd",
            "ls -a .",
            "cat -- -file",
            "head -n 20 src/main.rs",
            "tail -c 20 notes",
            "wc -l src/main.rs",
            "grep -n -F TODO src/main.rs",
            "sort -nr notes",
            "uniq -c notes",
            "cut -f 1 -d : notes",
            "diff -u a b",
            "cmp -s a b",
        ] {
            assert!(parse(root.path(), command).is_some(), "{command}");
        }
        for command in [
            "ls . | ./danger.sh",
            "cat a; ./danger.sh",
            "ls $(./danger.sh)",
            "ls `./danger.sh`",
            "cat <(./danger.sh)",
            "cat a > out",
            "cat a && ./danger.sh",
            "A=1 cat a",
            "sh -c 'ls'",
            "env cat a",
            "./cat a",
            "/usr/bin/cat a",
            "cat /etc/passwd",
            "cat ../secret",
            "cat .env",
            "cat .netrc",
            "cat credentials.json",
            "ls .config",
            "cat .git/config",
            "ls .greenlight",
            "sort -o out a",
            "uniq a out",
            "tail -f a",
            "grep -f /etc/passwd a",
            "rg --pre ./danger.sh src",
            "ls -L a",
            "cat -- -",
            "ls ''",
            "ls *",
            "cat ~/secret",
            "find . -exec danger ;",
            "awk 'BEGIN{system(1)}' a",
            "sed -i x a",
            "cat a\nls",
        ] {
            assert!(parse(root.path(), command).is_none(), "{command}");
        }
    }
    #[test]
    fn snapshot_is_pinned_filtered_and_unchanged_by_later_project_edits() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/file"), "original").unwrap();
        fs::write(root.path().join("src/.env"), "secret").unwrap();
        fs::write(root.path().join("src/.netrc"), "secret").unwrap();
        fs::write(root.path().join("src/credentials.json"), "secret").unwrap();
        fs::write(outside.path().join("secret"), "outside").unwrap();
        symlink(
            outside.path().join("secret"),
            root.path().join("src/escape"),
        )
        .unwrap();
        let plan = parse(root.path(), "grep -r original src").unwrap();
        let view = plan.snapshot(root.path()).unwrap();
        assert_eq!(
            fs::read_to_string(view.path().join("src/file")).unwrap(),
            "original"
        );
        assert!(!view.path().join("src/.env").exists());
        assert!(!view.path().join("src/.netrc").exists());
        assert!(!view.path().join("src/credentials.json").exists());
        assert!(!view.path().join("src/escape").exists());
        fs::remove_file(root.path().join("src/file")).unwrap();
        symlink(outside.path().join("secret"), root.path().join("src/file")).unwrap();
        assert_eq!(
            fs::read_to_string(view.path().join("src/file")).unwrap(),
            "original"
        );
        assert!(parse(root.path(), "cat src/file")
            .unwrap()
            .snapshot(root.path())
            .is_err());
        assert!(parse(root.path(), "ls src/file")
            .unwrap()
            .snapshot(root.path())
            .is_err());
    }
    #[test]
    fn names_only_listing_does_not_copy_file_bodies_and_content_is_bounded() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("big"), vec![b'x'; MAX_FILE as usize + 1]).unwrap();
        let listing = parse(root.path(), "ls .")
            .unwrap()
            .snapshot(root.path())
            .unwrap();
        assert_eq!(fs::metadata(listing.path().join("big")).unwrap().len(), 0);
        assert!(parse(root.path(), "cat big")
            .unwrap()
            .snapshot(root.path())
            .is_err());
    }
    #[test]
    fn paths_are_normalized_without_allowing_parent_traversal_or_private_aliases() {
        let root = tempfile::tempdir().unwrap();
        let plan = parse(
            root.path(),
            &format!("cat {}/src/file", root.path().display()),
        )
        .unwrap();
        assert_eq!(plan.argv.last().unwrap(), "./src/file");
        assert!(parse(root.path(), "cat /workspace/../etc/passwd").is_none());
        assert!(parse(root.path(), "cat src/../../secret").is_none());
        assert!(parse(root.path(), "cat .greenlight/config.yaml").is_none());
        let plan = parse(root.path(), "cat 'file with spaces'").unwrap();
        assert_eq!(plan.argv.last().unwrap(), "./file with spaces");
    }
    #[test]
    fn validated_argv_treats_pattern_and_filename_as_data_in_the_filtered_view() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("file with spaces"), "TODO: check\nother\n").unwrap();
        let plan = parse(root.path(), "grep -n -F TODO 'file with spaces'").unwrap();
        let view = plan.snapshot(root.path()).unwrap();
        // This test checks argv semantics only; production always adds Bubblewrap.
        let output = std::process::Command::new(&plan.argv[0])
            .args(&plan.argv[1..])
            .current_dir(view.path())
            .env_clear()
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "1:TODO: check\n");
        fs::write(root.path().join("-n"), "literal file").unwrap();
        let plan = parse(root.path(), "cat -- -n").unwrap();
        let view = plan.snapshot(root.path()).unwrap();
        let output = std::process::Command::new(&plan.argv[0])
            .args(&plan.argv[1..])
            .current_dir(view.path())
            .env_clear()
            .output()
            .unwrap();
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "literal file");
    }
    #[test]
    fn inspection_retains_the_sandbox_guard_against_machine_wide_project_roots() {
        let plan = parse(Path::new("/"), "cat etc/passwd").unwrap();
        assert!(plan
            .snapshot(Path::new("/"))
            .unwrap_err()
            .to_string()
            .contains("entire host filesystem"));
    }
}
