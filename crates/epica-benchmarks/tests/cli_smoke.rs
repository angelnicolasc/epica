//! Smoke tests for the `epica-bench` binary.
//!
//! Exercises the actual CLI surface (argv parsing, exit codes, file
//! emission) against a temp directory, with a small number of
//! trajectories so the run is fast.

use std::path::PathBuf;
use std::process::Command;

fn binary_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_epica-bench"))
}

#[test]
fn run_alfworld_smoke_produces_outputs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let out = Command::new(binary_path())
        .args(["run", "--suite", "alfworld_like", "--trajectories", "4"])
        .arg("--out-dir")
        .arg(tmp.path())
        .output()
        .expect("invoke");
    assert!(
        out.status.success(),
        "epica-bench exited non-zero: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    for f in [
        "alfworld_like_per_trajectory.csv",
        "alfworld_like_summary.csv",
        "alfworld_like.md",
    ] {
        assert!(tmp.path().join(f).exists(), "missing artefact: {f}");
    }
}

#[test]
fn run_all_emits_both_suites() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let out = Command::new(binary_path())
        .args(["run-all", "--trajectories", "3"])
        .arg("--out-dir")
        .arg(tmp.path())
        .output()
        .expect("invoke");
    assert!(
        out.status.success(),
        "epica-bench run-all failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    for suite in ["alfworld_like", "webshop_like"] {
        for ext in ["per_trajectory.csv", "summary.csv"] {
            assert!(tmp
                .path()
                .join(format!("{suite}_{ext}"))
                .exists());
        }
        assert!(tmp.path().join(format!("{suite}.md")).exists());
    }
}

#[test]
fn per_trajectory_csv_has_header_plus_rows() {
    let tmp = tempfile::tempdir().expect("tempdir");
    Command::new(binary_path())
        .args(["run", "--suite", "webshop_like", "--trajectories", "5"])
        .arg("--out-dir")
        .arg(tmp.path())
        .status()
        .unwrap();
    let path = tmp.path().join("webshop_like_per_trajectory.csv");
    let body = std::fs::read_to_string(&path).unwrap();
    let n_lines = body.lines().count();
    assert_eq!(n_lines, 6, "header + 5 trajectory rows");
    assert!(body.lines().next().unwrap().starts_with("suite,trajectory_id"));
}

#[test]
fn no_active_inference_flag_disables_fe_metric() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let out = Command::new(binary_path())
        .args([
            "run",
            "--suite",
            "alfworld_like",
            "--trajectories",
            "3",
            "--no-active-inference",
        ])
        .arg("--out-dir")
        .arg(tmp.path())
        .output()
        .expect("invoke");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Markdown should report "NA" for free energy when disabled.
    assert!(
        stdout.contains("Free energy mean (nats) | NA |"),
        "expected NA free energy in markdown stdout, got: {stdout}"
    );
}
