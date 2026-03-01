# ClaudeFS Security Hardening Guide

**Phase 3 Production Security:** Comprehensive security hardening procedures for production deployments.

---

## Security Architecture Overview

ClaudeFS security is built on three layers:

1. **Network Security** — mTLS everywhere, firewall segmentation
2. **Authentication & Authorization** — certificate-based and/or Kerberos
3. **Data Security** — encryption at-rest (AES-GCM) and in-transit (TLS 1.3)

---

## Pre-Deployment Security Checklist

### AWS Environment
- [ ] AWS account hardened (MFA enabled, minimal IAM policies)
- [ ] Security groups restrict to necessary ports only (9400 Raft, 9401 RPC, etc.)
- [ ] VPC flow logs enabled (monitor suspicious traffic)
- [ ] CloudTrail logging enabled for audit
- [ ] S3 bucket versioning enabled (immutability against ransomware)
- [ ] S3 bucket encryption enabled (KMS or AES-256)
- [ ] EBS encryption enabled by default
- [ ] Secrets Manager and Systems Manager Parameter Store configured

### Cluster Certificates
- [ ] Cluster CA generated and backed up securely
- [ ] CA key stored in AWS Secrets Manager (encrypted)
- [ ] Each node certificate valid for 1 year
- [ ] Certificate rotation process documented and tested
- [ ] Client certificate enrollment tokens generated and distributed securely

### Access Control
- [ ] SSH key pairs generated (4096-bit RSA minimum)
- [ ] SSH keys stored in AWS Secrets Manager or local HSM
- [ ] SSH access restricted to orchestrator + on-call ops
- [ ] Bastion host configured (optional, recommended for large teams)
- [ ] Break-glass emergency access procedure documented

### Audit & Compliance
- [ ] Audit logging requirements documented
- [ ] Log destination configured (ELK, Splunk, or S3 + Athena)
- [ ] Log retention policy enforced (compliance requirement)
- [ ] Encryption key audit trail enabled
- [ ] Change management process in place

---

## Certificate and Key Management

### Cluster CA Generation (One-Time)
```bash
# 1. Generate CA key (stored encrypted in AWS Secrets Manager)
openssl genrsa -out ca-key.pem 4096

# 2. Generate CA certificate (valid 10 years)
openssl req -new -x509 -days 3650 -key ca-key.pem -out ca-cert.pem \
  -subj "/CN=claudefs-cluster-ca/O=ClaudeFS/C=US"

# 3. Back up CA key and certificate
aws secretsmanager create-secret --name cfs/ca-key \
  --secret-string file://ca-key.pem --region us-west-2

aws secretsmanager create-secret --name cfs/ca-cert \
  --secret-string file://ca-cert.pem --region us-west-2

# 4. Securely delete local copies
shred -vfz ca-key.pem ca-cert.pem
```

### Node Certificate Generation (Per Node)
```bash
# 1. Generate node key
openssl genrsa -out node-key.pem 2048

# 2. Create certificate signing request
openssl req -new -key node-key.pem -out node.csr \
  -subj "/CN=storage-node-1/O=ClaudeFS/C=US"

# 3. Sign certificate with CA (valid 365 days)
# Note: In production, this happens on the cluster CA node
cfs admin sign-cert --csr node.csr --output node-cert.pem --valid-days 365

# 4. Deploy to node
scp -i ~/.ssh/cfs-key node-cert.pem ec2-user@storage-node-1:/etc/cfs/node-cert.pem
scp -i ~/.ssh/cfs-key node-key.pem ec2-user@storage-node-1:/etc/cfs/node-key.pem

# 5. Verify certificate
openssl x509 -in node-cert.pem -text -noout
```

