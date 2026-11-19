# Native file discovery

Native `read` is confined to the selected project root, normally the folder containing `.greenlight/`. Absolute paths, parent traversal and symlinks resolving outside that root are rejected. The model cannot choose another root through `path`; legacy/explicit root selection is described in [project setup](22-installation-and-projects.md).

An enabled `read` tool can list directories as well as read files. This reuses the current read policy: shell permission is unnecessary, `read: deny` prevents discovery, and a workflow without read cannot use it. Existing project/workflow files need no migration or replacement.

Start with the project root instead of guessing filenames:

```yaml
action: tool
tool: read
path: .
```

To narrow a listing, select a subdirectory and/or supply a literal case-sensitive substring of the project-relative path:

```yaml
action: tool
tool: read
path: widgets
input: bar
```

The result is JSON containing `directory`, `filter`, `scope`, `entries` (project-relative `path` and file/directory `kind`), `truncated`, `unreadable_entries` and `limits`. Returned entries are sorted; a truncated listing is a bounded subset, not a complete index. This searches **filenames/paths**, not file contents, and uses no glob/regular-expression or shell syntax. Read a returned file path separately for its contents. Supplying a directory filter to a file read produces an explicit error.

Directory traversal does not read/hash file contents or follow symlinks. It excludes Greenlight/legacy control directories, private settings, known credential paths/file extensions and the build/cache exclusions used by workspace recording. Resolved paths are checked again before exposure. Excluded entries are not named in error details; these exclusions are not comprehensive secret detection, and normal read authorization still matters. Direct file reads retain their existing sensitive-path and resolved-symlink checks.

A listing visits at most 10,000 entries, returns at most 500 matches, descends at most 16 levels below the requested directory, and stays within 24,000 output bytes. Limits, unreadable/non-UTF-8 paths and incomplete traversal are reported; narrow the directory/filter when needed. An empty listing does not establish that a file is absent outside this scope. No new model-turn limit is introduced.

Regular file reads accept UTF-8 only and retain the 100,000-byte limit. Size is checked before loading, and the actual read is capped to handle growth after that check. Non-regular files are rejected. Native writes still reject the project root and retain their existing journal and approval guards.

The embedded coding prompt and generated step tool help describe discovery even for existing initialized projects. Original raw model output remains in conversation dumps; successful directory reads are normal correlated `tool.request` / `tool.result` records under tool `read`.
