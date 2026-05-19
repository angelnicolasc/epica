//! CSV + Markdown emitters for benchmark reports.
//!
//! Two output shapes:
//!
//! - Per-trajectory CSV: one row per trajectory, columns covering
//!   every field of [`PerTrajectory`]. This is what data analysts /
//!   plotting scripts consume.
//! - Aggregate Markdown table: human-readable summary, fits in a
//!   GitHub `README.md` and reads at a glance for a portfolio
//!   reviewer.

use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;

use crate::harness::SuiteReport;
use crate::metrics::LatencyStats;

/// Write `report.per_trajectory` as a CSV to `path`.
///
/// Format is plain CSV (RFC 4180), `,` separator, no quoting because
/// every column is numeric or a single-token slug. Header is the first
/// line.
pub fn write_per_trajectory_csv(
    report: &SuiteReport,
    path: &Path,
) -> std::io::Result<()> {
    let mut f = std::fs::File::create(path)?;
    writeln!(
        f,
        "suite,trajectory_id,steps,contradictions,tece,calibration_met,\
         soft_violations,hard_violations,critical_violations,\
         free_energy_mean,free_energy_samples,\
         latency_p50_us,latency_p95_us,latency_p99_us,latency_max_us"
    )?;
    for t in &report.per_trajectory {
        let mut buf: Vec<u64> = t.insert_latencies_us.clone();
        let lat = LatencyStats::from_samples(&mut buf);
        writeln!(
            f,
            "{suite},{tid},{steps},{contra},{tece},{cal},{soft},{hard},{crit},{fe},{fen},\
             {p50},{p95},{p99},{max}",
            suite = report.suite.slug(),
            tid = t.trajectory_id,
            steps = t.steps,
            contra = t.contradictions_detected,
            tece = render_optional_f32(t.tece),
            cal = t.calibration_target_met,
            soft = t.soft_violations,
            hard = t.hard_violations,
            crit = t.critical_violations,
            fe = render_optional_f64(t.free_energy_mean),
            fen = t.free_energy_samples,
            p50 = lat.p50,
            p95 = lat.p95,
            p99 = lat.p99,
            max = lat.max,
        )?;
    }
    Ok(())
}

/// Write a single-row CSV with the aggregate [`MetricSet`] for the
/// suite.
pub fn write_summary_csv(report: &SuiteReport, path: &Path) -> std::io::Result<()> {
    let mut f = std::fs::File::create(path)?;
    writeln!(
        f,
        "suite,trajectories,total_steps,total_contradictions,tece_mean,calibration_met_pct,\
         soft_violations,hard_violations,critical_violations,\
         free_energy_mean,free_energy_samples,\
         latency_samples,latency_mean_us,latency_p50_us,latency_p95_us,latency_p99_us,\
         latency_max_us,wall_clock_seconds"
    )?;
    let m = &report.metrics;
    let cal_pct = if m.trajectories == 0 {
        0.0
    } else {
        100.0 * m.calibration_met as f64 / m.trajectories as f64
    };
    writeln!(
        f,
        "{suite},{n},{steps},{contra},{tece},{cal:.2},{soft},{hard},{crit},{fe},{fen},\
         {ln},{lmean:.2},{p50},{p95},{p99},{lmax},{wall:.4}",
        suite = report.suite.slug(),
        n = m.trajectories,
        steps = m.total_steps,
        contra = m.total_contradictions,
        tece = render_optional_f32(m.tece),
        cal = cal_pct,
        soft = m.soft_violations,
        hard = m.hard_violations,
        crit = m.critical_violations,
        fe = render_optional_f64(m.free_energy_mean),
        fen = m.free_energy_samples,
        ln = m.insert_latency_us.samples,
        lmean = m.insert_latency_us.mean,
        p50 = m.insert_latency_us.p50,
        p95 = m.insert_latency_us.p95,
        p99 = m.insert_latency_us.p99,
        lmax = m.insert_latency_us.max,
        wall = report.wall_clock_seconds,
    )?;
    Ok(())
}