### Client Certificate Enrollment (Per Client)
```bash
# 1. On orchestrator, generate one-time enrollment token
TOKEN=$(cfs admin issue-enrollment-token --ttl 1h --user alice)

# 2. Distribute token securely (email, message, etc.)
echo "Alice's enrollment token: $TOKEN"

# 3. On client, enroll to cluster
cfs mount --token $TOKEN /mnt/claudefs

# 4. Client certificate is stored at ~/.cfs/client.crt
# 5. Subsequent mounts use mTLS automatically (no token needed)
```

### Certificate Rotation

**Automated (Recommended):**
```bash
# ClaudeFS rotates certificates automatically 30 days before expiry
cfs admin cert-status  # Check expiry dates

# Monitor for rotation status
cfs admin cert-rotation-status
```

**Manual Rotation:**
```bash
# 1. Generate new certificate for node (30 days before expiry)
cfs admin gen-cert --hostname storage-node-1 --ttl 365 \
  --output /tmp/node-new-cert.pem

# 2. Verify new cert is valid and signed by current CA
openssl x509 -in /tmp/node-new-cert.pem -noout -text

# 3. Deploy without downtime (hot-swap)
cfs admin rotate-cert --node storage-node-1 \
  --new-cert /tmp/node-new-cert.pem

# 4. Verify rotation complete
cfs admin cert-status
```

### Emergency Certificate Revocation
```bash
# If a client certificate is compromised:

# 1. Revoke immediately
cfs admin revoke-cert --cert-id alice-laptop

# 2. Distribution via SWIM gossip (< 1 minute)
# 3. Client must re-enroll with new token

# To re-enroll compromised client:
cfs admin issue-enrollment-token --user alice --ttl 1h
# (new one-time token, different from compromised cert)
```

---

## Network Segmentation and Firewall

### Security Group Configuration (AWS)

**ClaudeFS Storage Nodes:**
```
Ingress:
  - Port 9400/tcp from <security-group-id> (Raft peer-to-peer)
  - Port 9401/tcp from <security-group-id> (RPC server)
  - Port 9402/tcp from <client-sg-id> (Data stream)
  - Port 9403/tcp from <monitoring-sg-id> (Prometheus exporter)
  - Port 22/tcp from <bastion-sg-id> (SSH)

Egress:
  - All to <security-group-id> (intra-cluster)
  - Port 443/tcp to 0.0.0.0/0 (S3, public internet)
  - Port 53/udp to 0.0.0.0/0 (DNS)
```

**FUSE Clients:**
```
Ingress:
  - Port 22/tcp from <bastion-sg-id> (SSH)

Egress:
  - Port 9401/tcp to <storage-sg-id> (RPC)
  - Port 9402/tcp to <storage-sg-id> (Data)
  - Port 443/tcp to 0.0.0.0/0 (Optional: cloud CA for certificates)
```

**Monitoring (Prometheus/Grafana):**
```
Ingress:
  - Port 9090/tcp from <admin-sg-id> (Prometheus UI)
  - Port 3000/tcp from <admin-sg-id> (Grafana UI)

Egress:
  - Port 9403/tcp to <storage-sg-id> (Prometheus scrape)
  - Port 443/tcp to 0.0.0.0/0 (Optional: Grafana cloud)
```

### Network ACLs (Optional, Additional Layer)
```bash
# Block suspicious traffic patterns
# Detect port scanning, DDoS attempts

# Example: Drop packets with unusual TCP flags
aws ec2 create-network-acl-entry --network-acl-id acl-xxx \
  --rule-number 100 --protocol tcp --port-range 1-65535 \
  --cidr-block 0.0.0.0/0 --ingress --egress
```

---

## TLS and Encryption Configuration

### TLS 1.3 Enforcement
In `config.toml`:
```toml
[tls]
min_version = "TLSv1.3"          # Enforce TLS 1.3 (no fallback)
cipher_suites = [
  "TLS_AES_256_GCM_SHA384",
  "TLS_AES_128_GCM_SHA256",
  "TLS_CHACHA20_POLY1305_SHA256"
]
```

