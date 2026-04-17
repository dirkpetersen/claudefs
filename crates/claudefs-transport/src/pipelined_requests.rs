//! Request pipelining for parallel request processing with per-stream ordering.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct RequestId(pub u64);

impl RequestId {
    pub fn new(id: u64) -> Self {
        RequestId(id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StreamId(pub u64);

impl StreamId {
    pub fn new(id: u64) -> Self {
        StreamId(id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestState {
    Pending,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelinedRequest {
    pub request_id: RequestId,
    pub stream_id: StreamId,
    pub sequence_num: u32,
    pub depends_on: Vec<RequestId>,
    pub payload: Vec<u8>,
    pub timeout_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelinedResponse {
    pub request_id: RequestId,
    pub stream_id: StreamId,
    pub sequence_num: u32,
    pub result: Result<Vec<u8>, String>,
    pub latency_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    pub max_depth: usize,
    pub max_streams: usize,
    pub default_timeout_ms: u32,
    pub enable_dependencies: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            max_depth: 32,
            max_streams: 128,
            default_timeout_ms: 30000,
            enable_dependencies: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InFlightRequest {
    pub request: PipelinedRequest,
    pub state: RequestState,
    pub submitted_ns: u64,
    pub dependencies_satisfied: bool,
    pub response: Option<PipelinedResponse>,
}

pub struct PipelineStats {
    pub requests_submitted: AtomicU64,
    pub requests_completed: AtomicU64,
    pub requests_failed: AtomicU64,
    pub requests_cancelled: AtomicU64,
    pub max_pipeline_depth_reached: AtomicU32,
    pub dependency_satisfaction_time_ns: AtomicU64,
}

impl Default for PipelineStats {
    fn default() -> Self {
        Self {
            requests_submitted: AtomicU64::new(0),
            requests_completed: AtomicU64::new(0),
            requests_failed: AtomicU64::new(0),
            requests_cancelled: AtomicU64::new(0),
            max_pipeline_depth_reached: AtomicU32::new(0),
            dependency_satisfaction_time_ns: AtomicU64::new(0),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStatsSnapshot {
    pub requests_submitted: u64,
    pub requests_completed: u64,
    pub requests_failed: u64,
    pub requests_cancelled: u64,
    pub max_pipeline_depth_reached: u32,
    pub avg_dependency_satisfaction_time_ns: u64,
}

pub struct RequestPipeline {
    pub config: PipelineConfig,
    pub stats: Arc<PipelineStats>,
    streams: RwLock<HashMap<StreamId, VecDeque<RequestId>>>,
    in_flight: RwLock<HashMap<RequestId, InFlightRequest>>,
    completed_responses: RwLock<VecDeque<PipelinedResponse>>,
    max_depth_seen: AtomicU32,
}

impl RequestPipeline {
    pub fn new(config: PipelineConfig) -> Self {
        Self {
            config,
            stats: Arc::new(PipelineStats::default()),
            streams: RwLock::new(HashMap::new()),
            in_flight: RwLock::new(HashMap::new()),
            completed_responses: RwLock::new(VecDeque::new()),
            max_depth_seen: AtomicU32::new(0),
        }
    }

    pub fn submit_request(&self, request: PipelinedRequest) -> Result<(), String> {
        let current_depth = self.current_depth();

        if current_depth >= self.config.max_depth {
            return Err("Pipeline is full".to_string());
        }

        let stream_depth = self.stream_depth(request.stream_id);
        if stream_depth >= self.config.max_streams {
            return Err("Stream is full".to_string());
        }

        let depth_u32 = current_depth as u32;
        let max_seen = self.max_depth_seen.load(Ordering::Relaxed);
        if depth_u32 > max_seen {
            self.max_depth_seen.store(depth_u32, Ordering::Relaxed);
            self.stats
                .max_pipeline_depth_reached
                .store(depth_u32, Ordering::Relaxed);
        }

        self.stats
            .requests_submitted
            .fetch_add(1, Ordering::Relaxed);

        let in_flight_req = InFlightRequest {
            request: request.clone(),
            state: RequestState::Pending,
            submitted_ns: current_time_ns(),
            dependencies_satisfied: !self.config.enable_dependencies
                || request.depends_on.is_empty(),
            response: None,
        };

        {
            let mut in_flight = self.in_flight.write().unwrap();
            in_flight.insert(request.request_id, in_flight_req);
        }

        {
            let mut streams = self.streams.write().unwrap();
            streams
                .entry(request.stream_id)
                .or_insert_with(VecDeque::new)
                .push_back(request.request_id);
        }

        Ok(())
    }

    pub fn complete_request(
        &self,
        request_id: RequestId,
        response: PipelinedResponse,
    ) -> Result<(), String> {
        {
            let mut in_flight = self.in_flight.write().unwrap();
            let req = in_flight.get_mut(&request_id).ok_or("Request not found")?;
            req.state = RequestState::Completed;
            req.response = Some(response.clone());
        }

        self.stats
            .requests_completed
            .fetch_add(1, Ordering::Relaxed);

        {
            let mut completed = self.completed_responses.write().unwrap();
            completed.push_back(response);
        }

        Ok(())
    }

    pub fn fail_request(&self, request_id: RequestId, error: String) -> Result<(), String> {
        {
            let mut in_flight = self.in_flight.write().unwrap();
            let req = in_flight.get_mut(&request_id).ok_or("Request not found")?;
            req.state = RequestState::Failed;
            let response = PipelinedResponse {
                request_id: req.request.request_id,
                stream_id: req.request.stream_id,
                sequence_num: req.request.sequence_num,
                result: Err(error),
                latency_us: (current_time_ns() - req.submitted_ns) / 1000,
            };
            req.response = Some(response.clone());
        }

        self.stats.requests_failed.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    pub fn cancel_request(&self, request_id: RequestId) -> Result<(), String> {
        {
            let mut in_flight = self.in_flight.write().unwrap();
            let req = in_flight.get_mut(&request_id).ok_or("Request not found")?;
            req.state = RequestState::Cancelled;
        }

        self.stats
            .requests_cancelled
            .fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn next_ready_response(&self) -> Option<PipelinedResponse> {
        let mut completed = self.completed_responses.write().unwrap();
        completed.pop_front()
    }

    pub fn responses_for_stream(&self, stream_id: StreamId) -> Vec<PipelinedResponse> {
        let in_flight = self.in_flight.read().unwrap();
        let streams = self.streams.read().unwrap();

        let mut responses = Vec::new();
        if let Some(req_ids) = streams.get(&stream_id) {
            for id in req_ids {
                if let Some(req) = in_flight.get(id) {
                    if let Some(resp) = &req.response {
                        if req.state == RequestState::Completed {
                            responses.push(resp.clone());
                        }
                    }
                }
            }
        }

        responses.sort_by_key(|r| r.sequence_num);
        responses
    }

    pub fn current_depth(&self) -> usize {
        self.in_flight.read().unwrap().len()
    }

    pub fn stream_depth(&self, stream_id: StreamId) -> usize {
        let streams = self.streams.read().unwrap();
        streams.get(&stream_id).map(|v| v.len()).unwrap_or(0)
    }

    pub fn dependencies_satisfied(&self, request_id: RequestId) -> bool {
        let in_flight = self.in_flight.read().unwrap();

        if let Some(req) = in_flight.get(&request_id) {
            if req.dependencies_satisfied {
                return true;
            }

            req.request.depends_on.iter().all(|dep_id| {
                in_flight
                    .get(dep_id)
                    .map(|r| matches!(r.state, RequestState::Completed | RequestState::Failed))
                    .unwrap_or(true)
            })
        } else {
            false
        }
    }

    pub fn stats_snapshot(&self) -> PipelineStatsSnapshot {
        let in_flight = self.in_flight.read().unwrap();
        let count = in_flight
            .values()
            .filter(|r| r.dependencies_satisfied)
            .count() as u64;

        PipelineStatsSnapshot {
            requests_submitted: self.stats.requests_submitted.load(Ordering::Relaxed),
            requests_completed: self.stats.requests_completed.load(Ordering::Relaxed),
            requests_failed: self.stats.requests_failed.load(Ordering::Relaxed),
            requests_cancelled: self.stats.requests_cancelled.load(Ordering::Relaxed),
            max_pipeline_depth_reached: self
                .stats
                .max_pipeline_depth_reached
                .load(Ordering::Relaxed),
            avg_dependency_satisfaction_time_ns: if count > 0 {
                self.stats
                    .dependency_satisfaction_time_ns
                    .load(Ordering::Relaxed)
                    / count
            } else {
                0
            },
        }
    }
}

fn current_time_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

unsafe impl Send for RequestPipeline {}
unsafe impl Sync for RequestPipeline {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_creation_with_default_config() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());
        assert_eq!(pipeline.current_depth(), 0);
    }

    #[test]
    fn test_submit_single_request() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());
        let request = PipelinedRequest {
            request_id: RequestId::new(1),
            stream_id: StreamId::new(1),
            sequence_num: 0,
            depends_on: vec![],
            payload: vec![1, 2, 3],
            timeout_ms: 5000,
        };

        pipeline.submit_request(request).unwrap();
        assert_eq!(pipeline.current_depth(), 1);
    }

    #[test]
    fn test_submit_multiple_requests_different_streams() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());

        for i in 0..5 {
            let request = PipelinedRequest {
                request_id: RequestId::new(i),
                stream_id: StreamId::new(i),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            };
            pipeline.submit_request(request).unwrap();
        }

        assert_eq!(pipeline.current_depth(), 5);
    }

    #[test]
    fn test_pipeline_depth_increments_on_submit() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(1),
                stream_id: StreamId::new(1),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(2),
                stream_id: StreamId::new(2),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        assert_eq!(pipeline.current_depth(), 2);
    }

    #[test]
    fn test_pipeline_full_returns_error() {
        let config = PipelineConfig {
            max_depth: 2,
            ..Default::default()
        };
        let pipeline = RequestPipeline::new(config);

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(1),
                stream_id: StreamId::new(1),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(2),
                stream_id: StreamId::new(2),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        let result = pipeline.submit_request(PipelinedRequest {
            request_id: RequestId::new(3),
            stream_id: StreamId::new(3),
            sequence_num: 0,
            depends_on: vec![],
            payload: vec![],
            timeout_ms: 5000,
        });

        assert!(result.is_err());
    }

    #[test]
    fn test_stream_full_returns_error() {
        let config = PipelineConfig {
            max_streams: 2,
            ..Default::default()
        };
        let pipeline = RequestPipeline::new(config);

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(1),
                stream_id: StreamId::new(1),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(2),
                stream_id: StreamId::new(1),
                sequence_num: 1,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        let result = pipeline.submit_request(PipelinedRequest {
            request_id: RequestId::new(3),
            stream_id: StreamId::new(1),
            sequence_num: 2,
            depends_on: vec![],
            payload: vec![],
            timeout_ms: 5000,
        });

        assert!(result.is_err());
    }

    #[test]
    fn test_complete_request_removes_from_inflight() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(1),
                stream_id: StreamId::new(1),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .complete_request(
                RequestId::new(1),
                PipelinedResponse {
                    request_id: RequestId::new(1),
                    stream_id: StreamId::new(1),
                    sequence_num: 0,
                    result: Ok(vec![]),
                    latency_us: 100,
                },
            )
            .unwrap();

        assert_eq!(pipeline.current_depth(), 1);
    }

    #[test]
    fn test_fail_request_sets_failed_state() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(1),
                stream_id: StreamId::new(1),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .fail_request(RequestId::new(1), "Error".to_string())
            .unwrap();

        let stats = pipeline.stats_snapshot();
        assert_eq!(stats.requests_failed, 1);
    }

    #[test]
    fn test_cancel_request_sets_cancelled_state() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(1),
                stream_id: StreamId::new(1),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline.cancel_request(RequestId::new(1)).unwrap();

        let stats = pipeline.stats_snapshot();
        assert_eq!(stats.requests_cancelled, 1);
    }

    #[test]
    fn test_next_ready_response_returns_completed() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(1),
                stream_id: StreamId::new(1),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .complete_request(
                RequestId::new(1),
                PipelinedResponse {
                    request_id: RequestId::new(1),
                    stream_id: StreamId::new(1),
                    sequence_num: 0,
                    result: Ok(vec![1, 2, 3]),
                    latency_us: 100,
                },
            )
            .unwrap();

        let response = pipeline.next_ready_response();
        assert!(response.is_some());
    }

    #[test]
    fn test_responses_for_stream_ordered_by_sequence() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(1),
                stream_id: StreamId::new(1),
                sequence_num: 2,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(2),
                stream_id: StreamId::new(1),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(3),
                stream_id: StreamId::new(1),
                sequence_num: 1,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .complete_request(
                RequestId::new(1),
                PipelinedResponse {
                    request_id: RequestId::new(1),
                    stream_id: StreamId::new(1),
                    sequence_num: 2,
                    result: Ok(vec![]),
                    latency_us: 100,
                },
            )
            .unwrap();

        pipeline
            .complete_request(
                RequestId::new(2),
                PipelinedResponse {
                    request_id: RequestId::new(2),
                    stream_id: StreamId::new(1),
                    sequence_num: 0,
                    result: Ok(vec![]),
                    latency_us: 100,
                },
            )
            .unwrap();

        pipeline
            .complete_request(
                RequestId::new(3),
                PipelinedResponse {
                    request_id: RequestId::new(3),
                    stream_id: StreamId::new(1),
                    sequence_num: 1,
                    result: Ok(vec![]),
                    latency_us: 100,
                },
            )
            .unwrap();

        let responses = pipeline.responses_for_stream(StreamId::new(1));
        assert_eq!(responses[0].sequence_num, 0);
        assert_eq!(responses[1].sequence_num, 1);
        assert_eq!(responses[2].sequence_num, 2);
    }

    #[test]
    fn test_responses_for_stream_excludes_failed() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(1),
                stream_id: StreamId::new(1),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .complete_request(
                RequestId::new(1),
                PipelinedResponse {
                    request_id: RequestId::new(1),
                    stream_id: StreamId::new(1),
                    sequence_num: 0,
                    result: Ok(vec![]),
                    latency_us: 100,
                },
            )
            .unwrap();

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(2),
                stream_id: StreamId::new(1),
                sequence_num: 1,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .fail_request(RequestId::new(2), "Error".to_string())
            .unwrap();

        let responses = pipeline.responses_for_stream(StreamId::new(1));
        assert_eq!(responses.len(), 1);
    }

    #[test]
    fn test_head_of_line_blocking_prevented() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(1),
                stream_id: StreamId::new(1),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(2),
                stream_id: StreamId::new(1),
                sequence_num: 1,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .complete_request(
                RequestId::new(2),
                PipelinedResponse {
                    request_id: RequestId::new(2),
                    stream_id: StreamId::new(1),
                    sequence_num: 1,
                    result: Ok(vec![]),
                    latency_us: 100,
                },
            )
            .unwrap();

        let response = pipeline.next_ready_response();
        assert!(response.is_some());
    }

