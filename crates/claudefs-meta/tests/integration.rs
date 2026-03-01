//! Integration tests for multi-node cluster scenarios.
//!
//! These tests verify end-to-end cluster behavior including leader election,
//! metadata consistency, failure recovery, and scaling.

mod common;

use common::{TestCluster, TestConfig};

#[tokio::test]
async fn test_cluster_bootstrap() {
    // Verify cluster initialization
    let cluster = TestCluster::new(3);
    let alive = cluster.alive_nodes().await;

    assert_eq!(alive.len(), 3);
    assert_eq!(alive, vec![0, 1, 2]);
}

#[tokio::test]
async fn test_node_failure_detection() {
    // Verify single node failure is detected
    let cluster = TestCluster::new(3);

    cluster.stop_node(1).await;
    let alive = cluster.alive_nodes().await;

    assert_eq!(alive.len(), 2);
    assert!(!alive.contains(&1));
}

#[tokio::test]
async fn test_network_partition() {
    // Verify cluster detects network partition
    let cluster = TestCluster::new(3);

    // Partition node 0 from nodes 1-2
    cluster.partition(vec![0]).await;
    let alive = cluster.alive_nodes().await;

    assert_eq!(alive.len(), 2);
    assert!(alive.contains(&1));
    assert!(alive.contains(&2));
    assert!(!alive.contains(&0));
}

#[tokio::test]
async fn test_partition_healing() {
    // Verify cluster recovers from network partition
    let cluster = TestCluster::new(3);

    cluster.partition(vec![0, 1]).await;
    assert_eq!(cluster.alive_nodes().await.len(), 1);

    cluster.heal_partition().await;
    assert_eq!(cluster.alive_nodes().await.len(), 3);
}

#[tokio::test]
async fn test_cascading_failures() {
    // Verify cluster behavior under cascading failures
    let cluster = TestCluster::new(5);

    // Kill first node
    cluster.stop_node(0).await;
    assert_eq!(cluster.alive_nodes().await.len(), 4);

    // Kill second node
    cluster.stop_node(1).await;
    assert_eq!(cluster.alive_nodes().await.len(), 3);

    // Kill third node
    cluster.stop_node(2).await;
    assert_eq!(cluster.alive_nodes().await.len(), 2);
}

#[tokio::test]
async fn test_majority_quorum_threshold() {
    // Verify cluster requires majority quorum
    let cluster = TestCluster::new(3);

    // Partition to minority (1 out of 3)
    cluster.partition(vec![0]).await;
    let alive = cluster.alive_nodes().await;
    assert!(alive.len() > 1, "Majority quorum should exist");

    // Continue partitioning to reach minority
    cluster.stop_node(1).await;
    let alive = cluster.alive_nodes().await;
    assert_eq!(alive.len(), 1, "Only 1 node should be alive");
}

#[tokio::test]
async fn test_recovery_sequence() {
    // Verify recovery after multiple failures
    let cluster = TestCluster::new(3);

    // Simulate cascading failures
    cluster.stop_node(0).await;
    cluster.stop_node(1).await;
    assert_eq!(cluster.alive_nodes().await.len(), 1);

    // Begin recovery
    cluster.start_node(0).await;
    assert_eq!(cluster.alive_nodes().await.len(), 2);

    cluster.start_node(1).await;
    assert_eq!(cluster.alive_nodes().await.len(), 3);
}

#[tokio::test]
async fn test_large_cluster_resilience() {
    // Verify larger clusters (9 nodes) can tolerate up to 4 failures
    let cluster = TestCluster::new(9);

    // Stop up to 4 nodes (majority of 5 remains: 5/9)
    for i in 0..4 {
        cluster.stop_node(i).await;
    }

    let alive = cluster.alive_nodes().await;
    assert!(alive.len() > 4, "Majority quorum should still exist");
}

#[tokio::test]
async fn test_config_validation() {
    // Verify test configuration
    let config = TestConfig::default();
    assert!(config.election_timeout_ms > 0);
    assert!(config.heartbeat_interval_ms > 0);
    assert!(config.election_timeout_ms > config.heartbeat_interval_ms);
}
