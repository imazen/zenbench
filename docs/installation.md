# Installing the CLI

The `zenbench` Cargo package contains both the library and the `zenbench` CLI.
The default `cli` feature includes `pair`, `nice`, the CPU launcher, and offline
workflow guides. No separate source checkout is needed to read those guides
after installation.

Install the 0.1.11 CLI with:

```sh
cargo install zenbench --version 0.1.11 --locked --bin zenbench
zenbench --help
zenbench guide
```

The CLI is optional for [writing a Cargo benchmark](https://docs.rs/zenbench/latest/zenbench/).
Install it when you need retained-result tools, external-program comparisons,
or CPU launch control.

To work from a source checkout instead:

```sh
cargo install --path . --locked --bin zenbench
```

For development versions, use `cargo install --git
https://github.com/imazen/zenbench --locked --bin zenbench`. Pin `--rev` when
you need to reproduce a specific Git build.

Cargo installs binaries into `$CARGO_HOME/bin` (normally `~/.cargo/bin`); ensure
that directory is on PATH. Windows uses `zenbench.exe`. The package supports Rust
1.85 or newer. `cargo install` builds for the machine where it runs; Windows ARM64
should use a native ARM64 Rust toolchain. Native Windows ARM64 discovers OS
core/cache classes and enforces a selected class with an inherited Job boundary
on supported single-group topology. Unknown topology and multiple processor
groups still fail closed. Inspect `zenbench cpu-info` before choosing a class.

Linux `nice` requires a working systemd user manager, `nice`, `ionice`, `ps` and
`systemctl`; it fails rather than dropping its hard memory ceiling. macOS uses
`nice` and `ps`. Windows supervision uses native Job Objects. No Python runtime
is required for normal CLI operation.

`just install-check` installs into `target/install-check` and exercises the
installed binary's help and offline guides. The version-control checkout and
installed binary are separate: reinstall after code changes before testing the
installed command.
