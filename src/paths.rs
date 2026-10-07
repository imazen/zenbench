//! Where zenbench keeps files outside the project's `.zenbench/` directory.
//!
//! - Auto-saved results and multi-process exchange files go under the cargo
//!   target directory (`<target>/zenbench/`), next to the benchmark binary,
//!   so `cargo clean` removes them with everything else the build produced.
//! - Cross-process locks go in a per-user cache directory, so every zenbench
//!   process of a user meets on one path whatever its `TMPDIR` says.
//!
//! Neither uses the system temp directory unless no home directory is known.

use std::path::{Path, PathBuf};

/// Per-user cache directory: `$XDG_CACHE_HOME/zenbench`, else
/// `$HOME/.cache/zenbench` (`%LOCALAPPDATA%\zenbench` on Windows). Falls back
/// to `<temp>/zenbench` only when none of those is set to an absolute path.
pub(crate) fn user_cache_dir() -> PathBuf {
    #[cfg(windows)]
    if let Some(dir) = absolute_env("LOCALAPPDATA") {
        return dir.join("zenbench");
    }
    if let Some(dir) = absolute_env("XDG_CACHE_HOME") {
        return dir.join("zenbench");
    }
    if let Some(home) = absolute_env("HOME") {
        return home.join(".cache").join("zenbench");
    }
    std::env::temp_dir().join("zenbench")
}

/// `<target>/zenbench` for the running benchmark binary: `$CARGO_TARGET_DIR`
/// when it is absolute, else the nearest ancestor of the executable holding
/// cargo's `CACHEDIR.TAG` (cargo writes one at the root of every target
/// directory). Falls back to `<user cache>/results` for a binary that does
/// not live under a cargo target directory.
pub(crate) fn target_zenbench_dir() -> PathBuf {
    if let Some(dir) = absolute_env("CARGO_TARGET_DIR") {
        return dir.join("zenbench");
    }
    if let Some(target) = std::env::current_exe()
        .ok()
        .and_then(|exe| cargo_target_of(&exe))
    {
        return target.join("zenbench");
    }
    user_cache_dir().join("results")
}

/// The cargo target directory containing `path`, found by its `CACHEDIR.TAG`.
fn cargo_target_of(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .skip(1)
        .find(|dir| dir.join("CACHEDIR.TAG").is_file())
        .map(Path::to_path_buf)
}

fn absolute_env(key: &str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os(key)?);
    path.is_absolute().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_target_dir_by_its_cachedir_tag() {
        // Fixture beside the test binary (inside target/), not in the temp dir.
        let exe_dir = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let root = exe_dir.join(format!("zenbench-paths-{}", std::process::id()));
        let deps = root.join("target").join("release").join("deps");
        std::fs::create_dir_all(&deps).unwrap();
        std::fs::write(
            root.join("target").join("CACHEDIR.TAG"),
            "Signature: 8a477f597d28d172789f06886806bc55\n",
        )
        .unwrap();
        let enclosing = cargo_target_of(&root).expect("test binary runs under a cargo target dir");
        let exe = deps.join("bench-0123abcd");
        // The nearest CACHEDIR.TAG wins…
        assert_eq!(cargo_target_of(&exe), Some(root.join("target")));
        // …and a path outside the fixture target falls through to the enclosing one.
        assert_eq!(
            cargo_target_of(&root.join("elsewhere").join("bin")),
            Some(enclosing)
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn this_test_binary_lives_in_a_cargo_target_dir() {
        // cargo runs unit tests from <target>/<profile>/deps/, so the
        // CACHEDIR.TAG walk must find a target dir for the test binary itself.
        let exe = std::env::current_exe().unwrap();
        assert!(
            cargo_target_of(&exe).is_some(),
            "no CACHEDIR.TAG above {}",
            exe.display()
        );
    }
}
