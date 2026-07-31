//! Workspace task runner. Cargo has no post-build hook, so release packaging
//! lives here instead: `cargo dist` (aliased in .cargo/config.toml) builds the
//! release dll + launcher and zips them under a revision/hash stamped name.
//!
//!     cargo dist                      # public build
//!     cargo dist --features devmode   # extra args are forwarded to cargo build

include!("../../buildinfo.rs");

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    match dist() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn dist() -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("xtask must live one level below the workspace root")?;
    let extra: Vec<String> = std::env::args().skip(1).collect();

    // Build only the shippable crates -- `--workspace` would rebuild xtask itself.
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut build = std::process::Command::new(cargo);
    build
        .current_dir(root)
        .args(["build", "--release", "-p", "gw-run-timer", "-p", "launcher"])
        .args(&extra);
    let status = build
        .status()
        .map_err(|e| format!("failed to run cargo build: {e}"))?;
    if !status.success() {
        return Err("cargo build --release failed".to_string());
    }

    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    // .cargo/config.toml pins a target triple, which adds a directory level.
    let mut out_dir = target_dir.join(env!("BUILD_TARGET")).join("release");
    if !out_dir.exists() {
        out_dir = target_dir.join("release");
    }

    let artifacts = [out_dir.join("kaos_zone_timer.dll"), out_dir.join("kaos_zone_timer_launcher.exe")];
    for artifact in &artifacts {
        if !artifact.exists() {
            return Err(format!("expected artifact is missing: {}", artifact.display()));
        }
    }

    let (rev, hash, dirty) = git_info();
    let version = version_string(rev, &hash, dirty);

    let dist_dir = root.join("dist");
    std::fs::create_dir_all(&dist_dir).map_err(|e| format!("failed to create dist/: {e}"))?;
    let zip = dist_dir.join(format!("kaos_zone_timer-{version}.zip"));

    compress(&artifacts, &zip)?;
    println!("packaged {}", zip.display());
    Ok(())
}

/// Zips via PowerShell's Compress-Archive -- keeps xtask dependency-free, and
/// this workspace is Windows-only anyway.
fn compress(artifacts: &[std::path::PathBuf], zip: &Path) -> Result<(), String> {
    let paths = artifacts
        .iter()
        .map(|p| format!("'{}'", p.display()))
        .collect::<Vec<_>>()
        .join(",");
    let script = format!(
        "Compress-Archive -Path {paths} -DestinationPath '{}' -Force",
        zip.display()
    );

    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status()
        .map_err(|e| format!("failed to run powershell: {e}"))?;
    if !status.success() {
        return Err("Compress-Archive failed".to_string());
    }
    Ok(())
}
