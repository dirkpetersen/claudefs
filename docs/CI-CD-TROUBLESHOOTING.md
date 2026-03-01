# ClaudeFS CI/CD Troubleshooting Guide

**Last Updated:** 2026-03-01
**Scope:** GitHub Actions workflows, build failures, test failures, infrastructure issues

## Quick Diagnostics

### 1. Workflow Not Running

**Symptoms:** Pushed code but workflow doesn't start

**Checklist:**
```bash
# ✓ Check if workflows are in GitHub
# Go to: https://github.com/dirkpetersen/claudefs/actions
# Should show: ci-build, tests-all, integration-tests, release, deploy-prod

# ✓ Verify workflows are enabled
# Settings > Actions > Workflow Permissions > "Read and write permissions"

# ✓ Check branch protection rules
# Settings > Branches > main > Require workflows to pass

# ✓ Verify push triggers the right workflow
# Pushed to main? Check: on: [push]
# Opened PR? Check: on: [pull_request]

# ✓ Check YAML syntax
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci-build.yml'))"
```

**Resolution:**
- Workflow files must be in main branch `.github/workflows/` directory
- Workflows don't execute on pull requests from forks (security feature)
- Check GitHub Actions tab for error messages

---

### 2. Build Failure: "Cargo build failed"

**Symptoms:** `cargo build` fails in ci-build.yml

**Common Causes & Fixes:**

#### A. Compilation Error
```bash
# Error: "error[E0425]: cannot find function `foo` in this scope"

# Solution:
# 1. The function exists but wasn't re-exported
# 2. Run `cargo check` locally, fix the issue
# 3. Ensure OpenCode fixed the issue correctly
# 4. Commit and push
```

#### B. Dependency Version Conflict
```bash
# Error: "error: failed to resolve: use of undeclared type `X`"

# Solution:
# 1. Check Cargo.toml for version mismatches
# cargo tree --depth 3 | grep -i duplicates
# 2. If duplicate versions, update one crate's dependency
# 3. Run `cargo update` locally to verify
# 4. Commit Cargo.lock changes
```

#### C. Feature Not Enabled
```bash
# Error: "error[E0433]: cannot find crate `tokio`"

# Solution:
# 1. Check if dependency is in Cargo.toml
# grep "tokio" crates/*/Cargo.toml
# 2. If missing, add to [dependencies]
# 3. Check if feature flags are needed:
# tokio = { version = "1.0", features = ["macros", "rt"] }
```

#### D. Platform-Specific Issue
```bash
# Error: "error[E0425]: cannot find `libc::XXX` on this platform"

# Solution:
# 1. Use cfg() attributes for platform-specific code
# 2. Test locally: cargo test --target x86_64-unknown-linux-gnu
# 3. For ARM: cross compile with: cross build --target aarch64-unknown-linux-gnu
```

**Debug Workflow:**

1. Try locally first:
   ```bash
   cargo check           # Faster than build
   cargo build --release # Full build
   cargo build --target aarch64-unknown-linux-gnu  # Cross-compile
   ```

2. Check if other crates compile:
   ```bash
   cargo check -p claudefs-fuse  # Test single crate
   ```

3. Check for bad imports:
   ```bash
   grep -r "use.*::" crates/*/src/*.rs | grep "^[^:]*::" | head -20
   ```

---

### 3. Test Failure: "Tests failed"

**Symptoms:** `cargo test` passes locally but fails in CI

**Common Causes:**

#### A. Timing-Sensitive Tests
```bash
# Symptom: Test passes locally, fails randomly in CI

# Root cause: CI might be slower or have different thread scheduling
# Solution:
# 1. Add timeouts: #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
# 2. Use synchronization: futures::sync::Barrier instead of sleep()
# 3. Increase timeout values in CI
```

#### B. Environment Differences
```bash
# Symptom: Test fails on CI but not locally

# Root cause: Different kernel version, libraries, etc.
# Solution:
# 1. Check CI environment: /etc/os-release
# 2. Check locally: uname -r
# 3. Use containers to match environments
# 4. Add debug output to understand difference
```

#### C. Port Conflicts
```bash
# Symptom: "Address already in use" error

# Root cause: Previous test didn't clean up, port in TIME_WAIT
# Solution:
# 1. Use port 0 to get ephemeral port: "127.0.0.1:0"
# 2. Read the assigned port from socket
# 3. Add cleanup in test teardown
```

#### D. File System Issues
```bash
# Symptom: "Permission denied" or "No such file or directory"

# Root cause: Test creates files in /tmp that aren't cleaned up
# Solution:
# 1. Use tempfile crate: tempfile::TempDir::new()?
# 2. Or use: env::temp_dir() + random suffix
# 3. Ensure cleanup in drop() or test cleanup
```

**Debug Commands:**

