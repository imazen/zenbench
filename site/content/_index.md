+++
title = "zenbench"
description = "Interleaved microbenchmarking for Rust"
template = "landing.html"

[extra]
section_order = ["hero", "features", "easy_command", "final_cta"]

[extra.hero]
title = "zenbench"
description = "Interleaved microbenchmarking for Rust with paired statistics, CI regression testing, and hardware-adaptive measurement."
badge = "v0.1 — MIT / Apache-2.0"
cta_buttons = [
    { text = "Get Started", url = "/getting-started/", style = "primary" },
    { text = "GitHub", url = "https://github.com/imazen/zenbench", style = "secondary" },
]

[extra.easy_command_section]
title = "Quick Start"
description = "Add to your project, create a bench file, run `cargo bench`."
tabs = [
    { name = "Install", command = "cargo add zenbench --dev" },
    { name = "Run", command = "cargo bench" },
    { name = "Compare", command = "cargo bench -- --baseline=main" },
]

[[extra.features]]
title = "Interleaved Execution"
desc = "Every round, all benchmarks run in shuffled order. Paired statistics on the differences detect changes that sequential harnesses miss."
icon = "fa-solid fa-shuffle"

[[extra.features]]
title = "CI Regression Testing"
desc = "`--save-baseline=main` and `--baseline=main` with exit codes. Auto-update on pass. Block PRs on performance regressions."
icon = "fa-solid fa-shield"

[[extra.features]]
title = "Criterion Migration"
desc = "Drop-in compatibility layer — change one import, zero code changes. Closures borrow freely, no 'static needed."
icon = "fa-solid fa-arrow-right"

[[extra.features]]
title = "Rich Output"
desc = "Tree and table terminal displays. HTML reports with SVG charts. JSON, CSV, LLM, Markdown formats. Streaming per-group."
icon = "fa-solid fa-chart-bar"

[[extra.features]]
title = "Hardware-Adaptive"
desc = "TSC timer, stack alignment jitter, overhead compensation, allocation profiling. Auto-calibration across platforms."
icon = "fa-solid fa-microchip"

[[extra.features]]
title = "Cross-Platform"
desc = "Tested on Linux, Windows, macOS — x86_64 and ARM64. 5 CI targets with hardware timer support."
icon = "fa-solid fa-globe"

[extra.final_cta_section]
title = "Ready to benchmark?"
description = "See the tutorial, check example output, or dive into the API docs."
button = { text = "Tutorial", url = "/getting-started/" }
+++
