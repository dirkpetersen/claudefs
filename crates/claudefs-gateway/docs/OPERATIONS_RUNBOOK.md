# A7 Gateway Operations Runbook

## Day-1: Pre-Deployment Checklist

### Networking

- [ ] All required ports are available:
  - NFS: 2049 (TCP)
  - MOUNT: 20048 (TCP)  
  - S3: 9000 (TCP)
  - SMB: 445 (TCP)
  - Health: 8080 (TCP)
- [ ] Firewall rules configured for:
  - Client networks to gateway ports
  - Gateway to A2 metadata servers (port 7001)
  - Gateway to A4 transport (port 7002)
- [ ] DNS resolution works for:
  - Metadata server hostnames
  - Gateway hostname (for S3 endpoints)

### TLS Certificates

For S3 with HTTPS:
```bash
# Generate self-signed certificate
openssl req -x509 -nodes -days 365 -newkey rsa:2048 \
    -keyout key.pem -out cert.pem \
    -subj "/CN=gateway.claudefs.local"

# Verify certificate
openssl x509 -in cert.pem -text -noout
```

### Configuration Validation

```bash
# Validate configuration file
./cfs gateway validate-config --config /etc/claudefs/gateway.yaml

# Check for port conflicts
netstat -tuln | grep -E '(2049|20048|9000|445|8080)'
```

### Dependencies

- [ ] A2 metadata service is running and healthy
- [ ] A4 transport layer is accessible
- [ ] Authentication token is valid

## Startup Procedures

### Starting NFS

```bash
# Start NFS server
./cfs gateway start nfs \
    --config /etc/claudefs/gateway.yaml \
    --log-level info

# Or with specific options
./cfs gateway start nfs \
    --bind 0.0.0.0:2049 \
    --mount-bind 0.0.0.0:20048 \
    --export /export
```

### Starting S3

```bash
# Start S3 API server
./cfs gateway start s3 \
    --config /etc/claudefs/gateway.yaml \
    --bind 0.0.0.0:9000

# Enable HTTPS
./cfs gateway start s3 \
    --config /etc/claudefs/gateway.yaml \
    --tls-cert /etc/claudefs/cert.pem \
    --tls-key /etc/claudefs/key.pem
```

### Starting SMB

```bash
# Start SMB server
./cfs gateway start smb \
    --config /etc/claudefs/gateway.yaml \
    --domain CLAUDEFS \
    --workgroup CLAUDEFS
```

### Systemd Service (Recommended)

```ini
# /etc/systemd/system/claudefs-gateway.service
[Unit]
Description=ClaudeFS Protocol Gateway
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/cfs gateway start \
    --config /etc/claudefs/gateway.yaml
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
# Enable and start
systemctl enable claudefs-gateway
systemctl start claudefs-gateway
systemctl status claudefs-gateway
```

## Health Check Procedures

### Built-in Health Endpoint

```bash
# Get health status
curl http://localhost:8080/health

# Response format:
# {
#   "overall": "healthy",
#   "checks": [
#     {"name": "metadata", "status": "healthy", "message": "", "duration_ms": 5},
#     {"name": "transport", "status": "healthy", "message": "", "duration_ms": 2},
#     {"name": "nfs", "status": "healthy", "message": "", "duration_ms": 1},
#     {"name": "s3", "status": "healthy", "message": "", "duration_ms": 1}
#   ],
#   "timestamp": 1706745600
# }
```

### Protocol-Specific Health Checks

```bash
# NFS health
rpcinfo -p localhost | grep -E '(nfs|mount)'
showmount -e localhost

# S3 health  
curl -s http://localhost:9000/ | head -5

# SMB health
smbstatus -L
```

### Automated Health Monitoring

```bash
# Add to crontab for 1-minute health checks
*/1 * * * * curl -sf http://localhost:8080/health >/dev/null || \
    (echo "Gateway unhealthy at $(date)" | mail -s "Alert" admin@example.com)
```

## Monitoring via Prometheus Metrics

### Key Metrics

| Metric | Description | Alert Threshold |
|--------|-------------|-----------------|
| `gateway_requests_total` | Total requests | N/A |
| `gateway_errors_total` | Total errors | > 1% of requests |
| `gateway_latency_p99_us` | p99 latency | > 10ms for NFS |
| `gateway_active_connections` | Active connections | > 90% max pool |
| `gateway_backend_errors_total` | Backend errors | > 0 for 5min |
| `gateway_circuit_breaker_state` | Circuit state | = 1 (open) |

### Prometheus Scrape Config

```yaml
scrape_configs:
  - job_name: 'claudefs-gateway'
    static_configs:
      - targets: ['localhost:8080']
    metrics_path: /metrics
    scrape_interval: 15s
```

