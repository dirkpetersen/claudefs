# GitHub Actions CI/CD Troubleshooting Guide

**For:** ClaudeFS builders (A1-A8) and infrastructure team (A11)
**Updated:** 2026-04-17

---

## Quick Reference

| Problem | Symptom | First Step |
|---------|---------|-----------|
| Build timeout | CI job takes >25 min | Check cache hit rate in `Actions` tab |
| Clippy failures | "failed to compile; could not compile" | Run `cargo clippy --all-targets` locally |
| Test failures | "test result: FAILED" | Run `cargo test --lib` locally |
| Security scan blocked | "cargo deny check" fails | Run locally: `cargo deny check advisories` |
| OpenCode stuck | OpenCode step times out (>180s) | Check `/var/log/cfs-agents/opencode-fixes.log` |
| Memory OOM | Job killed by GitHub Actions | Reduce parallel jobs (split per-crate) |
| Cache corruption | Intermittent failures | Clear cache in Actions tab |

---

## When Your Commit Breaks CI

### Step 1: Reproduce Locally

**Fastest diagnosis path:**

```bash
cd /home/cfs/claudefs

# Try the exact sequence that CI runs
cargo fmt --all -- --check       # Check formatting
cargo clippy --all-targets -- -D warnings  # Lint check
cargo test --lib               # Test
```

**If any step fails locally:** That's your bug. Fix it and recommit.

**If all pass locally:** It's a CI environmental issue. Go to Step 2.

### Step 2: Check the CI Logs

1. Open: https://github.com/dirkpetersen/claudefs/actions
2. Click the failing workflow run
3. Click the failing job (usually `Build Workspace`, `Lint Check`, or `Unit Tests`)
4. Scroll to the failed step and **read the full output**

**Look for:**
- Rust compiler error (red `error:` lines)
- Timeout (exceeded 25 min)
- Out of memory (killed by GitHub)
- Network error (download failure)

### Step 3: Common Fixes

#### **Build Timeout (>25 min)**

**Cause:** Cache miss, slow compilation, or large workspace
**Solution:**

```bash
# Option 1: Clear build cache (one-time)
# In GitHub UI: Actions → All workflows → click ... → Clear all caches

# Option 2: Check cache hit rate
# In GitHub UI: Actions tab, find your workflow, check "Caches" section
# If <80% hit rate: may need to adjust cache key

# Option 3: Parallel test execution (faster)
# Edit .github/workflows/ci-build.yml:
# Change: cargo test --workspace
# To: parallel -j 4 ::: crates/*/  (run per-crate)
```

**Escalation:** If timeout persists after cache clear, file GitHub issue.

#### **Clippy Lint Failures**

**Cause:** New warnings in code
**Solution:**

```bash
# Reproduce locally
cargo clippy --all-targets -- -D warnings

# Fix warnings (usually auto-fixable)
cargo fix --allow-dirty

# Manual fixes for non-auto-fixable warnings
# (see specific clippy error for advice)

# Verify
cargo clippy --all-targets -- -D warnings

# Commit fix
git add -A
git commit -m "[A<N>] Fix clippy warnings"
```

#### **Test Failures**

**Cause:** Flaky test, environment difference, or real bug
**Solution:**

```bash
# Reproduce locally (multiple times to catch flakiness)
cargo test --lib -- --test-threads=1  # Single-threaded for consistent results

# If fails consistently: real bug in your code
# If fails intermittently: likely flaky test

# For flaky tests, check:
# - Timingensitive operations (use mock clocks)
# - Global state (ensure tests are isolated)
# - Async operations (proper await/block_on)
```

**Report flaky test:** File issue labeled `flaky-test`

#### **Cargo Audit (CVE) Failure**

**Cause:** Outdated dependency with known CVE
**Solution:**

```bash
# See which CVE blocked you
cargo audit

# Update the problematic dependency
cargo update <dependency-name>

# Verify fix
cargo audit

# Commit update
git add Cargo.lock
git commit -m "[A<N>] Update <dependency> for CVE fix"
```

#### **Cargo Deny License Failure**

**Cause:** New dependency with incompatible license
**Solution:**