```bash
# Run specific test with output
cargo test -p claudefs-fuse test_name -- --nocapture

# Run with single thread (easier to debug)
cargo test -- --test-threads=1

# Run in release mode (CI runs release)
cargo test --release

# Run with RUST_BACKTRACE for stack traces
RUST_BACKTRACE=1 cargo test
```

**Workflow Fix:**

1. Reproduce locally:
   ```bash
   # Match CI conditions
   cargo test --release
   # or
   cargo test --features all_features
   ```

2. If can't reproduce, add debug output:
   ```rust
   eprintln!("DEBUG: value={:?}", value);
   ```

3. Re-run in CI:
   ```bash
   git commit -am "[DEBUG] Add debug output"
   git push origin
   # Check Actions tab for output
   ```

---

### 4. Cache Issues

**Symptoms:**
- Build slower than expected (cache miss)
- Build suddenly breaks after cache change
- "Cache restored, but files missing"

**Solutions:**

#### A. Cache Key Mismatch
```bash
# Check cache key in workflow
# .github/workflows/ci-build.yml:
# key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}

# If Cargo.lock changed, cache key changes
# First run after lock change will be slow

# Solution: Expected behavior, cache rebuilds next run
```

#### B. Cache Corruption
```bash
# Solution: Clear cache manually
gh actions-cache delete --repo dirkpetersen/claudefs --all

# Or via GitHub UI:
# Settings > Actions > Caches > Delete All
```

#### C. Cache Size Exceeded
```bash
# GitHub Actions limit: 5GB per workflow
# If cache >5GB, newest entries evicted

# Solution:
# 1. Check size: du -sh ~/.cargo/registry
# 2. Prune old dependencies: cargo clean
# 3. Optimize: rm -rf ~/.cargo/registry/cache/*/.fingerprint/*
```

---

### 5. Artifact Issues

**Symptoms:**
- Workflow completes but no artifacts
- Artifacts don't download
- Artifact size is wrong

**Troubleshooting:**

```bash
# Check artifact upload in workflow
# Look for: uses: actions/upload-artifact@v3

# Verify artifact exists locally
ls -lh target/release/cfs

# Check workflow logs
# GitHub Actions > [workflow] > [job] > Search for "Upload artifact"

# Common issue: artifact path wrong
# Path is relative to workspace, not absolute
```

**Workflow Fix:**

```yaml
# Wrong:
- uses: actions/upload-artifact@v3
  with:
    name: cfs-binary
    path: /home/cfs/claudefs/target/release/cfs  # Absolute path fails!

# Correct:
- uses: actions/upload-artifact@v3
  with:
    name: cfs-binary
    path: target/release/cfs  # Relative to workspace
```

---

### 6. Timeout Issues

**Symptoms:**
- Workflow stuck, then times out
- "Workflow run exceeded execution time"

**Default Timeouts:**
- Per job: 360 minutes (6 hours)
- Per workflow: 35 days

**Usual Cause:** Deadlock or infinite loop in code

**Debug:**

```bash
# 1. Check if test hangs locally
timeout 30 cargo test test_name -- --nocapture

# 2. If it hangs, likely deadlock
# Check for:
# - Multiple mutexes acquired in different order
# - Arc<Mutex<Arc<Mutex<_>>>> nested locks
# - tokio::spawn without join handle

# 3. Add logging to find where it hangs
eprintln!("DEBUG: Before operation");
operation();
eprintln!("DEBUG: After operation");
```

**Workflow Fix:**

```yaml
# Add explicit timeout per job
jobs:
  build:
    name: Build
    runs-on: ubuntu-latest
    timeout-minutes: 30  # Kill if >30 min
    steps:
      ...
```

---

### 7. Permission & Authentication Failures

**Symptoms:**
- "fatal: could not read Username" during git operations
- "Error: Failed to authenticate" during AWS operations
- "Permission denied" during checkout

**Solutions:**

#### Git Authentication
```bash
# Issue: Token doesn't have right scopes
# Solution: Use token with repo + workflow scopes

# In GitHub: Settings > Developer Settings > Personal Access Tokens
# Required scopes:
# - repo (all)
# - workflow
# - read:org
```

#### AWS Credentials
```bash
# Issue: Credentials not found in CI
# Solution: Add to GitHub Secrets

# Settings > Secrets and variables > Actions > New secret
# Add:
# - AWS_ACCESS_KEY_ID
# - AWS_SECRET_ACCESS_KEY
# - AWS_REGION: us-west-2

# Then use in workflow:
env:
  AWS_ACCESS_KEY_ID: ${{ secrets.AWS_ACCESS_KEY_ID }}
  AWS_SECRET_ACCESS_KEY: ${{ secrets.AWS_SECRET_ACCESS_KEY }}
```

