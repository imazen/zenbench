# Installing the CLI

The `zenbench` Cargo package contains both the library and the `zenbench` CLI.
The default `cli` feature includes `pair`, `nice`, the CPU launcher, and offline
workflow guides. No separate source checkout is needed to read those guides
after installation.

The new commands are merged into `main` and currently unreleased on crates.io.
Install the current Git version with:

```sh
cargo install --git https://github.com/imazen/zenbench --locked --bin zenbench
zenbench --help
zenbench guide --topic pair
```

For a local checkout:

```sh
cargo install --path . --locked --bin zenbench
```

The crates.io installation command is `cargo install zenbench --locked --bin
zenbench`; it installs the published version, which may not include unreleased
commands. This change does not publish a release. A release must pass the
project's full validation and release review first.

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
