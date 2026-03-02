# A7 Gateway Integration Guide

This guide covers integrating the Protocol Gateway (A7) with the ClaudeFS metadata service (A2) and transport layer (A4).

## Prerequisites

- A2 metadata service running and accessible
- A4 transport layer configured (RDMA or TCP)
- Network connectivity on required ports

## Connecting to A2 Metadata Servers

### Configuration

The gateway requires a list of metadata server endpoints:

```yaml
# gateway.yaml
metadata_servers:
  - address: "meta-1.claudefs.internal:7001"
    region: "us-west-2"
  - address: "meta-2.claudefs.internal:7001"
    region: "us-west-2"
  - address: "meta-3.claudefs.internal:7001"
    region: "us-east-1"
```

### Initial Connection Test

```bash
# Test metadata server connectivity
nc -zv meta-1.claudefs.internal 7001
nc -zv meta-2.claudefs.internal 7001

# Test with timeout
timeout 5 bash -c 'cat < /dev/null > /dev/tcp/meta-1.claudefs.internal/7001' && echo "OK"
```

### Authentication

A2 uses token-based authentication. Configure the gateway with a service account:

```yaml
auth:
  token_file: /etc/claudefs/gateway.token
  token_refresh_interval: 3600
```

The token file format:
```
<service_account_id>:<secret_key>
```

## Connecting to A4 Transport Layer

### Transport Selection

The gateway automatically selects transport based on hardware availability:

1. **RDMA (preferred)**: If InfiniBand/RoCE NICs detected
2. **TCP (fallback)**: Always available

### RDMA Configuration

```yaml
transport:
  type: rdma
  provider: auto  # auto-detect: verbs, ucx, etc.
  device: mlx5_0:1  # optional: specific RDMA device
```

Test RDMA connectivity:
```bash
# List available RDMA devices
ibv_devices

# Test RDMA connection
rping -s -a <local_ip> -v &
rping -c -a <remote_ip> -v
```

### TCP Configuration

```yaml
transport:
  type: tcp
  bind_address: "0.0.0.0"
  connect_timeout_ms: 5000
  keepalive: true
  keepalive_interval_ms: 30000
```

For high-throughput environments, enable zero-copy:
```yaml
transport:
  type: tcp
  zero_copy: true
  send_buffer_size: 2097152   # 2MB
  recv_buffer_size: 2097152
```

## NFS Export Configuration

### Basic Export

```yaml
nfs:
  bind_address: "0.0.0.0"
  port: 2049
  mount_bind:
    address: "0.0.0.0"
    port: 20048
  
  exports:
    - path: "/export"
      read_only: false
      root_squash: true
      anon_uid: 65534
      anon_gid: 65534
```

### Per-Directory Permissions

```yaml
exports:
  - path: "/export/public"
    allowed_clients:
      - "10.0.0.0/8"
      - "192.168.0.0/16"
    read_only: true
    root_squash: true
  
  - path: "/export/private"
    allowed_clients:
      - "10.1.0.0/24"
    read_only: false
    root_squash: false
  
  - path: "/export/enterprise"
    allowed_clients:
      - "corp.example.com"
    read_only: false
    root_squash: false
    squash_to_uid: 1000
    squash_to_gid: 1000
```

### Testing NFS Integration

```bash
# Verify mount daemon is listening
rpcinfo -p localhost | grep mountd

# Test mount protocol
showmount -e localhost

# Mount from client
mount -t nfs gateway.claudefs.local:/export /mnt/claudefs

# Test basic operations
touch /mnt/claudefs/testfile
ls -la /mnt/claudefs/
umount /mnt/claudefs
```

## S3 Bucket Configuration

### Basic S3 Setup

```yaml
s3:
  bind_address: "0.0.0.0"
  port: 9000
  region: "us-west-2"
  
  # Disable versioning for better performance
  enable_versioning: false
  
  # Maximum single object size (5TB default)
  max_object_size: 5497558138880
  
  # Minimum multipart chunk size (5MB)
  multipart_chunk_min: 5242880
```

### Bucket Configuration

Buckets are created dynamically from the namespace. Configure bucket policies:

