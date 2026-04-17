[0m[0m> build · accounts/fireworks/models/minimax-m2p5


[0m✱ [0mGlob "monitoring/grafana/provisioning/dashboards/json/*.json"[90m 4 matches[0m
[0m→ [0mRead monitoring/grafana/provisioning/dashboards/json/01-cluster-health.json
[0m
[0m← [0mWrite monitoring/grafana/provisioning/dashboards/json/05-data-reduction.json
Wrote file successfully.

[0m[0m← [0mWrite monitoring/grafana/provisioning/dashboards/json/06-replication.jsonWrote file successfully.


[0m[0m← [0mWrite monitoring/grafana/provisioning/dashboards/json/07-transport.jsonWrote file successfully.


[0m[0m← [0mWrite monitoring/grafana/provisioning/dashboards/json/08-fuse-gateway.jsonWrote file successfully.
[0m

[0m✱ [0mGlob "monitoring/grafana/provisioning/dashboards/json/*.json"[90m 8 matches[0m
Created all 4 missing Grafana dashboards:

| Dashboard | File | Panels |
|-----------|------|--------|
| Data Reduction | `05-data-reduction.json` | Dedup ratio, compression ratio, tiering activity, pipeline latencies, reduction efficiency |
| Replication | `06-replication.json` | Cross-site lag, failover count, site health status, latency histogram, conflict rate, lag trend |
| Transport | `07-transport.json` | RPC latency p95, bandwidth utilization, connection pool, method latency breakdown, RDMA vs TCP, error rate |
| FUSE & Gateway | `08-fuse-gateway.json` | FUSE ops/sec, cache hit ratio, passthrough %, protocol distribution, error rate, syscall latency, quota usage, SMB connections |

All 8 crates now have corresponding Grafana dashboards:
- 01: Cluster Health (storage, metadata, transport, fuse, repl, gateway, mgmt)
- 02: Storage Performance
- 03: Metadata Consensus
- 04: Cost Tracking
- 05: Data Reduction (claudefs-reduce)
- 06: Replication (claudefs-repl)
- 07: Transport (claudefs-transport)
- 08: FUSE & Gateway (claudefs-fuse, claudefs-gateway)