### Data-at-Rest Encryption
```toml
[encryption]
data_encryption = "aes-gcm-256"  # AES-256-GCM
key_derivation = "hkdf-sha256"   # HKDF key derivation
key_rotation_days = 90           # Rotate keys every 90 days

[s3_tiering]
s3_encryption = "aes-256"        # S3-side encryption
```

### mTLS Configuration (Node-to-Node)
In `config.toml`:
```toml
[mtls]
enabled = true
cert_path = "/etc/cfs/node-cert.pem"
key_path = "/etc/cfs/node-key.pem"
ca_cert_path = "/etc/cfs/ca-cert.pem"
verify_client = true             # Verify all peers
verify_hostname = true           # Verify certificate CN
```

### Client Authentication Options

**Option 1: mTLS (Certificate-Based)**
```toml
[client_auth]
method = "mtls"
ca_cert_path = "/etc/cfs/ca-cert.pem"
```

**Option 2: Kerberos (Active Directory)**
```toml
[client_auth]
method = "kerberos"
realm = "CORP.EXAMPLE.COM"
keytab_path = "/etc/cfs/krb5.keytab"
```

**Option 3: Hybrid (Both Supported)**
```toml
[client_auth]
methods = ["mtls", "kerberos"]  # Client can use either
```

---

## Access Control and Permissions

### File-Level Permissions (POSIX)
```bash
# ClaudeFS respects traditional POSIX permissions
# Standard Linux chmod/chown apply

# Example: Restrict file access
chmod 600 /mnt/claudefs/secrets.db        # Owner only
chown alice:finance /mnt/claudefs/report  # Change ownership
```

### Directory Quotas (Multi-Tenancy)
```bash
# Set quotas per user/team
cfs admin set-quota --path /mnt/claudefs/alice --size 1TB
cfs admin set-quota --path /mnt/claudefs/bob --size 500GB

# Monitor quota usage
cfs admin quota-stats
```

### WORM (Write-Once, Read-Many) Protection
```bash
# Lock a file to prevent modifications
cfs admin worm-lock /mnt/claudefs/audit-log.txt

# Verify WORM status
cfs admin worm-status /mnt/claudefs/audit-log.txt

# Unlocking requires administrative override (with audit trail)
cfs admin worm-unlock /mnt/claudefs/audit-log.txt --reason "Audit retention expired"
```

---

## Audit Logging

### ClaudeFS Audit Log Configuration
In `config.toml`:
```toml
[audit]
enabled = true
log_all_operations = true        # Log every metadata mutation
log_level = "info"               # Minimum level to log
destination = "syslog"           # syslog, file, or both

[audit_syslog]
server = "syslog.example.com"
port = 514
facility = "local0"
protocol = "tcp"                 # Use TCP for reliability
```

### Audit Log Format
```
[2026-03-01T12:34:56Z] operation=create_file user=alice path=/data/project-x/file.txt
  result=success inode=1234567 size=1024 permissions=644 parent=1234566

[2026-03-01T12:35:10Z] operation=setattr user=alice path=/data/project-x/file.txt
  attr=permissions old_perms=644 new_perms=755 result=success
```

### Log Retention and Archive
```bash
# Retention policy (3 years for compliance)
cfs admin audit-config --retention-days 1095

# Archive old logs to S3 + Glacier
cfs admin audit-archive --before 2025-01-01 --destination s3://cfs-logs/archive
```

### Integration with ELK Stack (Optional)
```bash
# Ship logs to Elasticsearch
[audit_elasticsearch]
enabled = true
hosts = ["elasticsearch-1.example.com:9200", "elasticsearch-2:9200"]
index = "cfs-audit-logs"
username = "cfs-user"
password_secret = "cfs/elk-password"  # From AWS Secrets Manager
```

---

## Authentication Bypass Prevention

