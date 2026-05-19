//! Integration tests for the Sprint-2 active-inference hook in
//! `BeliefRuntime::insert_belief`.
//!
//! These tests run only when the `active-inference` feature is enabled —
//! the runtime's default build path is unchanged.

#![cfg(feature = "active-inference")]

use std::sync::Arc;

use tokio::sync::Mutex;

use epica_active_inference::{ActiveInferenceMonitor, MonitorConfig};
use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
use epica_runtime::BeliefRuntime;

fn node(key: &str, conf: f32) -> BeliefNode {
    BeliefNode::new(
        key,
        BeliefValue::Asserted("v".into()),
        Provenance::UserStatement { turn: 0 },
        conf,
    )
}

#[tokio::test]
async fn monitor_observes_each_insert() {
    let monitor = Arc::new(Mutex::new(ActiveInferenceMonitor::new()));
    let rt = BeliefRuntime::new(BeliefQuad::new(), 0.5, 10, 0.0)
        .with_active_inference(monitor.clone());

    for k in ["a", "b", "c"] {
        rt.insert_belief(node(k, 0.5)).await;
    }

    let m = monitor.lock().await;
    assert_eq!(m.observations, 3);
    assert_eq!(m.history().len(), 3);
}

#[tokio::test]
async fn monitor_signals_budget_breach_on_overconfident_quad() {
    // Tight budget so even a single overconfident belief tips it.
    let cfg = MonitorConfig {
        surprise_threshold: 3.0,
        homeostatic_budget: 0.5,
        history_capacity: 64,
    };
    let monitor = Arc::new(Mutex::new(ActiveInferenceMonitor::with_config(cfg)));
    let rt = BeliefRuntime::new(BeliefQuad::new(), 0.5, 10, 0.0)
        .with_active_inference(monitor.clone());

    for k in 0..20 {
        rt.insert_belief(node(&format!("k{k}"), 0.999)).await;
    }

    let m = monitor.lock().await;
    let last = m
        .history()
        .back()
        .copied()
        .expect("at least one observation");
    assert!(
        last > 0.5,
        "last observation must exceed homeostatic_budget=0.5; got {last}"
    );
    assert_eq!(m.observations, 20);
}

#[tokio::test]
async fn last_surprise_helper_returns_head() {
    let monitor = Arc::new(Mutex::new(ActiveInferenceMonitor::new()));
    let rt = BeliefRuntime::new(BeliefQuad::new(), 0.5, 10, 0.0)
        .with_active_inference(monitor);

    assert!(rt.last_surprise().await.is_none());
    rt.insert_belief(node("k", 0.7)).await;
    let head = rt.last_surprise().await;
    assert!(head.is_some());
    assert!(head.unwrap().is_finite());
}

#[tokio::test]
async fn detach_stops_observations() {
    let monitor = Arc::new(Mutex::new(ActiveInferenceMonitor::new()));
    let mut rt = BeliefRuntime::new(BeliefQuad::new(), 0.5, 10, 0.0)
        .with_active_inference(monitor.clone());

    rt.insert_belief(node("k0", 0.5)).await;
    rt.clear_active_inference();
    rt.insert_belief(node("k1", 0.5)).await;
    rt.insert_belief(node("k2", 0.5)).await;

    let m = monitor.lock().await;
    assert_eq!(m.observations, 1,
        "detach must stop further observations; got {}", m.observations);
}

#[tokio::test]
async fn insert_without_monitor_is_zero_cost_default() {
    // No call to `with_active_inference` ⇒ default `active_inference =
    // None` ⇒ insert path is identical to the pre-Sprint-2 baseline.
    let rt = BeliefRuntime::new(BeliefQuad::new(), 0.5, 10, 0.0);
    let id = rt.insert_belief(node("k", 0.5)).await;
    assert!(rt.last_surprise().await.is_none());
    assert_eq!(rt.get_by_key("k").await, Some(id));
}