```bash
# See which license is blocked
cargo deny check licenses

# Options:
# 1. Choose different dependency with compatible license
# 2. Add exception in deny.toml (requires A11 approval)

# Then remove the problematic dependency
cargo remove <package>
```

#### **OpenCode Execution Timeout (>180s)**

**Cause:** OpenCode server slow or overloaded
**Solution:**

```bash
# Check OpenCode fix log
tail -100 /var/log/cfs-agents/opencode-fixes.log

# Check retry count
cat /tmp/cfs-opencode-retries  # Will auto-retry (max 3 times)

# Manual trigger supervisor (runs every 15 min)
/opt/cfs-supervisor.sh

# If stuck >1 hour:
# - Contact A11 (infrastructure team)
# - Check AWS Bedrock API status
# - May need manual code fix (A11 will do)
```

---

## Running Tests Locally (Before Committing)

### Quick Test (5 min)

```bash
cargo test --lib --all-targets --workspace
```

**Expected:** All tests PASS, ~30-60 sec runtime

### Slow Test (40+ min)

```bash
cargo test --workspace  # Includes integration tests
```

**Expected:** All tests PASS, ~45 min runtime

### Test Specific Crate

```bash
cargo test -p claudefs-storage  # Just A1
cargo test -p claudefs-meta      # Just A2
# ... etc
```

### Test Specific Module

```bash
cargo test -p claudefs-storage io_depth_limiter  # Specific module
```

---

## Caching Strategy

### How CI Caching Works

GitHub Actions cache keys based on:
- Runner OS (`ubuntu-latest`)
- `Cargo.lock` hash (dependency versions)
- Optional: git SHA (for truly isolated builds)

**Cache path:** `~/.cargo/` and `target/` directories

### Cache Hit/Miss

**If cache hit >80%:**
- Build will be <2 min (deps already cached)
- Incremental compile ~10.5 sec
- **Total job:** ~10 min (fast)

**If cache miss (<80%):**
- Full dependency download ~3-5 min
- Clean build ~12-15 min
- **Total job:** ~20-25 min

### Force Cache Refresh

If cache is corrupted (intermittent failures):

```bash
# In GitHub UI:
Actions → All workflows → ... → Clear all caches

# Or via CLI:
gh actions-cache delete --all  # Requires admin token
```

---

## Debugging Cache Issues

**Check current cache:**

```bash
# List all cached items (GitHub UI only, no CLI currently)
# Actions tab → find your workflow → "Caches" section
```

**Reproduce cache-miss locally:**

```bash
rm -rf ~/.cargo target/

# Then run build
cargo build --release
```

**If build fails without cache but passes with cache:**
- Likely environment dependency (system library, binary tool)
- Report to A11 with: `cargo build --verbose 2>&1 | grep -E "(error|warning|linking)"

---

## Parallel Test Execution

ClaudeFS uses per-crate parallelization to speed up test suite:

```bash
# CI runs this (4 parallel jobs):
parallel -j 4 'cargo test -p {} --lib' ::: \
  claudefs-storage claudefs-meta claudefs-reduce claudefs-transport \
  claudefs-fuse claudefs-repl claudefs-gateway claudefs-mgmt

# Run same locally:
./tools/cfs-parallel-test.sh
```

**Expected:** 8-10 min total (vs 40+ min serial)

---

## Agent Session Restarting

If an agent session crashes or hangs:

```bash
# Supervisor auto-restarts agents every 15 min
# (you don't need to do anything)

# But to manually restart immediately:
/opt/cfs-agent-launcher.sh --agent A5  # Restart A5 (FUSE)

# Or restart all agents:
for i in {1..11}; do
  /opt/cfs-agent-launcher.sh --agent A$i
done
```

**Check if agent is running:**

```bash
tmux list-sessions | grep cfs-a5  # Look for cfs-a5 session
```

---

## Workflow Dependency Graph

GitHub Actions workflows can depend on each other:

```
ci-build.yml (build + clippy)
    ↓
tests-all.yml (run tests)
    ↓
security-scan.yml (only if tests pass)
    ↓
deploy-multinode.yml (multi-node suite)
```

**If earlier step fails:** Later steps won't run (by design).

**Skip failed step temporarily:**

```bash
# In workflow YAML: set 'continue-on-error: true'
# Only do this if you have a good reason!
```

---

## Emergency: Disable All Workflows

**Only use if CI is broken and blocking all commits:**

```bash
# Rename workflows to disable them
mv .github/workflows/ci-build.yml .github/workflows/ci-build.yml.disabled

