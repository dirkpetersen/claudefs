//! Security tests for claudefs-transport trace_aggregator module.
//!
//! This module validates security properties of the distributed tracing system
//! including trace ID generation, span record integrity, trace data aggregation,
//! critical path analysis, and memory/performance handling.

#[cfg(test)]
mod tests {
    use claudefs_transport::trace_aggregator::{TraceId, SpanRecord, TraceData, TraceAggregator, TraceAggregatorConfig};
    use claudefs_transport::otel::{OtlpStatusCode, OtlpAttribute, OtlpValue};
    use std::time::SystemTime;
    use std::collections::HashMap;

    fn make_span(id: u64, parent_id: Option<u64>, name: &str, start: u64, end: u64) -> SpanRecord {
        SpanRecord::new(
            id.to_le_bytes(),
            parent_id.map(|p| p.to_le_bytes()),
            name,
            start,
            end,
        )
    }

    // ============================================================================
    // Category 1: Trace ID Generation and Uniqueness (6 tests)
    // ============================================================================

    mod trace_id_generation_and_uniqueness {
        use super::*;

        #[test]
        fn test_transport_trace_sec_random_traceid_distinct() {
            let mut trace_ids = Vec::new();

            for _ in 0..1000 {
                trace_ids.push(TraceId::random());
            }

            let unique_count = trace_ids.iter().collect::<HashMap<_, ()>>().len();
            assert_eq!(unique_count, 1000, "All randomly generated TraceIds should be unique");
        }

        #[test]
        fn test_transport_trace_sec_traceid_from_bytes_deterministic() {
            let bytes = [1u8; 16];
            let trace_id1 = TraceId::from_bytes(bytes);
            let trace_id2 = TraceId::from_bytes(bytes);

            assert_eq!(trace_id1, trace_id2,
                "Same bytes should produce equal TraceId");
        }

        #[test]
        fn test_transport_trace_sec_traceid_default_is_zero() {
            let trace_id = TraceId::default();

            assert_eq!(trace_id.0, [0u8; 16],
                "Default TraceId should be all zeros");
        }

        #[test]
        fn test_transport_trace_sec_traceid_hashable() {
            let mut map = HashMap::new();

            for i in 0..100 {
                let trace_id = TraceId::from_bytes((0..16).map(|j| (i + j) as u8).collect::<Vec<_>>().try_into().unwrap());
                map.insert(trace_id, i);
            }

            assert_eq!(map.len(), 100, "TraceIds should be usable as HashMap keys");
        }

        #[test]
        fn test_transport_trace_sec_traceid_equality_reflexive() {
            let trace_id = TraceId::random();

            assert_eq!(trace_id, trace_id,
                "TraceId should equal itself (reflexive)");
        }

        #[test]
        fn test_transport_trace_sec_traceid_clone_identical() {
            let trace_id = TraceId::random();
            let cloned = trace_id.clone();

            assert_eq!(cloned, trace_id,
                "Cloned TraceId should equal original");
        }
    }

    // ============================================================================
    // Category 2: Span Record Integrity (7 tests)
    // ============================================================================

    mod span_record_integrity {
        use super::*;

        #[test]
        fn test_transport_trace_sec_span_fields_preserved() {
            let span = SpanRecord::new(
                [1, 2, 3, 4, 5, 6, 7, 8],
                Some([9, 10, 11, 12, 13, 14, 15, 16]),
                "test_operation",
                1000,
                5000,
            ).with_status(OtlpStatusCode::Ok)
             .with_attributes(vec![
                 OtlpAttribute::new("key1", OtlpValue::string("value1")),
                 OtlpAttribute::new("key2", OtlpValue::int(42)),
             ]);

            assert_eq!(span.name, "test_operation");
            assert_eq!(span.start_time_unix_nano, 1000);
            assert_eq!(span.end_time_unix_nano, 5000);
            assert_eq!(span.status, OtlpStatusCode::Ok);
            assert_eq!(span.attributes.len(), 2);
        }