#### Checkout Failure
```bash
# Issue: "Could not resolve the repository"
# Solution: Check repository URL and permissions

git remote -v  # Verify remote

# If using HTTPS and token, ensure token is set
gh auth login

# If using SSH, ensure key is added to agent
ssh-add ~/.ssh/id_rsa
```

---

## Workflow-Specific Issues

### ci-build.yml Issues

**Problem:** "Rustfmt formatting failed"
- **Cause:** Code doesn't match rustfmt style
- **Solution:**
  ```bash
  cargo fmt
  git add .
  git commit -m "Format code"
  ```

**Problem:** "Clippy lint warnings"
- **Cause:** Clippy enabled with -D (deny) warnings
- **Solution:**
  ```bash
  cargo clippy --all-targets -- -D warnings
  cargo fix --allow-dirty
  git add .
  git commit -m "Fix clippy warnings"
  ```

**Problem:** "cargo audit found vulnerabilities"
- **Cause:** Dependency with known CVE
- **Solution:**
  ```bash
  cargo audit
  # Update vulnerable dependency to patched version
  cargo update vulnerable_crate
  ```

---

### tests-all.yml Issues

**Problem:** "test suite timed out"
- **Cause:** Tests taking too long
- **Solution:**
  ```bash
  cargo test --release  # Always release mode for CI
  # Or reduce test count with --lib (skip integration)
  ```

**Problem:** "Flaky test: passes 50% of time"
- **Cause:** Race condition or timing sensitivity
- **Solution:**
  ```bash
  # Run test 50 times
  for i in {1..50}; do cargo test test_name || break; done

  # If it fails, add synchronization:
  #  - Use channels instead of sleep()
  #  - Increase timeouts
  #  - Add barriers for synchronization
  ```

---

### integration-tests.yml Issues

**Problem:** "Integration test fails, but unit tests pass"
- **Cause:** Multi-crate interaction issue
- **Solution:**
  ```bash
  # Test specific integration
  cargo test -p claudefs-tests test_fuse_integration -- --nocapture

  # Check if other crates' changes broke it
  git log --oneline | head -10  # Check recent commits
  ```

---

### release.yml Issues

**Problem:** "Cross-compilation failed (ARM64)"
- **Cause:** ARM toolchain not installed
- **Solution:**
  ```bash
  # Add to workflow:
  - uses: actions-rs/toolchain@v1
    with:
      toolchain: stable
      target: aarch64-unknown-linux-gnu

  # Or use container:
  - name: Build ARM64
    run: |
      sudo apt-get install -y gcc-aarch64-linux-gnu
      cargo build --target aarch64-unknown-linux-gnu --release
  ```

---

### deploy-prod.yml Issues

**Problem:** "Terraform plan shows unexpected changes"
- **Cause:** State out of sync with infrastructure
- **Solution:**
  ```bash
  # Refresh state
  terraform refresh

  # Check what's different
  terraform plan -detailed-exitcode
  ```

**Problem:** "Deployment stuck on approval gate"
- **Cause:** Manual approval not given
- **Solution:**
  ```bash
  # Go to GitHub Actions > [workflow] > Pending Review
  # Click "Review deployments"
  # Select environment and click "Approve"
  ```

---

## Advanced Debugging

### Enable Debug Logging

```yaml
# In workflow file:
jobs:
  build:
    runs-on: ubuntu-latest
    env:
      RUST_LOG: debug
      RUST_BACKTRACE: full
    steps:
      - uses: actions/checkout@v3
      - run: cargo build 2>&1 | head -100
```

### Re-run with SSH

```bash
# For debugging, re-run workflow with SSH access
# GitHub Actions > [workflow run] > Re-run with SSH
# SSH into runner and inspect state

ssh runner@runner.github.actions
cd /home/runner/work/claudefs/claudefs
cargo build --verbose
```

### Create Minimal Reproduction

```bash
# If CI fails but local works, create minimal test case
cat > /tmp/minimal_test.rs << 'EOF'
#[test]
fn minimal() {
    // Isolated test that reproduces issue
}
EOF

rustc /tmp/minimal_test.rs --test -o /tmp/test
/tmp/test -- --nocapture
```

---

## Escalation Path

### Issue Not Resolved?

1. **Check GitHub Issues:** Search for similar issues
2. **Check CHANGELOG:** See if related work was done
3. **Review recent commits:** `git log --oneline -20`
4. **Ask other agents:** Create GitHub Issue tagged with relevant agent
   ```
   [BLOCKER] CI/CD issue — @A5 @A11

   ## Problem
   [description]

   ## Steps to Reproduce
   [steps]

   ## Expected
   [expected]

   ## Actual
   [actual]
   ```

---

**Document Status:** ✅ Complete
**Prepared by:** A11 (Infrastructure & CI)
**Last Updated:** 2026-03-01
