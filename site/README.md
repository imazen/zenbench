# Website development

This is a Zola site using [Goyo](https://github.com/hahwul/goyo). The Pages workflow
builds with Zola 0.19.2 and fetches the theme's current default branch into the
ignored `themes/goyo` directory. Keep a local theme checkout there when building.
The published project prefix is `/zenbench`; Goyo's literal favicon URLs use the
same prefix in `config.toml`.

From this directory:

```sh
just check
just check-external
just browser
just plans /path/to/prebuilt/zenbench
```

`check` builds and verifies rendered local links, resource paths and anchors,
including the Pages prefix. `check-external` also checks external links through
Zola. `browser` requires Python Playwright with Chromium installed; it tests the
built pages at desktop, tablet and narrow mobile widths, keyboard navigation,
code copying, script loading and document overflow. It serves local artifacts
through intercepted browser requests and makes no external browser requests.
`plans` passes the published external JSON plan and all extractor/correctness
objects through a supplied CLI's dry-run, retaining the parser output without
executing workloads or building Rust. It substitutes only local executable/input
paths for parsing; this does not validate real program timing or correctness.
Use `ZOLA=/path/to/zola` to select a local binary. Browser scratch defaults to
`~/tmp`; override `TMPDIR` with another disk-backed location if needed.

Guides describe the current Git implementation, with an unreleased-version note
on the guide index and tutorial. Verify flags against `src/bin/zenbench.rs`,
benchmark flags against `src/plan.rs`/`src/bench-help.txt`, and external plan fields
against `src/bin/support/pair/plan.rs` before changing command examples. Keep native
suite decisions, external pair decisions and descriptive fleet observations clear.

The root `_index.md` sorts sidebar pages by weight. Use Zola `@/page.md` links for
content pages/anchors; the build resolves and checks them. The small project
navigation/header overrides retain the Goyo layout and native keyboard controls.