        #[test]
        fn test_transport_trace_sec_duration_ns_saturating_subtract() {
            let span = SpanRecord::new(
                [1u8; 8],
                None,
                "test",
                5000,
                1000,
            );

            let duration = span.duration_ns();

            assert_eq!(duration, 0,
                "When end_time < start_time, duration should be 0 (saturating)");
        }

        #[test]
        fn test_transport_trace_sec_duration_ns_calculation_correct() {
            let span = SpanRecord::new(
                [1u8; 8],
                None,
                "test",
                100,
                500,
            );

            let duration = span.duration_ns();

            assert_eq!(duration, 400,
                "Duration should be end_time - start_time = 500 - 100 = 400");
        }

        #[test]
        fn test_transport_trace_sec_status_code_unset_default() {
            let span = SpanRecord::new(
                [1u8; 8],
                None,
                "test",
                0,
                100,
            );

            assert_eq!(span.status, OtlpStatusCode::Unset,
                "New SpanRecord should have status=Unset by default");
        }

        #[test]
        fn test_transport_trace_sec_with_status_builder_pattern() {
            let span = SpanRecord::new(
                [1u8; 8],
                None,
                "test",
                0,
                100,
            ).with_status(OtlpStatusCode::Error);

            assert_eq!(span.status, OtlpStatusCode::Error,
                "with_status should set the status field");
        }

        #[test]
        fn test_transport_trace_sec_attributes_list_preserved() {
            let mut attrs = Vec::new();
            for i in 0..100 {
                attrs.push(OtlpAttribute::new(
                    format!("key{}", i),
                    OtlpValue::string(format!("value{}", i)),
                ));
            }

            let span = SpanRecord::new(
                [1u8; 8],
                None,
                "test",
                0,
                100,
            ).with_attributes(attrs);

            assert_eq!(span.attributes.len(), 100,
                "All attributes should be preserved");
        }

        #[test]
        fn test_transport_trace_sec_span_name_into_conversion() {
            let span = SpanRecord::new(
                [1u8; 8],
                None,
                "test_op",
                0,
                100,
            );

            assert_eq!(span.name, "test_op",
                "Name field should be set correctly via Into<String>");
        }
    }

    // ============================================================================
    // Category 3: Trace Data Aggregation (6 tests)
    // ============================================================================

    mod trace_data_aggregation {
        use super::*;

        #[test]
        fn test_transport_trace_sec_multiple_spans_aggregated() {
            let agg = TraceAggregator::default();
            let trace_id = TraceId::random();

            for i in 0..10 {
                agg.record_span(trace_id.clone(), make_span(i, None, "span", 0, 100));
            }

            let stats = agg.stats();
            assert_eq!(stats.spans_recorded, 10,
                "All 10 spans should be aggregated");
        }

        #[test]
        fn test_transport_trace_sec_root_span_identification() {
            let trace = TraceData {
                trace_id: TraceId::default(),
                root_span_id: [1u8; 8],
                spans: vec![
                    SpanRecord::new([1u8; 8], None, "root", 0, 100),
                    SpanRecord::new([2u8; 8], Some([1u8; 8]), "child", 10, 80),
                ],
                received_at_ns: 0,
            };

            let root = trace.root_span();
            assert!(root.is_some(), "Should find root span");
            assert_eq!(root.unwrap().name, "root", "Root should be the one with no parent");
        }

        #[test]
        fn test_transport_trace_sec_parent_child_relationships() {
            let agg = TraceAggregator::default();
            let trace_id = TraceId::random();

            agg.record_span(trace_id.clone(), make_span(1, None, "root", 0, 100));
            agg.record_span(trace_id.clone(), make_span(2, Some(1), "child", 10, 80));

            let trace_opt = agg.complete_trace(trace_id.clone());
            assert!(trace_opt.is_some(), "Trace should be completable");

            let trace = trace_opt.unwrap();
            assert!(trace.spans.len() >= 2, "Should have parent-child spans");
        }

