# Descriptor-anchored file access

GL-003 closes the gap between checking a path and later opening it. A concurrent directory/symlink swap must not redirect agent reads, source mutations or private state IO to another host location.

## Filesystem capabilities

`anchored_fs.rs` centralizes the Linux calls. It opens directory components relative to owned descriptors with `O_DIRECTORY`, `O_NOFOLLOW` and `O_CLOEXEC`, rejects absolute/parent-traversal paths inside a capability, and binds each file operation to its parent directory. Reads first pin an `O_PATH` handle, check that inode is a regular file, then acquire a readable descriptor through the kernel-owned `/proc/self/fd` link. A raced-in FIFO or device is not opened for reading. Directory iteration owns independent streams and does not follow links or collect an unbounded directory before applying caller budgets.

Model-visible paths keep their existing lexical/resolved privacy checks. Ordinary in-project aliases can resolve to an allowed project file before its descriptor-relative open; outside/private aliases fail. Restore paths and private state/configuration files are stricter and reject links. Host configuration directories containing symlinks may therefore require an explicit real directory path. Linux and a mounted `/proc/self/fd` are required; there is no unchecked portable fallback.

A directory descriptor authorizes the opened directory object, not a newly substituted pathname. Mutations create exclusive temporary files, write/fsync through their descriptors and publish relative to the same pinned parent. Published file identity is checked. Native writes/restores also check parent bindings around mutation; uncertainty preserves a native write intent or reports a partial restore. This guards path resolution, not hard-link provenance or arbitrary privileged filesystem tampering. Metadata checks do not make concurrent content edits or multi-file restoration transactional. Uncooperative processes with the same host privileges can still change authorized file contents; GL-007 coordination and ordinary drift/recovery checks remain relevant.

## Covered paths

- Native tool reads and recursive directory discovery.
- Workflow definitions, project instructions and prompt includes.
- Workspace hashes and manager project-source reads.
- Native-write snapshots, temporary files, replacement and recovery.
- Restore snapshots, replacement and deletion.
- Artifact/edit/restore/trace/checkpoint blobs and conversation exports.
- State initialization/migration, session/native-write/manager locks and approval read–merge–save operations.
- Pricing cache reads/publication.
- SQLite database, journal, WAL and shared-memory path prefixes.
- Bubblewrap project and cache bind sources and sensitive-path enumeration.

Private blob writers create new files rather than overwriting existing names. Existing trace/checkpoint hashes and artifact/session formats remain unchanged. Corrupt or linked blob paths fail instead of following an external target. User-selected exports publish atomically without clobbering an existing file/link.

## SQLite and sandbox integration

SQLite's default Unix `xFullPathname` resolves `/proc/self/fd/N` into its current ordinary pathname. A narrowly scoped VFS adapter preserves only the owned descriptor path ending in `history.db`; SQLite's existing Unix methods and final-component no-follow behavior handle the actual database and sidecars. The Store owns the state directory descriptor until after its database connection closes. The adapter is registered independently and does not replace the process-wide default VFS.

Bubblewrap receives owned project/cache directory descriptors through `--ro-bind-fd` and `--bind-fd`. Only those cloned descriptors lose close-on-exec in the child; private state/database descriptors are not passed. Unsupported Bubblewrap versions or unavailable user namespaces fail explicitly. Source access stays read-only, the cache stays private/writable, and host-home/network isolation is unchanged. Deferred sandbox usability work remains GL-027.

## Validation

Regression tests cover repeated parent swaps during real tool/include reads, pinned-parent replacement, linked/special files, private blob/database links, and state-directory replacement while SQLite performs new journaled writes. Outside marker files remain unchanged and private directory targets receive no redirected data. Sandbox argument/descriptor-lifetime tests verify that replacing the project pathname cannot change a command's bind source. Existing write-recovery, approval concurrency, restore drift, attempt/continuation and export tests remain in the full suite.

The optional live sandbox integration test cannot run in this cloud environment: Bubblewrap reports `setting up uid map: Read-only file system`. Descriptor-level tests still run; live isolation should be checked with `greenlight sandbox-check` on a host with enabled user namespaces and the required Bubblewrap options.
