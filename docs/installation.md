# Installing the CLI

The `zenbench` Cargo package contains both the library and the `zenbench` CLI.
The default `cli` feature includes `pair`, `nice`, the CPU launcher, and offline
workflow guides. No separate source checkout is needed to read those guides
after installation.

The new commands are currently unreleased. Install the review stack with:

```sh
cargo install --git https://github.com/imazen/zenbench --branch review/16-external-pair --locked --bin zenbench
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
should use a native ARM64 Rust toolchain. Platform CPU-class restrictions still
apply: Windows ARM64 currently cannot certify P/E placement, even when the CLI
builds and runs. Use explicit unverified exploration for pair testing there.

Linux `nice` requires a working systemd user manager, `nice`, `ionice`, `ps` and
`systemctl`; it fails rather than dropping its hard memory ceiling. macOS uses
`nice` and `ps`. Windows supervision uses native Job Objects. No Python runtime
is required for normal CLI operation.

`just install-check` installs into `target/install-check` and exercises the
installed binary's help and offline guides. The version-control checkout and
installed binary are separate: reinstall after code changes before testing the
installed command.