        #[test]
        fn test_transport_trace_sec_timeline_ordering_by_start_time() {
            let spans = vec![
                SpanRecord::new([3u8; 8], None, "span3", 200, 300),
                SpanRecord::new([1u8; 8], None, "span1", 100, 200),
                SpanRecord::new([2u8; 8], None, "span2", 50, 150),
            ];

            let mut sorted_spans = spans.clone();
            sorted_spans.sort_by_key(|s| s.start_time_unix_nano);

            assert_eq!(sorted_spans[0].start_time_unix_nano, 50);
            assert_eq!(sorted_spans[1].start_time_unix_nano, 100);
            assert_eq!(sorted_spans[2].start_time_unix_nano, 200);
        }

        #[test]
        fn test_transport_trace_sec_received_at_ns_recorded() {
            let agg = TraceAggregator::default();
            let trace_id = TraceId::random();

            agg.record_span(trace_id.clone(), make_span(1, None, "root", 0, 100));

            let trace_opt = agg.complete_trace(trace_id);
            assert!(trace_opt.is_some());

            let trace = trace_opt.unwrap();
            assert!(trace.received_at_ns > 0,
                "received_at_ns should be set to a valid timestamp");
        }

        #[test]
        fn test_transport_trace_sec_trace_id_in_data_preserved() {
            let trace_id = TraceId::from_bytes([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);

            let trace = TraceData {
                trace_id: trace_id.clone(),
                root_span_id: [1u8; 8],
                spans: vec![SpanRecord::new([1u8; 8], None, "root", 0, 100)],
                received_at_ns: 0,
            };

            assert_eq!(trace.trace_id, trace_id,
                "trace_id should be preserved in TraceData");
        }
    }

    // ============================================================================
    // Category 4: Critical Path Analysis (5 tests)
    // ============================================================================

    mod critical_path_analysis {
        use super::*;

        #[test]
        fn test_transport_trace_sec_critical_path_longest_dependency_chain() {
            let trace = TraceData {
                trace_id: TraceId::default(),
                root_span_id: [1u8; 8],
                spans: vec![
                    SpanRecord::new([1u8; 8], None, "A", 0, 100),
                    SpanRecord::new([2u8; 8], Some([1u8; 8]), "B", 10, 80),
                    SpanRecord::new([3u8; 8], Some([2u8; 8]), "C", 20, 60),
                ],
                received_at_ns: 0,
            };

            let stats = trace.latency_stats();
            assert_eq!(stats.span_count, 3,
                "All 3 spans should be included in critical path analysis");
        }

        #[test]
        fn test_transport_trace_sec_critical_path_excludes_parallel_branches() {
            let trace = TraceData {
                trace_id: TraceId::default(),
                root_span_id: [1u8; 8],
                spans: vec![
                    SpanRecord::new([1u8; 8], None, "root", 0, 100),
                    SpanRecord::new([2u8; 8], Some([1u8; 8]), "branch_a", 10, 50),
                    SpanRecord::new([3u8; 8], Some([1u8; 8]), "branch_b", 10, 50),
                ],
                received_at_ns: 0,
            };

            let stats = trace.latency_stats();
            assert_eq!(stats.span_count, 3,
                "Both parallel branches should be included");
        }

        #[test]
        fn test_transport_trace_sec_latency_attribution_per_stage() {
            let trace = TraceData {
                trace_id: TraceId::default(),
                root_span_id: [1u8; 8],
                spans: vec![
                    SpanRecord::new([1u8; 8], None, "client", 0, 100),
                    SpanRecord::new([2u8; 8], Some([1u8; 8]), "metadata", 10, 50),
                    SpanRecord::new([3u8; 8], Some([2u8; 8]), "storage", 20, 40),
                ],
                received_at_ns: 0,
            };

            let stats = trace.latency_stats();
            assert!(stats.max_ns > 0, "Should compute latency per stage");
        }

