# ClaudeFS Monitoring Stack

Local monitoring infrastructure using Docker Compose.

## Quick Start

```bash
cd monitoring
docker-compose up -d
```

## Services

| Service | Port | URL | Notes |
|---------|------|-----|-------|
| Prometheus | 9090 | http://localhost:9090 | Metrics scraper & TSDB |
| Grafana | 3000 | http://localhost:3000 | Dashboards & alerts (admin/admin) |
| Jaeger | 16686 | http://localhost:16686 | Distributed tracing |
| Loki | 3100 | http://localhost:3100 | Log aggregation |
| Alertmanager | 9093 | http://localhost:9093 | Alert routing |

## Configuration Files

- `prometheus.yml` — Scrape targets and rule files
- `alerts.yml` — Alert rules (CPU, memory, latency, etc.)
- `alertmanager.yml` — Alert routing (Slack, PagerDuty)
- `docker-compose.yml` — Service definitions
- `loki-config.yml` — Log ingestion configuration
- `promtail-config.yml` — Log shipping from agents

## Starting ClaudeFS with Metrics

Set environment variables:

```bash
export PROMETHEUS_PUSHGATEWAY=http://localhost:9091
export JAEGER_AGENT_HOST=localhost
export JAEGER_AGENT_PORT=6831
export LOKI_API_URL=http://localhost:3100
```

Then run ClaudeFS (metrics will be scraped at port 9001-9008).

## Viewing Dashboards

1. Navigate to http://localhost:3000
2. Login with admin/admin
3. Add Prometheus datasource: http://prometheus:9090
4. Import dashboards from `grafana/provisioning/dashboards/`

## Useful Queries

```promql
# CPU usage over time
rate(process_cpu_seconds_total[5m])

# Memory usage
process_resident_memory_bytes

# I/O latency histogram
histogram_quantile(0.99, rate(storage_read_latency_us_bucket[5m]))

# Error rate
rate(module_errors_total[5m])

# Disk usage
node_filesystem_avail_bytes / node_filesystem_size_bytes
```

## Cleanup

```bash
docker-compose down -v
```

## Troubleshooting

**Prometheus targets show "DOWN":**
- Ensure ClaudeFS is running and exporting metrics on configured ports
- Check firewall/network connectivity

**Grafana datasource error:**
- Verify Prometheus is accessible from Grafana container
- Check `prometheus:9090` in Grafana (not localhost)

**Alerts not firing:**
- Verify alert rules syntax: `promtool check rules alerts.yml`
- Check Prometheus alerts UI at http://localhost:9090/alerts

## Documentation

See `../docs/OBSERVABILITY.md` for architecture and setup details.