# Then commit and push
git add .github/workflows/
git commit -m "[A11] Temporarily disable CI for emergency fix"
git push

# To re-enable (after fix):
mv .github/workflows/ci-build.yml.disabled .github/workflows/ci-build.yml
git add .github/workflows/
git commit -m "[A11] Re-enable CI"
git push
```

---

## Common Pitfalls

### Pitfall 1: Committing Uncommitted Files

**Problem:** `git status` shows dirty files, CI doesn't see them
**Solution:**

```bash
git add .
git commit -m "Uncommitted changes"
git push
```

### Pitfall 2: Pushing to Wrong Branch

**Problem:** Pushing to feature branch instead of `main`
**Solution:**

```bash
git checkout main
git rebase origin/main
git push origin main
```

### Pitfall 3: Cargo.lock Out of Sync

**Problem:** `Cargo.lock` and `Cargo.toml` don't match
**Solution:**

```bash
cargo update
git add Cargo.lock
git commit -m "Update Cargo.lock"
```

### Pitfall 4: Large Uncommitted Changes

**Problem:** Makefile/docs/config changes not staged
**Solution:**

```bash
git add .
git status  # Verify all changes are staged
git commit -m "..."
```

---

## Monitoring CI Health

### Subscribe to Notifications

```bash
# GitHub: Settings → Notifications → Automatic
# (You'll get email on CI failures)
```

### Check CI History

```bash
# View all workflow runs
gh run list --repo dirkpetersen/claudefs --limit 20

# View details of specific run
gh run view <run-id>
```

### CI Dashboard

```bash
# In GitHub web UI:
https://github.com/dirkpetersen/claudefs/actions
```

---

## Reporting CI Issues

**Found a CI bug?** Create GitHub issue:

```markdown
## Title: [CI] <brief description>

**Problem:** What went wrong?
**Reproduction:** Steps to reproduce locally
**Expected:** What should happen?
**Actual:** What happened instead?

**Environment:**
- OS: Ubuntu 22.04 / macOS 12 / ...
- Rust version: `rustc --version`
- Cargo version: `cargo --version`

**Logs:**
[Paste relevant output here, or attach file]
```

---

## For A11 (Infrastructure Team)

### Adding New Workflows

1. Create `workflow-name.yml` in `.github/workflows/`
2. Add documentation (this file)
3. Test locally with `act` (GitHub Actions emulator)
4. Commit with `[A11]` prefix
5. Update this troubleshooting guide

### Updating Cache Keys

If dependency updates cause cache misses:

```yaml
# Old (hashes by Cargo.lock):
key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}

# New (includes git SHA for isolation):
key: ${{ runner.os }}-cargo-${{ github.sha }}-${{ hashFiles('**/Cargo.lock') }}

# This ensures each commit gets fresh cache (slower first run, isolation)
```

### Escalation Paths

| Issue | Owner | Contact |
|-------|-------|---------|
| Timeout (>25 min) | A11 | Create GitHub issue |
| OpenCode stuck | A11 + Supervisor | Check logs, manual intervention if >2 hrs |
| Memory OOM | GitHub Actions limit | Switch to larger runner or split jobs |
| CVE in dependency | A1-A8 | Create security issue for builder to fix |
| Flaky test | A9 | File issue, runs integration tests to isolate |

---

## Success Criteria

Your CI should meet these targets:

- ✅ Build completes in **<15 min** (including cache operations)
- ✅ All tests pass on **first run** (no flakiness)
- ✅ Cache hit rate **>85%**
- ✅ No security warnings (**cargo deny** passes)
- ✅ Clippy warnings **zero** (by policy)
- ✅ OpenCode fixes >**90%** of errors automatically

---

## Questions?

- **For builders:** Post in #engineering Slack
- **For A11 issues:** Create GitHub issue with `[A11]` tag
- **For emergencies:** Contact Dirk (project lead)

---

**Last Updated:** 2026-04-17
**Maintained By:** A11 Infrastructure & CI
**Co-Authored-By:** Claude Haiku 4.5 <noreply@anthropic.com>