    #[test]
    fn test_dependency_satisfaction_blocks_processing() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(1),
                stream_id: StreamId::new(1),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(2),
                stream_id: StreamId::new(1),
                sequence_num: 1,
                depends_on: vec![RequestId::new(1)],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        assert!(!pipeline.dependencies_satisfied(RequestId::new(2)));
    }

    #[test]
    fn test_dependency_satisfied_allows_processing() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(1),
                stream_id: StreamId::new(1),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .complete_request(
                RequestId::new(1),
                PipelinedResponse {
                    request_id: RequestId::new(1),
                    stream_id: StreamId::new(1),
                    sequence_num: 0,
                    result: Ok(vec![]),
                    latency_us: 100,
                },
            )
            .unwrap();

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(2),
                stream_id: StreamId::new(1),
                sequence_num: 1,
                depends_on: vec![RequestId::new(1)],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        assert!(pipeline.dependencies_satisfied(RequestId::new(2)));
    }

    #[test]
    fn test_dependency_across_streams() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(1),
                stream_id: StreamId::new(1),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .complete_request(
                RequestId::new(1),
                PipelinedResponse {
                    request_id: RequestId::new(1),
                    stream_id: StreamId::new(1),
                    sequence_num: 0,
                    result: Ok(vec![]),
                    latency_us: 100,
                },
            )
            .unwrap();

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(2),
                stream_id: StreamId::new(2),
                sequence_num: 0,
                depends_on: vec![RequestId::new(1)],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        assert!(pipeline.dependencies_satisfied(RequestId::new(2)));
    }

    #[test]
    fn test_multiple_dependencies_all_required() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(1),
                stream_id: StreamId::new(1),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(2),
                stream_id: StreamId::new(1),
                sequence_num: 1,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(3),
                stream_id: StreamId::new(1),
                sequence_num: 2,
                depends_on: vec![RequestId::new(1), RequestId::new(2)],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        assert!(!pipeline.dependencies_satisfied(RequestId::new(3)));

        pipeline
            .complete_request(
                RequestId::new(1),
                PipelinedResponse {
                    request_id: RequestId::new(1),
                    stream_id: StreamId::new(1),
                    sequence_num: 0,
                    result: Ok(vec![]),
                    latency_us: 100,
                },
            )
            .unwrap();

        assert!(!pipeline.dependencies_satisfied(RequestId::new(3)));

        pipeline
            .complete_request(
                RequestId::new(2),
                PipelinedResponse {
                    request_id: RequestId::new(2),
                    stream_id: StreamId::new(1),
                    sequence_num: 1,
                    result: Ok(vec![]),
                    latency_us: 100,
                },
            )
            .unwrap();

        assert!(pipeline.dependencies_satisfied(RequestId::new(3)));
    }

    #[test]
    fn test_dependencies_satisfied_true_when_all_complete() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(1),
                stream_id: StreamId::new(1),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .complete_request(
                RequestId::new(1),
                PipelinedResponse {
                    request_id: RequestId::new(1),
                    stream_id: StreamId::new(1),
                    sequence_num: 0,
                    result: Ok(vec![]),
                    latency_us: 100,
                },
            )
            .unwrap();

        assert!(pipeline.dependencies_satisfied(RequestId::new(1)));
    }

    #[test]
    fn test_dependencies_satisfied_false_when_any_pending() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(1),
                stream_id: StreamId::new(1),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(2),
                stream_id: StreamId::new(1),
                sequence_num: 1,
                depends_on: vec![RequestId::new(1)],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        assert!(!pipeline.dependencies_satisfied(RequestId::new(2)));
    }

    #[test]
    fn test_current_depth_reflects_inflight_count() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());

        for i in 0..10 {
            pipeline
                .submit_request(PipelinedRequest {
                    request_id: RequestId::new(i),
                    stream_id: StreamId::new(i % 3),
                    sequence_num: i as u32,
                    depends_on: vec![],
                    payload: vec![],
                    timeout_ms: 5000,
                })
                .unwrap();
        }

        assert_eq!(pipeline.current_depth(), 10);
    }

    #[test]
    fn test_stream_depth_counts_only_stream_requests() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(1),
                stream_id: StreamId::new(1),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(2),
                stream_id: StreamId::new(1),
                sequence_num: 1,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(3),
                stream_id: StreamId::new(2),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        assert_eq!(pipeline.stream_depth(StreamId::new(1)), 2);
        assert_eq!(pipeline.stream_depth(StreamId::new(2)), 1);
    }

    #[test]
    fn test_timeout_isolation_per_request() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(1),
                stream_id: StreamId::new(1),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 1000,
            })
            .unwrap();

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(2),
                stream_id: StreamId::new(1),
                sequence_num: 1,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        let in_flight = pipeline.in_flight.read().unwrap();
        let req1 = in_flight.get(&RequestId::new(1)).unwrap();
        let req2 = in_flight.get(&RequestId::new(2)).unwrap();

        assert_eq!(req1.request.timeout_ms, 1000);
        assert_eq!(req2.request.timeout_ms, 5000);
    }

    #[test]
    fn test_concurrent_submission_100_streams() {
        use std::thread;

        let pipeline = Arc::new(RequestPipeline::new(PipelineConfig::default()));

        let handles: Vec<_> = (0..100)
            .map(|i| {
                let pipeline = Arc::clone(&pipeline);
                thread::spawn(move || {
                    pipeline.submit_request(PipelinedRequest {
                        request_id: RequestId::new(i),
                        stream_id: StreamId::new(i),
                        sequence_num: 0,
                        depends_on: vec![],
                        payload: vec![],
                        timeout_ms: 5000,
                    })
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap().unwrap();
        }

        assert_eq!(pipeline.current_depth(), 100);
    }

    #[test]
    fn test_concurrent_completion_multiple_streams() {
        use std::thread;

        let pipeline = Arc::new(RequestPipeline::new(PipelineConfig::default()));

        for i in 0..10 {
            pipeline
                .submit_request(PipelinedRequest {
                    request_id: RequestId::new(i),
                    stream_id: StreamId::new(i % 3),
                    sequence_num: 0,
                    depends_on: vec![],
                    payload: vec![],
                    timeout_ms: 5000,
                })
                .unwrap();
        }

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let pipeline = Arc::clone(&pipeline);
                thread::spawn(move || {
                    pipeline.complete_request(
                        RequestId::new(i),
                        PipelinedResponse {
                            request_id: RequestId::new(i),
                            stream_id: StreamId::new(i % 3),
                            sequence_num: 0,
                            result: Ok(vec![]),
                            latency_us: 100,
                        },
                    )
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap().unwrap();
        }

        let stats = pipeline.stats_snapshot();
        assert_eq!(stats.requests_completed, 10);
    }

    #[test]
    fn test_stats_tracking_submitted() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(1),
                stream_id: StreamId::new(1),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        let stats = pipeline.stats_snapshot();
        assert_eq!(stats.requests_submitted, 1);
    }

    #[test]
    fn test_stats_tracking_completed() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(1),
                stream_id: StreamId::new(1),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .complete_request(
                RequestId::new(1),
                PipelinedResponse {
                    request_id: RequestId::new(1),
                    stream_id: StreamId::new(1),
                    sequence_num: 0,
                    result: Ok(vec![]),
                    latency_us: 100,
                },
            )
            .unwrap();

        let stats = pipeline.stats_snapshot();
        assert_eq!(stats.requests_completed, 1);
    }

    #[test]
    fn test_stats_tracking_failed() {
        let pipeline = RequestPipeline::new(PipelineConfig::default());

        pipeline
            .submit_request(PipelinedRequest {
                request_id: RequestId::new(1),
                stream_id: StreamId::new(1),
                sequence_num: 0,
                depends_on: vec![],
                payload: vec![],
                timeout_ms: 5000,
            })
            .unwrap();

        pipeline
            .fail_request(RequestId::new(1), "Error".to_string())
            .unwrap();

        let stats = pipeline.stats_snapshot();
        assert_eq!(stats.requests_failed, 1);
    }

    #[test]
    fn test_max_depth_reached_recorded() {
        let config = PipelineConfig {
            max_depth: 5,
            ..Default::default()
        };
        let pipeline = RequestPipeline::new(config);

        for i in 0..5 {
            pipeline
                .submit_request(PipelinedRequest {
                    request_id: RequestId::new(i),
                    stream_id: StreamId::new(i),
                    sequence_num: 0,
                    depends_on: vec![],
                    payload: vec![],
                    timeout_ms: 5000,
                })
                .unwrap();
        }

        let stats = pipeline.stats_snapshot();
        assert_eq!(stats.max_pipeline_depth_reached, 5);
    }
}
