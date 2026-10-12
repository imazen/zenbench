#!/usr/bin/env python3
"""Compile the first-benchmark guide against this checkout, without measuring.

Invoke through the host resource wrapper. Uses Cargo's shared target directory
and an isolated generated package under target/; never modifies the user's crate.
"""
from pathlib import Path
import re
import subprocess
import os

root=Path(__file__).resolve().parents[2]
page=(root/'site/content/getting-started.md').read_text()
blocks=re.findall(r'```rust\n(.*?)\n```',page,re.S)
if len(blocks)!=1:
    raise SystemExit('Expected exactly one runnable first-benchmark Rust block')
example=root/'target/site-guide-check'
(example/'benches').mkdir(parents=True,exist_ok=True)
(example/'benches/my_bench.rs').write_text(blocks[0]+'\n')
(example/'Cargo.toml').write_text('''[package]
name = "zenbench-site-guide-check"
version = "0.0.0"
edition = "2024"
[workspace]
[dev-dependencies]
zenbench = { path = "../.." }
[[bench]]
name = "my_bench"
harness = false
''')
env=dict(os.environ,CARGO_TARGET_DIR=str(root/'target'))
subprocess.run(['cargo','check','--manifest-path',str(example/'Cargo.toml'),'--bench','my_bench'],env=env,check=True)
print('First Rust benchmark guide compiles against this checkout')