### SSH Hardening
```bash
# On each storage node, configure sshd:

# /etc/ssh/sshd_config
PermitRootLogin no
PasswordAuthentication no         # Keys only
PubkeyAuthentication yes
AuthorizedKeysFile .ssh/authorized_keys
Protocol 2                        # SSH v2 only
X11Forwarding no
MaxAuthTries 3                    # Limit login attempts
LoginGraceTime 30s                # Timeout idle connections

# Restart SSH
sudo systemctl restart sshd
```

### sudo Hardening (for emergencies)
```bash
# Only allow specific users, require password
# /etc/sudoers (edit with visudo)

%ops ALL=(ALL) ALL NOPASSWD:ALL         # Ops team (no password)
alice ALL=(ALL) ALL PASSWD:ALL          # Require password for alice

# Log all sudo commands
Defaults use_pty
Defaults syslog="local1"
```

### Breakglass Access (Emergency)
```bash
# One-time emergency access procedure (document and secure)

# 1. Verify authorization with 2+ admins
# 2. Issue temporary SSH key (30 min TTL)
cfs admin issue-breakglass-key --ttl 30m --reason "Emergency data recovery" \
  --approvers alice,bob > breakglass-key.pem

# 3. SSH with breakglass key
ssh -i breakglass-key.pem ec2-user@storage-1

# 4. Log all actions (syslog, audit)
# 5. Key automatically expires after TTL
```

---

## Secrets Management

### Encryption Keys Storage
```bash
# Never store keys in code or config files
# Use AWS Secrets Manager instead

# Store cluster CA key
aws secretsmanager create-secret --name cfs/ca-key \
  --secret-string "$(cat ca-key.pem)" \
  --kms-key-id alias/cfs-secrets \
  --region us-west-2

# Retrieve when needed
aws secretsmanager get-secret-value --secret-id cfs/ca-key \
  --region us-west-2 --query 'SecretString'
```

### S3 Credentials Management
```bash
# Use IAM role credentials (not access keys)
# In config.toml:

[s3]
# Don't specify access_key/secret_key; use IAM role
use_iam_role = true
role_arn = "arn:aws:iam::123456789012:role/cfs-s3-writer"
```

### Database Passwords (KV Store Admin)
```bash
# Store in AWS Secrets Manager
# On startup, retrieve and use in memory only

cfs admin kv-config --password-secret cfs/kv-admin-password
```

---

## Vulnerability Scanning and Patching

### Dependency Auditing
```bash
# Weekly scan for known vulnerabilities
cargo audit

# Update vulnerable dependencies
cargo update [crate-name]
```

### System Patching Strategy
```bash
# 1. Weekly OS updates on non-production test cluster
# 2. Monthly patching schedule for production
# 3. Emergency patching for critical CVEs (< 24 hours)

# Example: Emergency kernel patch
sudo yum update -y kernel
sudo shutdown -r now  # Schedule downtime during maintenance window
```

### Security Policy
```
- Critical CVEs (CVSS > 9.0): Fix within 24 hours, emergency downtime if needed
- High CVEs (CVSS 7-9): Fix within 1 week, scheduled maintenance
- Medium CVEs (CVSS 4-7): Fix within 30 days, normal patching
- Low CVEs (CVSS < 4): Fix within 90 days, bundled with release
```

---

## Encryption Key Rotation

### Automatic Key Rotation
In `config.toml`:
```toml
[encryption]
key_rotation_enabled = true
key_rotation_days = 90          # Rotate every 90 days
key_rotation_overlap_hours = 24 # Overlap window for in-flight operations
```

### Manual Key Rotation
```bash
# 1. Generate new encryption key
cfs admin generate-key --algorithm aes-256 --output /tmp/new-key.bin

# 2. Initiate rotation (data is re-encrypted in background)
cfs admin rotate-key --new-key-path /tmp/new-key.bin

# 3. Monitor rotation progress
cfs admin key-rotation-status

# 4. Verify all data encrypted with new key
cfs admin verify-encryption
```

---

## Security Incident Response

