#!/bin/bash
# CI/CD Diagnostics Script
# Usage: ./tools/ci-diagnostics.sh [--full] [--cost] [--logs]
#
# Quickly diagnose CI/CD infrastructure health

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
REPO="dirkpetersen/claudefs"
AWS_REGION="${AWS_REGION:-us-west-2}"

# Functions
print_header() {
    echo -e "${BLUE}=== $1 ===${NC}"
}

print_ok() {
    echo -e "${GREEN}✓${NC} $1"
}

print_error() {
    echo -e "${RED}✗${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}⚠${NC} $1"
}

# 1. Check local repository status
check_repo_status() {
    print_header "Repository Status"

    # Check git status
    if [ "$(git rev-parse --abbrev-ref HEAD)" != "main" ]; then
        print_warning "Not on main branch: $(git rev-parse --abbrev-ref HEAD)"
    else
        print_ok "On main branch"
    fi

    # Check for uncommitted changes
    if [ -n "$(git status --porcelain | grep '^ M')" ]; then
        print_warning "Uncommitted changes detected:"
        git status --short | grep '^ M' | head -5
    else
        print_ok "All changes committed"
    fi

    # Check for unpushed commits
    UNPUSHED=$(git log --oneline origin/main..HEAD | wc -l)
    if [ "$UNPUSHED" -gt 0 ]; then
        print_warning "$UNPUSHED unpushed commits"
        git log --oneline origin/main..HEAD | head -3
    else
        print_ok "All commits pushed"
    fi

    # Check local commits
    LATEST=$(git log -1 --oneline)
    print_ok "Latest commit: $LATEST"
}

# 2. Check Rust build status
check_build() {
    print_header "Build Status"

    if ! cargo check --quiet 2>&1 | head -5; then
        print_error "cargo check failed"
        cargo check 2>&1 | head -20
        return 1
    fi
    print_ok "cargo check passed"

    # Check for clippy warnings
    if CLIPPY=$(cargo clippy --all-targets 2>&1 | grep "warning:" | wc -l); then
        if [ "$CLIPPY" -gt 0 ]; then
            print_warning "$CLIPPY clippy warnings found"
        else
            print_ok "No clippy warnings"
        fi
    fi
}

# 3. Check tests
check_tests() {
    print_header "Test Status"

    echo "Counting tests..."
    TEST_COUNT=$(cargo test --lib -- --list 2>&1 | grep "test \[" | wc -l)
    echo -e "${BLUE}  Tests: $TEST_COUNT${NC}"

    if [ -n "${RUN_TESTS:-}" ]; then
        echo "Running quick test (first crate only)..."
        cargo test -p claudefs-storage --lib -- --test-threads=2 2>&1 | tail -10
    fi
}