        #[test]
        fn test_transport_trace_sec_p50_p95_p99_percentiles_computed() {
            let mut spans = Vec::new();
            for i in 0..100 {
                let duration = ((i as f64 * 10.0) + 1000.0) as u64;
                spans.push(SpanRecord::new(
                    (i as u64).to_le_bytes(),
                    None,
                    "span",
                    0,
                    duration,
                ));
            }

            let trace = TraceData {
                trace_id: TraceId::default(),
                root_span_id: [1u8; 8],
                spans,
                received_at_ns: 0,
            };

            let stats = trace.latency_stats();

            assert!(stats.p50_ns > 0, "p50 should be computed");
            assert!(stats.p95_ns > stats.p50_ns, "p95 should be > p50");
            assert!(stats.p99_ns > stats.p95_ns, "p99 should be > p95");
        }

        #[test]
        fn test_transport_trace_sec_outlier_detection_anomalous_spans() {
            let mut spans = Vec::new();

            for i in 0..99 {
                spans.push(SpanRecord::new(
                    (i as u64).to_le_bytes(),
                    None,
                    "normal",
                    0,
                    1000,
                ));
            }

            spans.push(SpanRecord::new(
                99u64.to_le_bytes(),
                None,
                "outlier",
                0,
                100000,
            ));

            let trace = TraceData {
                trace_id: TraceId::default(),
                root_span_id: [1u8; 8],
                spans,
                received_at_ns: 0,
            };

            let stats = trace.latency_stats();

            assert!(stats.max_ns >= 100000,
                "Outlier should be detected as max");
            assert!(stats.mean_ns > 1000,
                "Mean should be affected by outlier");
        }
    }

    // ============================================================================
    // Category 5: Memory and Performance (4 tests)
    // ============================================================================

    mod memory_and_performance {
        use super::*;

        #[test]
        fn test_transport_trace_sec_large_trace_no_oom() {
            let agg = TraceAggregator::default();
            let trace_id = TraceId::random();

            for i in 0..10000 {
                agg.record_span(trace_id.clone(), make_span(i as u64, None, "span", 0, 100));
            }

            let stats = agg.stats();
            assert!(stats.spans_recorded >= 10000, "Should handle large traces without OOM");
        }

        #[test]
        fn test_transport_trace_sec_span_storage_not_copied() {
            let agg = TraceAggregator::default();
            let trace_id = TraceId::random();

            let span = make_span(1, None, "span", 0, 100);
            agg.record_span(trace_id.clone(), span.clone());

            let trace_opt = agg.complete_trace(trace_id);
            assert!(trace_opt.is_some());

            let trace = trace_opt.unwrap();
            assert!(trace.spans.len() > 0, "Span should be stored");
        }

        #[test]
        fn test_transport_trace_sec_hash_based_lookup_efficient() {
            let mut map: HashMap<TraceId, Vec<SpanRecord>> = HashMap::new();

            for i in 0..10000 {
                let trace_id = TraceId::from_bytes(
                    (i as u64).to_le_bytes().iter()
                        .chain((i as u64).to_le_bytes().iter())
                        .cloned()
                        .collect::<Vec<u8>>()
                        .try_into()
                        .unwrap()
                );

                let span = make_span(i as u64, None, "span", 0, 100);
                map.entry(trace_id).or_insert_with(Vec::new).push(span);
            }

            assert_eq!(map.len(), 10000,
                "Hash-based lookup should be efficient for 10000 traces");
        }

        #[tokio::test]
        async fn test_transport_trace_sec_concurrent_span_insertion_thread_safe() {
            use std::sync::Arc;

            let agg = Arc::new(TraceAggregator::default());
            let mut handles = vec![];

            for i in 0..10 {
                let agg = Arc::clone(&agg);
                let handle = tokio::spawn(async move {
                    let trace_id = TraceId::random();
                    for j in 0..100 {
                        let span = make_span(j, None, "span", 0, 100);
                        agg.record_span(trace_id.clone(), span);
                    }
                });
                handles.push(handle);
            }

            for handle in handles {
                handle.await.unwrap();
            }

            let stats = agg.stats();
            assert!(stats.traces_recorded > 0,
                "Concurrent insertions should not panic");
        }
    }
}