/// Render a human-readable Markdown summary for the suite.
pub fn render_markdown_summary(report: &SuiteReport) -> String {
    let m = &report.metrics;
    let cal_pct = if m.trajectories == 0 {
        0.0
    } else {
        100.0 * m.calibration_met as f64 / m.trajectories as f64
    };
    let mut s = String::new();
    let _ = writeln!(s, "## {} — {} trajectories", report.suite, m.trajectories);
    let _ = writeln!(s);
    let _ = writeln!(s, "| Metric | Value |");
    let _ = writeln!(s, "|---|---|");
    let _ = writeln!(
        s,
        "| BeliefShift (T-ECE) mean | {} |",
        render_optional_f32(m.tece)
    );
    let _ = writeln!(s, "| Calibration target met | {cal_pct:.1}% |");
    let _ = writeln!(s, "| Total operations | {} |", m.total_steps);
    let _ = writeln!(s, "| Total AGM contradictions | {} |", m.total_contradictions);
    let _ = writeln!(
        s,
        "| Contract violations (soft / hard / critical) | {} / {} / {} |",
        m.soft_violations, m.hard_violations, m.critical_violations
    );
    let _ = writeln!(
        s,
        "| Free energy mean (nats) | {} |",
        render_optional_f64(m.free_energy_mean)
    );
    let _ = writeln!(
        s,
        "| Insert latency p50 / p95 / p99 / max (µs) | {} / {} / {} / {} |",
        m.insert_latency_us.p50,
        m.insert_latency_us.p95,
        m.insert_latency_us.p99,
        m.insert_latency_us.max,
    );
    let _ = writeln!(s, "| Wall-clock | {:.3} s |", report.wall_clock_seconds);
    s
}

/// Write the markdown summary to a file.
pub fn write_markdown_summary(report: &SuiteReport, path: &Path) -> std::io::Result<()> {
    std::fs::write(path, render_markdown_summary(report))
}

fn render_optional_f32(v: Option<f32>) -> String {
    match v {
        Some(x) => format!("{x:.5}"),
        None => "NA".into(),
    }
}

fn render_optional_f64(v: Option<f64>) -> String {
    match v {
        Some(x) => format!("{x:.5}"),
        None => "NA".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{LatencyStats, MetricSet, PerTrajectory};
    use crate::traces::Suite;

    fn fake_report() -> SuiteReport {
        let per = vec![
            PerTrajectory {
                trajectory_id: 0,
                steps: 5,
                contradictions_detected: 1,
                tece: Some(0.05),
                calibration_target_met: true,
                free_energy_mean: Some(2.0),
                free_energy_samples: 5,
                insert_latencies_us: vec![10, 20, 30],
                ..Default::default()
            },
            PerTrajectory {
                trajectory_id: 1,
                steps: 6,
                contradictions_detected: 2,
                tece: Some(0.07),
                calibration_target_met: false,
                soft_violations: 1,
                free_energy_mean: Some(2.5),
                free_energy_samples: 6,
                insert_latencies_us: vec![15, 25, 35],
                ..Default::default()
            },
        ];
        let metrics = MetricSet::from_trajectories(&per);
        SuiteReport {
            suite: Suite::AlfworldLike,
            trajectories: 2,
            wall_clock_seconds: 0.123,
            per_trajectory: per,
            metrics,
        }
    }

    #[test]
    fn per_trajectory_csv_is_parseable() {
        let tmp = std::env::temp_dir().join("epica_bench_per.csv");
        let r = fake_report();
        write_per_trajectory_csv(&r, &tmp).unwrap();
        let body = std::fs::read_to_string(&tmp).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 3, "header + 2 rows");
        assert!(lines[0].starts_with("suite,trajectory_id"));
        assert!(lines[1].starts_with("alfworld_like,0"));
        assert!(lines[2].starts_with("alfworld_like,1"));
        let _ = std::fs::remove_file(tmp);
    }

    #[test]
    fn summary_csv_has_one_data_row() {
        let tmp = std::env::temp_dir().join("epica_bench_summary.csv");
        let r = fake_report();
        write_summary_csv(&r, &tmp).unwrap();
        let body = std::fs::read_to_string(&tmp).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 2);
        let _ = std::fs::remove_file(tmp);
    }

    #[test]
    fn markdown_summary_contains_each_metric() {
        let r = fake_report();
        let md = render_markdown_summary(&r);
        assert!(md.contains("BeliefShift"));
        assert!(md.contains("Contract violations"));
        assert!(md.contains("Free energy"));
        assert!(md.contains("Insert latency"));
        assert!(md.contains("Wall-clock"));
    }

    #[test]
    fn latency_stats_render_zero_when_empty() {
        // No latency samples at all → all zeros, no panic.
        let mut empty: Vec<u64> = Vec::new();
        let s = LatencyStats::from_samples(&mut empty);
        assert_eq!(s.p99, 0);
    }
}
