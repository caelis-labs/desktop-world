# Managed package lifecycle

The installer owns this CLI. Agent desktop tasks still use only `apps` and
`exec({app,source})`; no package/session management appears in those tools.

## Installer Agent update workflow

Use the same installation/update prompt each time. Resolve the current host,
profile and its DTW registration automatically. Read the official latest stable
release once, and bind installer/host instructions to that exact release tag; never ask the user to choose a version, package ID, filesystem
path or cleanup command. Do not update other hosts as a side effect.

1. Inspect this host's actual executable/plugin path and its matching Skill.
   Read its recorded pin where present. A PATH command or remembered version is
   not proof of the host's runtime. Already-current verified bytes need only the
   live connection check and owned cleanup; do not create another installation or restart it.
   If this host already has a pending update, resume that transaction first:
   reuse its staged package/registration and perform the outstanding host reload
   and verification. Do not repeat downloads, configuration discovery or binding.
2. Download and verify the new release as one complete package. Run the new
   package's own executable for import; use its returned installed `executable`
   for subsequent management. This also
   upgrades the manager itself; do not depend on an old PATH executable exposing
   new package commands. Outputs distinguish `manager_version`, package `version`
   and `store_schema`. An unsupported format fails before changing references.
3. Reuse this host/profile's pin, stage the new package, finish outstanding work,
   and update this host's complete plugin/Skill/MCP registration. Establish the
   host's supported reload boundary before probing tools: an external config
   edit, a UI inventory refresh and a new conversation are not plugin reloads.
   Use a documented reload for this plugin, or leave pending state and request
   one normal host restart. Do not kill shared processes or reverse engineer
   host internals to avoid that boundary. Shared distribution bytes stay immutable.
4. After the host has loaded the new registration, reconnect and check the actual new process
   path/version, Skill, tools and read-only observation. Only then activate. A
   pending failure protects both the previous current and the intended new
   package. If the host requires a full restart, preserve pending state and give
   that specific remaining action; report prepared/awaiting restart, never an
   installed or verified upgrade. A fresh verifier refreshes a conversation's
   cached schema after host reload; it cannot reload the host itself. A standalone
   MCP handshake verifies the package, not the host's live connection.
5. Activation performs owned cleanup automatically, retaining one rollback,
   other hosts' pins and live old runtimes. Remove this invocation's owned
   download/extraction staging after successful import. The installer may retire
   this host's previous external package only after verifying exact package
   ownership/inventory, zero live consumers and zero remaining configuration
   references; keep it until the new version has passed verification. This is
   normal removal of the previous host-owned installation, not store adoption.
   Do not sweep unrelated installations, user files, Core bundles or other hosts'
   plugin directories. If ownership or sharing is unproven, retain and report it
   rather than asking the user to choose versions or deleting it blindly.

DSH's encoded Cordis URL follows the exact immutable package. agy's fixed plugin
slot must be replaced as a complete stopped plugin; its host owns that copy's
removal. After host reload, a stale installation conversation can use one fresh
verifier without asking the user to manage versions. Final reporting needs only the old
and new version, actual connection validation and any unavoidable host action.

The commands below document the installer's mechanism, not user prerequisites.

A distribution package is immutable and addressed by SHA-256 of its complete
`SHA256SUMS`. Import verifies every member, BUILD binary/Skill identity and all
plugin versions before publishing a staged copy. Links/reparse points, unsafe
paths, case collisions, unlisted files and unsupported store formats are rejected.
The source ZIP must first pass its official release transport checksum. Import
never executes imported content and does not edit a host's configuration.

Default stores are `~/Library/Application Support/DesktopWorld` on macOS and
`%LOCALAPPDATA%\DesktopWorld` on Windows. `--store` accepts an absolute dedicated
directory. Existing nonempty unmanaged directories are refused. Paths may contain
spaces/Unicode; use the returned encoded `cordis_url`, not a hand-built file URL.

```sh
dtw package import "/absolute/verified/package"
dtw package bind dsh-desktop <package_id>
```

`bind` creates a pending reference and returns the exact executable, Skill root,
plugin source and Cordis URL. Host/profile names use lowercase ASCII letters,
digits, dot, dash or underscore; Windows device names are rejected.

1. Finish active operations in that host/profile. Its supported plugin reload or
   normal restart drains old connections; do not kill a shared bridge manually.
2. Update its one complete plugin registration. DSH uses the recipe's Cordis URL,
   bundling executable and Skill. agy owns one plugin slot: install the complete
   `agy_source` using its normal plugin route, replacing its entire stopped slot.
3. Apply the host's supported reload/restart, then check actual `apps`/`exec`,
   the bundled Skill and the running executable path/version. Use one fresh
   conversation/verifier only if this conversation still caches the old schema.
   A green plugin card alone is insufficient.
4. Complete activation:

```sh
dtw package activate dsh-desktop
dtw package status
```

Activation promotes pending → current and retains the previous current as one
rollback reference. It collects unpinned distributions older than seven days.
Use `bind` with the rollback package ID and the same drain/configure/verify steps
to roll back. A failed or unfinished setup leaves old current plus new pending
protected. Atomic host-file replacement never removes the original first.

agy and DSH can share an identical imported package. Upgrading `dsh-desktop`
does not repoint `agy`; no global mutable `current` symlink is used. Separate
model connections need no separate package copies. Host-owned plugin copies
remain under their host's installer/removal control.

```sh
dtw package gc             # preview
dtw package gc --apply     # collect only owned, unreferenced, old packages
```

Collection retains current, pending and rollback references for every host, and
uses cross-process locks to retain live managed runtimes and their native
workers, including a worker that outlives its supervisor. It never kills a
consumer. Rename/removal refusals are reported as `blocked`; Windows executable
locks are preserved. Owned stale staging/trash entries are also collected after
seven days. Unknown temporary entries are left alone. A shared store lock
serializes imports, bindings, startup lease admission and collection.

To remove a host installation, stop its connections and remove its DTW plugin or
MCP/Skill registration first, then `dtw package unbind <host-profile>`. Later GC
can collect its unreferenced distributions. This store does not adopt, migrate
or delete standalone installations, source checkouts, other hosts' plugin
directories or Core's embedded packages.