### Sample Queries

```promql
# Request rate by protocol
rate(gateway_requests_total[5m]) by (protocol)

# Error rate
rate(gateway_errors_total[5m]) / rate(gateway_requests_total[5m])

# p99 latency
histogram_quantile(0.99, rate(gateway_latency_seconds_bucket[5m]))

# Active connections vs max
gateway_active_connections / gateway_max_connections
```

## Troubleshooting Common Issues

### High Latency

#### Diagnosis Flow

```
High Latency?
    ├─ Check Gateway CPU: top/htop
    │   └─ If > 80%: Scale horizontally or tune worker threads
    ├─ Check A2 Metadata: curl /health of A2
    │   └─ If slow: Check A2 performance
    ├─ Check A4 Transport: 
    │   ├─ RDMA: ibv_devinfo, ibv_stat
    │   └─ TCP: netstat -c, ss -s
    └─ Check Network: 
        ├─ iftop, iperf3
        └─ Check for packet loss
```

#### Common Causes

1. **Cache miss storm**: Check `gateway_cache_hits_total` ratio
2. **Backend overload**: Check A2 metrics
3. **Network congestion**: Check interface errors
4. **Connection pool exhausted**: Check active connections

### Connection Failures

#### TLS Certificate Issues

```bash
# Check certificate validity
openssl s_client -connect gateway.claudefs.local:9000 \
    -showcerts </dev/null 2>/dev/null | openssl x509 -noout -dates

# Common error: "certificate verify failed"
# Fix: Ensure CA cert is in system trust store
sudo cp ca.crt /usr/local/share/ca-certificates/
sudo update-ca-certificates
```

#### Firewall Issues

```bash
# Test connectivity
nc -zv meta-1.claudefs.local 7001

# Check iptables rules
sudo iptables -L -n -v | grep 7001

# Add rule if needed
sudo iptables -A INPUT -p tcp -s 10.0.0.0/8 --dport 7001 -j ACCEPT
```

### Quota Enforcement Issues

#### Soft Limit Exceeded Warning

```
WARN: Soft quota exceeded for uid=1000 (usage=10GB/10GB)
```

This is informational. To change limits:
```bash
# Update quota
./cfs quota set-user --uid 1000 \
    --soft 20GB --hard 25GB
```

#### Hard Limit Enforcement

```
ERROR: Hard quota exceeded for uid=1000 (usage=12GB/12GB)
```

Options:
1. Delete data to free space
2. Request quota increase from admin
3. Enable grace period (if configured)

### ACL/Permission Issues

#### UID/GID Mapping

NFS clients may have different UID/GID mappings:
```bash
# Check client UID
id

# Check file ownership on server
ls -la /export/file

# If mismatch, configure NFSv4 idmapping
# /etc/idmapd.conf:
[General]
Domain = claudefs.local
```

#### SMB Access Denied

```bash
# Check valid users
testparm -s

# Check share permissions
smbclient -L //localhost -U username

# Verify user is in claudefs-users group
getent group claudefs-users
```

## Emergency Procedures

### Gateway Unresponsive

```bash
# Check process status
ps aux | grep claudefs-gateway

# Check for deadlocks
# Enable debug mode:
# Add to config: log_level: debug

# If stuck, restart
sudo systemctl restart claudefs-gateway
```

### Backend (A2/A4) Failures

When metadata or transport is unavailable:

1. **Circuit breakers will open** - requests rejected with error
2. **Check A2 status**: `curl http://meta-1:8080/health`
3. **Check A4 status**: `curl http://transport-1:8080/health`
4. **After recovery**, circuits auto-close

### Data Inconsistency

If you suspect data inconsistency:
```bash
# Force cache invalidation
curl -X POST http://localhost:8080/admin/cache/invalidate

# Verify file on backend
./cfs fsck /export/problematic-file
```

## Maintenance Windows

### Graceful Shutdown

```bash
# Stop accepting new connections
./cfs gateway drain

# Wait for active requests to complete (30s)
sleep 30

# Stop service
sudo systemctl stop claudefs-gateway
```

### Rolling Restart

For zero-downtime updates:
```bash
# On each gateway node:
systemctl stop claudefs-gateway
# Wait for load balancer to remove from pool
systemctl start claudefs-gateway
# Verify health before continuing
curl http://localhost:8080/health
```

## See Also

- [Architecture](ARCHITECTURE.md) - System design
- [Integration Guide](INTEGRATION_GUIDE.md) - Initial setup
- [Performance Tuning](PERFORMANCE_TUNING.md) - Optimization
- [Protocol Notes](PROTOCOL_NOTES.md) - Protocol details