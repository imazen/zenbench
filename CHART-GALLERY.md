# QuickChart Gallery

Zenbench can generate [QuickChart](https://quickchart.io) URLs for embedding benchmark
charts directly in READMEs and docs. No API key needed — the Chart.js config is
URL-encoded inline.

```rust
use zenbench::quickchart::QuickChartConfig;

let config = QuickChartConfig {
    colors: vec![
        ("mozjpeg".into(), "#ff9800".into()),  // amber
        ("libjpeg".into(), "#2196f3".into()),  // blue
    ],
    ..Default::default()
};

// From benchmark results
let urls = suite.to_quickchart_urls(&config);
let markdown = suite.to_quickchart_markdown(&config);
```

## Flat chart (single dataset)

Each benchmark gets one bar. Fastest is phosphor green, others use custom or default gray.
Sorted fastest-first.

![JPEG Decode 4K](https://quickchart.io/chart?w=700&h=186&bkg=%23080808&f=png&c=%7B%22type%22%3A%22horizontalBar%22%2C%22data%22%3A%7B%22labels%22%3A%5B%22zenjpeg%22%2C%22libjpeg-turbo%22%2C%22mozjpeg%22%2C%22image%20crate%22%5D%2C%22datasets%22%3A%5B%7B%22data%22%3A%5B12.4%2C15.2%2C18.6%2C31.0%5D%2C%22backgroundColor%22%3A%5B%22%2300ff41%22%2C%22%232196f3%22%2C%22%23ff9800%22%2C%22%23666666%22%5D%7D%5D%7D%2C%22options%22%3A%7B%22layout%22%3A%7B%22padding%22%3A%7B%22top%22%3A0%2C%22bottom%22%3A0%2C%22left%22%3A0%2C%22right%22%3A4%7D%7D%2C%22plugins%22%3A%7B%22datalabels%22%3A%7B%22anchor%22%3A%22end%22%2C%22align%22%3A%22end%22%2C%22color%22%3A%22%23eeeeee%22%2C%22font%22%3A%7B%22weight%22%3A%22bold%22%2C%22size%22%3A20%7D%2C%22formatter%22%3A%22%28v%29%3D%3Ev%2B%27%20ms%27%22%7D%7D%2C%22scales%22%3A%7B%22xAxes%22%3A%5B%7B%22ticks%22%3A%7B%22beginAtZero%22%3Atrue%2C%22fontColor%22%3A%22%23999999%22%2C%22fontSize%22%3A18%2C%22padding%22%3A2%7D%2C%22gridLines%22%3A%7B%22color%22%3A%22%231a1a1a%22%2C%22zeroLineColor%22%3A%22%23333333%22%2C%22drawTicks%22%3Afalse%7D%7D%5D%2C%22yAxes%22%3A%5B%7B%22ticks%22%3A%7B%22fontColor%22%3A%22%23dddddd%22%2C%22fontSize%22%3A20%2C%22padding%22%3A6%7D%2C%22gridLines%22%3A%7B%22color%22%3A%22%23111111%22%2C%22drawTicks%22%3Afalse%7D%2C%22barPercentage%22%3A0.7%2C%22categoryPercentage%22%3A0.85%7D%5D%7D%2C%22legend%22%3A%7B%22display%22%3Afalse%7D%2C%22title%22%3A%7B%22display%22%3Atrue%2C%22fontColor%22%3A%22%2333ff66%22%2C%22fontSize%22%3A22%2C%22padding%22%3A6%2C%22text%22%3A%22JPEG%20Decode%204K%20%28ms%2C%20lower%20%3D%20better%29%22%7D%7D%7D)

## Throughput chart

When `Throughput` is set, values are throughput (GiB/s, MiB/s, etc.) and the chart
says "higher = better." Sorted highest-first.

![PNG Encode](https://quickchart.io/chart?w=700&h=154&bkg=%23080808&f=png&c=%7B%22type%22%3A%22horizontalBar%22%2C%22data%22%3A%7B%22labels%22%3A%5B%22libpng%22%2C%22image%20crate%22%2C%22zenpng%22%5D%2C%22datasets%22%3A%5B%7B%22data%22%3A%5B520.2%2C327%2C1.40%5D%2C%22backgroundColor%22%3A%5B%22%2300ff41%22%2C%22%23666666%22%2C%22%23666666%22%5D%7D%5D%7D%2C%22options%22%3A%7B%22layout%22%3A%7B%22padding%22%3A%7B%22top%22%3A0%2C%22bottom%22%3A0%2C%22left%22%3A0%2C%22right%22%3A4%7D%7D%2C%22plugins%22%3A%7B%22datalabels%22%3A%7B%22anchor%22%3A%22end%22%2C%22align%22%3A%22end%22%2C%22color%22%3A%22%23eeeeee%22%2C%22font%22%3A%7B%22weight%22%3A%22bold%22%2C%22size%22%3A20%7D%2C%22formatter%22%3A%22%28v%29%3D%3Ev%2B%27%20GiB%2Fs%27%22%7D%7D%2C%22scales%22%3A%7B%22xAxes%22%3A%5B%7B%22ticks%22%3A%7B%22beginAtZero%22%3Atrue%2C%22fontColor%22%3A%22%23999999%22%2C%22fontSize%22%3A18%2C%22padding%22%3A2%7D%2C%22gridLines%22%3A%7B%22color%22%3A%22%231a1a1a%22%2C%22zeroLineColor%22%3A%22%23333333%22%2C%22drawTicks%22%3Afalse%7D%7D%5D%2C%22yAxes%22%3A%5B%7B%22ticks%22%3A%7B%22fontColor%22%3A%22%23dddddd%22%2C%22fontSize%22%3A20%2C%22padding%22%3A6%7D%2C%22gridLines%22%3A%7B%22color%22%3A%22%23111111%22%2C%22drawTicks%22%3Afalse%7D%2C%22barPercentage%22%3A0.7%2C%22categoryPercentage%22%3A0.85%7D%5D%7D%2C%22legend%22%3A%7B%22display%22%3Afalse%7D%2C%22title%22%3A%7B%22display%22%3Atrue%2C%22fontColor%22%3A%22%2333ff66%22%2C%22fontSize%22%3A22%2C%22padding%22%3A6%2C%22text%22%3A%22PNG%20Encode%20%28GiB%2Fs%2C%20higher%20%3D%20better%29%22%7D%7D%7D)

## Grouped chart (matrix)

When benchmarks use `variant/param` naming (e.g., `BRAG8/256x256`), zenbench detects
the matrix structure and produces paired bars with a legend. Each variant gets its own
color from the palette.

![SrcOver Blend](https://quickchart.io/chart?w=700&h=278&bkg=%23080808&f=png&c=%7B%22type%22%3A%22horizontalBar%22%2C%22data%22%3A%7B%22labels%22%3A%5B%22256x256%22%2C%221024x1024%22%5D%2C%22datasets%22%3A%5B%7B%22label%22%3A%22BRAG8%22%2C%22data%22%3A%5B1.60%2C20.0%5D%2C%22backgroundColor%22%3A%22%2300ff41%22%7D%2C%7B%22label%22%3A%22naive%22%2C%22data%22%3A%5B13.0%2C89.0%5D%2C%22backgroundColor%22%3A%22%23007722%22%7D%2C%7B%22label%22%3A%22sw-composite%22%2C%22data%22%3A%5B6.00%2C29.0%5D%2C%22backgroundColor%22%3A%22%232196f3%22%7D%5D%7D%2C%22options%22%3A%7B%22layout%22%3A%7B%22padding%22%3A%7B%22top%22%3A0%2C%22bottom%22%3A0%2C%22left%22%3A0%2C%22right%22%3A4%7D%7D%2C%22plugins%22%3A%7B%22datalabels%22%3A%7B%22anchor%22%3A%22end%22%2C%22align%22%3A%22end%22%2C%22color%22%3A%22%23eeeeee%22%2C%22font%22%3A%7B%22weight%22%3A%22bold%22%2C%22size%22%3A18%7D%2C%22formatter%22%3A%22%28v%29%3D%3Ev%2B%27%20ms%27%22%7D%7D%2C%22scales%22%3A%7B%22xAxes%22%3A%5B%7B%22ticks%22%3A%7B%22beginAtZero%22%3Atrue%2C%22fontColor%22%3A%22%23999999%22%2C%22fontSize%22%3A18%2C%22padding%22%3A2%7D%2C%22gridLines%22%3A%7B%22color%22%3A%22%231a1a1a%22%2C%22zeroLineColor%22%3A%22%23333333%22%2C%22drawTicks%22%3Afalse%7D%7D%5D%2C%22yAxes%22%3A%5B%7B%22ticks%22%3A%7B%22fontColor%22%3A%22%23dddddd%22%2C%22fontSize%22%3A20%2C%22padding%22%3A6%7D%2C%22gridLines%22%3A%7B%22color%22%3A%22%23111111%22%2C%22drawTicks%22%3Afalse%7D%2C%22barPercentage%22%3A0.75%2C%22categoryPercentage%22%3A0.85%7D%5D%7D%2C%22legend%22%3A%7B%22display%22%3Atrue%2C%22position%22%3A%22bottom%22%2C%22labels%22%3A%7B%22fontColor%22%3A%22%23cccccc%22%2C%22fontSize%22%3A18%2C%22padding%22%3A8%7D%7D%2C%22title%22%3A%7B%22display%22%3Atrue%2C%22fontColor%22%3A%22%2333ff66%22%2C%22fontSize%22%3A22%2C%22padding%22%3A6%2C%22text%22%3A%22SrcOver%20Blend%20%28ms%2C%20lower%20%3D%20better%29%22%7D%7D%7D)

## Minimal chart (2 bars)

A simple A-vs-B comparison at 122px tall.

![Hash Function](https://quickchart.io/chart?w=700&h=122&bkg=%23080808&f=png&c=%7B%22type%22%3A%22horizontalBar%22%2C%22data%22%3A%7B%22labels%22%3A%5B%22xxhash%22%2C%22fnv%22%5D%2C%22datasets%22%3A%5B%7B%22data%22%3A%5B42.0%2C78.0%5D%2C%22backgroundColor%22%3A%5B%22%2300ff41%22%2C%22%23666666%22%5D%7D%5D%7D%2C%22options%22%3A%7B%22layout%22%3A%7B%22padding%22%3A%7B%22top%22%3A0%2C%22bottom%22%3A0%2C%22left%22%3A0%2C%22right%22%3A4%7D%7D%2C%22plugins%22%3A%7B%22datalabels%22%3A%7B%22anchor%22%3A%22end%22%2C%22align%22%3A%22end%22%2C%22color%22%3A%22%23eeeeee%22%2C%22font%22%3A%7B%22weight%22%3A%22bold%22%2C%22size%22%3A20%7D%2C%22formatter%22%3A%22%28v%29%3D%3Ev%2B%27%20ns%27%22%7D%7D%2C%22scales%22%3A%7B%22xAxes%22%3A%5B%7B%22ticks%22%3A%7B%22beginAtZero%22%3Atrue%2C%22fontColor%22%3A%22%23999999%22%2C%22fontSize%22%3A18%2C%22padding%22%3A2%7D%2C%22gridLines%22%3A%7B%22color%22%3A%22%231a1a1a%22%2C%22zeroLineColor%22%3A%22%23333333%22%2C%22drawTicks%22%3Afalse%7D%7D%5D%2C%22yAxes%22%3A%5B%7B%22ticks%22%3A%7B%22fontColor%22%3A%22%23dddddd%22%2C%22fontSize%22%3A20%2C%22padding%22%3A6%7D%2C%22gridLines%22%3A%7B%22color%22%3A%22%23111111%22%2C%22drawTicks%22%3Afalse%7D%2C%22barPercentage%22%3A0.7%2C%22categoryPercentage%22%3A0.85%7D%5D%7D%2C%22legend%22%3A%7B%22display%22%3Afalse%7D%2C%22title%22%3A%7B%22display%22%3Atrue%2C%22fontColor%22%3A%22%2333ff66%22%2C%22fontSize%22%3A22%2C%22padding%22%3A6%2C%22text%22%3A%22Hash%20Function%20%28ns%2C%20lower%20%3D%20better%29%22%7D%7D%7D)

## Color palette

| Role | Hex | Use |
|---|---|---|
| Winner | `#00ff41` | Phosphor green (fastest bar) |
| Winner (secondary) | `#00bb33` | Dimmer green |
| Grouped (2nd dataset) | `#007722` | Dark green |
| Competitor (fast) | `#ff9800` | Amber |
| Competitor (mid) | `#2196f3` | Blue |
| Baseline / slow | `#666666` | Gray (default) |

Assign custom colors by name substring:

```rust
let config = QuickChartConfig {
    colors: vec![
        ("mozjpeg".into(), "#ff9800".into()),
        ("libjpeg".into(), "#2196f3".into()),
    ],
    fastest_color: "#00ff41".into(),  // default
    default_color: "#666666".into(),  // default
    ..Default::default()
};
```

## Configuration

| Field | Default | Description |
|---|---|---|
| `width` | 700 | Chart width in pixels |
| `bar_height` | 32 | Per-bar height in pixels |
| `padding` | 58 | Fixed vertical padding (title + axes) |
| `format` | `"png"` | `"png"` or `"svg"` |
| `background` | `"080808"` | Background hex (no `#`) |
| `prefer_throughput` | `true` | Use throughput values when available |
| `colors` | `[]` | `Vec<(pattern, hex)>` for name matching |
| `fastest_color` | `"#00ff41"` | Color for the fastest bar |
| `default_color` | `"#666666"` | Color for unmatched bars |

## Regenerating these charts

```bash
cargo test print_demo_urls -- --ignored --nocapture
```
