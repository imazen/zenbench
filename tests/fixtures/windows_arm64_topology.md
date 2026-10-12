# Windows ARM64 topology fixture

`windows_arm64_topology.hex` is the hexadecimal encoding of the 2,408 bytes
returned by `GetLogicalProcessorInformationEx(RelationAll)` on a Snapdragon
X Elite X1E78100, Windows 11 Pro 10.0.26200, on 2026-10-12. It contains CPU/cache
relationships only. Capture through the independent PowerShell oracle:

```powershell
& ./examples/windows_topology_probe.ps1 -CompileOnly
[Convert]::ToBase64String([ZenbenchWindowsTopologyProbe]::Read())
```

Decode the base64 transport and encode those bytes as hexadecimal. The oracle's
interpreted observations are recorded in
[the native control evidence](../../benchmarks/winarm_native_controls_2026-10-12.json).
The parser tests require classes 0 (CPUs 0–3) and 1 (CPUs 4–11), each with 12 MiB
last-level cache. An additional mutation holds the performance ranks equal while
changing one cache domain's size, verifying that cache asymmetry remains visible.
Malformed lengths, duplicate cores, truncated records and multiple groups refuse.

The parser is shared by host-independent tests and native Windows ARM64
discovery. The approved FFI module only acquires an aligned byte buffer; checked
slices parse it. Native CI compares discovery with the independent PowerShell
oracle and exercises class selection and pre-existing Rayon worker confinement.
