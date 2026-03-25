+++
title = "zenbench"
description = "Interleaved microbenchmarking for Rust"
template = "landing.html"

[extra]
section_order = ["hero", "features", "easy_command", "final_cta"]

[extra.hero]
title = "zenbench"
subtitle = "Interleaved microbenchmarking for Rust with paired statistics, CI regression testing, and hardware-adaptive measurement."
cta_text = "Get Started"
cta_url = "/zenbench/getting-started/"
secondary_cta_text = "GitHub"
secondary_cta_url = "https://github.com/imazen/zenbench"

[extra.easy_command]
title = "Quick Start"
command = 'cargo add zenbench --dev'
desc = "Add to your project, create a bench file, run `cargo bench`."

[[extra.features]]
title = "Interleaved Execution"
desc = "Every round, all benchmarks run in shuffled order. Paired statistics on the differences detect changes that sequential harnesses miss."

[[extra.features]]
title = "CI Regression Testing"
desc = "`--save-baseline=main` and `--baseline=main` with exit codes. Auto-update on pass. Block PRs on performance regressions."

[[extra.features]]
title = "Criterion Migration"
desc = "Drop-in compatibility layer — change one import, zero code changes. Closures borrow freely, no 'static needed."

[[extra.features]]
title = "Rich Output"
desc = "Tree and table terminal displays. HTML reports with SVG charts. JSON, CSV, LLM, Markdown formats. Streaming per-group."

[[extra.features]]
title = "Hardware-Adaptive"
desc = "TSC timer, stack alignment jitter, overhead compensation, allocation profiling. Auto-calibration across platforms."

[[extra.features]]
title = "Cross-Platform"
desc = "Tested on Linux, Windows, macOS — x86_64, ARM64, and Intel. 6 CI targets with hardware timer support."

[extra.final_cta]
title = "Ready to benchmark?"
desc = "See the tutorial, check example output, or dive into the API docs."
buttons = [
    { text = "Tutorial", url = "/zenbench/getting-started/" },
    { text = "Example Report", url = "/zenbench/example-report.html" },
    { text = "API Docs", url = "https://docs.rs/zenbench" },
]
+++