# 4. Check GitHub Actions workflows
check_workflows() {
    print_header "GitHub Actions Workflows"

    if [ ! -d ".github/workflows" ]; then
        print_error "No .github/workflows directory"
        return 1
    fi

    print_ok "Workflow directory exists"

    # Count workflow files
    COUNT=$(find .github/workflows -name "*.yml" | wc -l)
    echo -e "${BLUE}  Workflows: $COUNT${NC}"

    # Validate YAML syntax
    for f in .github/workflows/*.yml; do
        if python3 -c "import yaml; yaml.safe_load(open('$f'))" 2>/dev/null; then
            print_ok "$(basename $f)"
        else
            print_error "$(basename $f) has invalid YAML"
        fi
    done
}

# 5. Check AWS status
check_aws() {
    print_header "AWS Infrastructure"

    if ! command -v aws &> /dev/null; then
        print_warning "AWS CLI not installed"
        return 0
    fi

    # Check credentials
    if ! aws sts get-caller-identity --region "$AWS_REGION" &>/dev/null; then
        print_error "AWS credentials not configured or invalid"
        return 1
    fi
    print_ok "AWS credentials valid"

    # Check for secrets
    SECRETS=$(aws secretsmanager list-secrets --region "$AWS_REGION" --query 'SecretList[?starts_with(Name, `cfs/`)].Name' --output text 2>/dev/null)
    SECRET_COUNT=$(echo "$SECRETS" | wc -w)
    echo -e "${BLUE}  Secrets: $SECRET_COUNT${NC}"
    echo "$SECRETS" | tr ' ' '\n' | sed 's/^/    /'
}

# 6. Check cost
check_cost() {
    print_header "Cost Status"

    if ! command -v aws &> /dev/null; then
        print_warning "AWS CLI not installed, skipping cost check"
        return 0
    fi

    # Get today's cost
    TODAY=$(date +%Y-%m-%d)
    COST=$(aws ce get-cost-and-usage \
        --time-period Start="$TODAY",End="$TODAY" \
        --granularity DAILY \
        --metrics UnblendedCost \
        --query 'ResultsByTime[0].Total.UnblendedCost.Amount' \
        --output text \
        --region "$AWS_REGION" 2>/dev/null || echo "unknown")

    if [ "$COST" != "unknown" ]; then
        echo -e "${BLUE}  Today's spend: \$$COST${NC}"

        if (( $(echo "$COST > 100" | bc -l 2>/dev/null || echo 0) )); then
            print_error "Over daily budget (\$100)"
        elif (( $(echo "$COST > 80" | bc -l 2>/dev/null || echo 0) )); then
            print_warning "Approaching budget (\$100)"
        else
            print_ok "Within budget"
        fi
    else
        print_warning "Could not retrieve cost data"
    fi
}

# 7. Check supervisor status
check_supervision() {
    print_header "Autonomous Supervision"

    # Check watchdog
    if pgrep -f "cfs-watchdog" &>/dev/null; then
        print_ok "Watchdog process running"
        if [ -f "/var/log/cfs-agents/watchdog.log" ]; then
            TAIL=$(tail -1 /var/log/cfs-agents/watchdog.log)
            echo -e "${BLUE}  Latest: $TAIL${NC}"
        fi
    else
        print_warning "Watchdog process not running"
    fi

    # Check supervisor
    if [ -f "/var/log/cfs-agents/supervisor.log" ]; then
        print_ok "Supervisor log exists"
        TAIL=$(tail -1 /var/log/cfs-agents/supervisor.log)
        echo -e "${BLUE}  Latest: $TAIL${NC}"
    else
        print_warning "Supervisor log not found"
    fi
}

# 8. Quick performance check
check_performance() {
    print_header "Build Performance"

    echo "Checking build cache..."
    CACHE_SIZE=$(du -sh ~/.cargo/registry 2>/dev/null | cut -f1 || echo "unknown")
    echo -e "${BLUE}  Cargo registry: $CACHE_SIZE${NC}"

    # Time a clean check
    echo "Timing cargo check (warm cache)..."
    time cargo check --quiet 2>&1 | head -1 || true
}

# 9. Generate report
generate_report() {
    print_header "Summary Report"

    echo ""
    echo "Next steps to activate CI/CD:"
    echo ""
    echo "1. Verify GitHub token has 'workflow' scope:"
    echo "   https://github.com/settings/tokens"
    echo ""
    echo "2. Push workflows:"
    echo "   git push origin main"
    echo ""
    echo "3. Monitor first run:"
    echo "   https://github.com/$REPO/actions"
    echo ""
    echo "4. Track costs:"
    echo "   aws ce get-cost-and-usage ... (see docs/A11-COST-OPTIMIZATION.md)"
    echo ""
}

# Main
main() {
    case "${1:-}" in
        --cost)
            check_cost
            ;;
        --logs)
            check_supervision
            ;;
        --full)
            check_repo_status
            check_build
            check_tests
            check_workflows
            check_aws
            check_cost
            check_supervision
            check_performance
            generate_report
            ;;
        --help)
            cat << EOF
CI/CD Diagnostics Script

Usage: $0 [OPTION]

Options:
  --cost      Show cost status
  --logs      Show supervision logs
  --full      Run all diagnostics (default)
  --help      Show this help message

Environment variables:
  AWS_REGION  AWS region (default: us-west-2)
  RUN_TESTS   If set, run quick tests

Examples:
  $0 --full
  $0 --cost
  RUN_TESTS=1 $0

EOF
            ;;
        *)
            check_repo_status
            check_build
            check_workflows
            check_aws
            generate_report
            ;;
    esac
}

main "$@"
