# Fleet deployment

`zenbench capabilities` emits schema-1 JSON with the installed executable's
SHA-256, version, process architecture, reported machine architecture, CPU
classes, and placement result. A failed placement check is retained as an error;
a successful capability probe is not permission to measure. The reported machine
architecture can fall back to process architecture: `native_arch_verified:false`
means a worker must independently check native versus emulated execution.

Build once per target/toolchain/features combination. Package only explicit
executables, libraries, symbols and inputs:

```json
{"schema":1,"target":"x86_64-unknown-linux-gnu","plan":{"input":"data/input"},
 "files":{"bin/metric":{"source":"target/release/metric","executable":true},
          "data/input":{"source":"inputs/reference","executable":false}}}
```

```sh
zenbench bundle bundle-spec.json deployed-bundle
zenbench bundle-check deployed-bundle
```

Source paths resolve beside the specification. Destination paths are portable
relative paths; the manifest contains hashes, lengths and executable flags, not
source paths. Copy the directory using the fleet's existing content store.
Verify it on arrival. Preserve executable permissions when transporting it.
The bundle manifest's SHA-256 binds its target, logical plan and file identities.
Changing the staging directory does not change that identity. Unlisted dynamic
libraries, drivers and OS services remain part of the separately recorded testbed.
A failed bundle creation retains its partial directory and never overwrites one.

The scheduler owns worker selection, leases, retries and storage. A benchmark
identity must include the intended machine/testbed, CPU domain and planned
replicate. Retry attempts remain separate evidence. Do not collapse measurements
across machines or select the fastest retry. See the
[heap and fleet contract](heap-and-fleet-design.md) for the measurement semantics
and remaining implementation gates.