```yaml
s3:
  bucket_policies:
    data-lake:
      versioning: true
      default_storage_class: STANDARD
      
      lifecycle:
        - name: "archive-old"
          prefix: "logs/"
          transition_days: 30
          storage_class: GLACIER
      
    analytics:
      versioning: false
      default_storage_class: STANDARD_IA
      
      cors:
        - allowed_origin: "https://analytics.example.com"
          allowed_methods: [GET, PUT]
          allowed_headers: ["*"]
```

### Testing S3 Integration

```bash
# Using AWS CLI with custom endpoint
aws --endpoint-url=http://gateway.claudefs.local:9000 \
    s3 ls s3://my-bucket/

# Create bucket
aws --endpoint-url=http://gateway.claudefs.local:9000 \
    s3 mb s3://test-bucket

# Upload file
aws --endpoint-url=http://gateway.claudefs.local:9000 \
    s3 cp myfile.txt s3://test-bucket/

# Test multipart upload
aws --endpoint-url=http://gateway.claudefs.local:9000 \
    s3 cp largefile s3://test-bucket/ \
    --storage-class STANDARD_IA
```

## SMB3/Samba VFS Plugin Configuration

### Enable SMB Support

```yaml
smb:
  enabled: true
  bind_address: "0.0.0.0"
  port: 445
  
  # Domain configuration
  workgroup: "CLAUDEFS"
  realm: "claudefs.local"
  
  # User mapping
  guest_ok: false
  valid_users: "@claudefs-users"
```

### Multi-Channel Configuration

For high-throughput workloads:

```yaml
smb:
  multichannel:
    enabled: true
    max_channels: 4
    min_channels: 2
    prefer_rdma: true
    
    interfaces:
      - name: eth0
        ip: 10.0.0.5
        speed_mbps: 10000
        rdma: true
      - name: eth1
        ip: 10.0.1.5
        speed_mbps: 10000
        rdma: true
```

### Testing SMB Integration

```bash
# Test connection
smbclient -L //gateway.claudefs.local -U username

# Mount share
mount -t cifs //gateway.claudefs.local/share /mnt -o user=username

# Test from Windows
\\gateway.claudefs.local\share
```

## Protocol Testing After Integration

### Health Checks

```bash
# NFS health
rpcinfo -p localhost | grep nfs
showmount -e localhost

# S3 health
curl http://localhost:9000/

# SMB health
smbstatus

# Gateway internal health
curl http://localhost:8080/health
```

### Integration Test Suite

```bash
# Run integration tests
cargo test --package claudefs-gateway --test integration

# Test specific protocol
cargo test --package claudefs-gateway nfs
cargo test --package claudefs-gateway s3
cargo test --package claudefs-gateway smb
```

### Load Testing

```bash
# NFS load test
fio --name=nfs-test --rw=randwrite --bs=4k --ioengine=libaio \
    --direct=1 --numjobs=4 --runtime=60 --time_based \
    --filename=/mnt/claudefs/testfile

# S3 load test
aws s3 cp large-file s3://test-bucket/ --expected-size=$(stat -c%s large-file)
```

## Troubleshooting Integration Issues

### Cannot Connect to A2

1. Check metadata server endpoints:
   ```bash
   cat /etc/claudefs/gateway.yaml | grep -A5 metadata_servers
   ```

2. Verify network:
   ```bash
   telnet meta-1.claudefs.internal 7001
   ```

3. Check authentication token:
   ```bash
   cat /etc/claudefs/gateway.token
   ```

### Cannot Connect to A4

1. Verify transport type:
   ```bash
   # Check for RDMA devices
   ibv_devices
   
   # Check TCP connectivity
   netstat -i | grep eth
   ```

2. Check firewall:
   ```bash
   iptables -L -n | grep 7001
   ```

### NFS Mount Fails

1. Check portmap:
   ```bash
   rpcinfo -p localhost
   ```

2. Check exports:
   ```bash
   showmount -e localhost
   ```

3. Check SELinux (if applicable):
   ```bash
   getsebool -a | grep nfs
   ```

## Next Steps

- [Performance Tuning](PERFORMANCE_TUNING.md) - Optimize for production
- [Operations Runbook](OPERATIONS_RUNBOOK.md) - Day-to-day management
- [Protocol Notes](PROTOCOL_NOTES.md) - Protocol-specific details