### Detection
- Monitor logs for repeated authentication failures
- Alert on unusual access patterns (off-hours, bulk operations)
- Monitor for privilege escalation attempts
- Track certificate revocations

### Containment
```bash
# 1. Revoke compromised certificate immediately
cfs admin revoke-cert --cert-id compromised-client

# 2. Disconnect compromised client
cfs admin disconnect-client --cert-id compromised-client

# 3. Check audit logs for unauthorized access
cfs admin audit-search --user compromised-client --since 24h

# 4. Disable user account (if applicable)
cfs admin user-disable alice  # Prevent re-login
```

### Investigation
```bash
# 1. Export audit logs for forensics
cfs admin audit-export --since 7d --format json > incident-audit.json

# 2. Analyze with grep/jq
jq '.[] | select(.operation == "unlink")' incident-audit.json

# 3. Notify stakeholders
# 4. File security incident report
```

### Recovery
```bash
# 1. If data was deleted, restore from snapshot
cfs admin restore-snapshot --snapshot metadata-20260228.tar.gz

# 2. Audit restored data
cfs admin verify-integrity

# 3. Re-issue clean certificates
# 4. Train users on security best practices
```

---

## Security Best Practices

### For Operators
- [ ] Store SSH keys in secure location (password-protected or HSM)
- [ ] Use breakglass emergency access procedure (not regular sudo)
- [ ] Enable MFA on AWS account and GitHub
- [ ] Review audit logs weekly
- [ ] Test disaster recovery procedures monthly
- [ ] Rotate certificates every 365 days
- [ ] Update system packages monthly

### For Developers
- [ ] Never commit secrets or keys to Git (use .gitignore)
- [ ] Use AWS Secrets Manager for all credentials
- [ ] Enable TLS 1.3 for all connections
- [ ] Validate all inputs (prevent injection attacks)
- [ ] Review unsafe blocks quarterly (A10 responsibility)
- [ ] Fuzz network protocol regularly (A10 responsibility)

### For Cluster Owners
- [ ] Document all custom configurations
- [ ] Maintain an asset inventory (nodes, certificates, accounts)
- [ ] Conduct security reviews annually
- [ ] Plan for key personnel transitions
- [ ] Document incident response procedures
- [ ] Maintain backup of cluster CA key
- [ ] Test backup/restore procedures quarterly

---

## Compliance and Audit

### Regulatory Requirements
- **HIPAA:** Encryption at-rest/transit, access controls, audit logging
- **SOC 2:** Documented procedures, audit trails, disaster recovery
- **GDPR:** Data subject access, right-to-be-forgotten (snapshots), data residency
- **PCI DSS:** Network segmentation, encryption, access logs

### Audit Trail Example
```bash
# Export audit logs for compliance review
cfs admin audit-export --since 2026-01-01 --until 2026-03-31 \
  --format csv --output q1-audit-logs.csv

# Key fields for compliance:
# - Timestamp, operation, user, resource, result, IP address, client cert ID
```

---

## Security Hardening Checklist (Production)

- [ ] Cluster CA key stored in AWS Secrets Manager (encrypted)
- [ ] All inter-node communication uses mTLS 1.3
- [ ] All client connections require mTLS or Kerberos
- [ ] Security groups restrict ports to necessary services only
- [ ] SSH access restricted to bastion host + on-call team
- [ ] Audit logging enabled and centralized (ELK or S3)
- [ ] Log retention policy enforced (3 years minimum)
- [ ] S3 bucket versioning and encryption enabled
- [ ] Encryption keys rotated annually
- [ ] Certificates rotated annually (automated)
- [ ] Secrets stored in AWS Secrets Manager (not in code/config)
- [ ] Breakglass emergency access procedure documented and tested
- [ ] Vulnerability scanning (cargo audit) weekly
- [ ] System patching scheduled monthly
- [ ] Disaster recovery tested quarterly
- [ ] Security incident response plan documented
- [ ] Compliance audit scheduled annually

