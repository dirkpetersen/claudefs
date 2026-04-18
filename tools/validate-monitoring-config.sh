#!/bin/bash
# validate-monitoring-config.sh — Validate all monitoring infrastructure configs
#
# Usage: ./validate-monitoring-config.sh
# Validates:
#   - Prometheus YAML syntax
#   - AlertManager YAML syntax
#   - Grafana dashboard JSON structure
#   - Cost aggregator script syntax

set -euo pipefail

TOOLS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ERRORS=0
WARNINGS=0

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Helper functions
print_error() {
    echo -e "${RED}✗ $1${NC}" >&2
    ((ERRORS++))
}

print_warning() {
    echo -e "${YELLOW}⚠ $1${NC}" >&2
    ((WARNINGS++))
}

print_ok() {
    echo -e "${GREEN}✓ $1${NC}"
}

# Check for required tools
check_required_tool() {
    if ! command -v "$1" &> /dev/null; then
        print_warning "Tool '$1' not found (some checks will be skipped)"
        return 1
    fi
    return 0
}

echo "=== Monitoring Infrastructure Validation ==="
echo ""

# 1. Validate Prometheus configuration
echo "1. Validating Prometheus configuration..."
if [ -f "$TOOLS_DIR/prometheus.yml" ]; then
    if check_required_tool "yamllint"; then
        if yamllint -d relaxed "$TOOLS_DIR/prometheus.yml" > /dev/null 2>&1; then
            print_ok "prometheus.yml YAML syntax valid"
        else
            print_error "prometheus.yml YAML syntax error"
            yamllint -d relaxed "$TOOLS_DIR/prometheus.yml" || true
        fi
    else
        # Fallback: at least verify it's valid YAML
        if python3 -c "import yaml; yaml.safe_load(open('$TOOLS_DIR/prometheus.yml'))" 2>/dev/null; then
            print_ok "prometheus.yml YAML structure valid (basic check)"
        else
            print_error "prometheus.yml YAML parsing failed"
        fi
    fi

    # Check key fields
    if grep -q "scrape_interval" "$TOOLS_DIR/prometheus.yml"; then
        print_ok "prometheus.yml contains scrape_interval"
    else
        print_warning "prometheus.yml missing scrape_interval"
    fi

    if grep -q "scrape_configs" "$TOOLS_DIR/prometheus.yml"; then
        print_ok "prometheus.yml contains scrape_configs"
    else
        print_error "prometheus.yml missing scrape_configs"
    fi
else
    print_error "prometheus.yml not found"
fi
echo ""

# 2. Validate AlertManager configuration
echo "2. Validating AlertManager configuration..."
if [ -f "$TOOLS_DIR/alertmanager.yml" ]; then
    if check_required_tool "yamllint"; then
        if yamllint -d relaxed "$TOOLS_DIR/alertmanager.yml" > /dev/null 2>&1; then
            print_ok "alertmanager.yml YAML syntax valid"
        else
            print_error "alertmanager.yml YAML syntax error"
            yamllint -d relaxed "$TOOLS_DIR/alertmanager.yml" || true
        fi
    else
        if python3 -c "import yaml; yaml.safe_load(open('$TOOLS_DIR/alertmanager.yml'))" 2>/dev/null; then
            print_ok "alertmanager.yml YAML structure valid (basic check)"
        else
            print_error "alertmanager.yml YAML parsing failed"
        fi
    fi

    # Check key fields
    if grep -q "route:" "$TOOLS_DIR/alertmanager.yml"; then
        print_ok "alertmanager.yml contains route configuration"
    else
        print_error "alertmanager.yml missing route configuration"
    fi

    if grep -q "receivers:" "$TOOLS_DIR/alertmanager.yml"; then
        print_ok "alertmanager.yml contains receivers"
    else
        print_error "alertmanager.yml missing receivers"
    fi
else
    print_error "alertmanager.yml not found"
fi
echo ""

# 3. Validate Prometheus alert rules
echo "3. Validating Prometheus alert rules..."
if [ -f "$TOOLS_DIR/prometheus-alerts.yml" ]; then
    if check_required_tool "yamllint"; then
        if yamllint -d relaxed "$TOOLS_DIR/prometheus-alerts.yml" > /dev/null 2>&1; then
            print_ok "prometheus-alerts.yml YAML syntax valid"
        else
            print_error "prometheus-alerts.yml YAML syntax error"
            yamllint -d relaxed "$TOOLS_DIR/prometheus-alerts.yml" || true
        fi
    else
        if python3 -c "import yaml; yaml.safe_load(open('$TOOLS_DIR/prometheus-alerts.yml'))" 2>/dev/null; then
            print_ok "prometheus-alerts.yml YAML structure valid (basic check)"
        else
            print_error "prometheus-alerts.yml YAML parsing failed"
        fi
    fi

    # Count alert rules
    ALERT_COUNT=$(grep -c "alert:" "$TOOLS_DIR/prometheus-alerts.yml" || echo "0")
    if [ "$ALERT_COUNT" -ge 8 ]; then
        print_ok "prometheus-alerts.yml contains $ALERT_COUNT alert rules"
    else
        print_warning "prometheus-alerts.yml contains only $ALERT_COUNT alert rules (expected >= 8)"
    fi
else
    print_error "prometheus-alerts.yml not found"
fi
echo ""

# 4. Validate Grafana dashboards
echo "4. Validating Grafana dashboards..."
DASHBOARD_COUNT=0
for dashboard in "$TOOLS_DIR"/grafana-dashboard-*.json; do
    if [ -f "$dashboard" ]; then
        DASHBOARD_COUNT=$((DASHBOARD_COUNT + 1))
        DASHBOARD_NAME=$(basename "$dashboard")

        if check_required_tool "jq"; then
            if jq . "$dashboard" > /dev/null 2>&1; then
                print_ok "$DASHBOARD_NAME is valid JSON"
            else
                print_error "$DASHBOARD_NAME has invalid JSON"
                jq . "$dashboard" 2>&1 | head -5 || true
            fi
        else
            if python3 -c "import json; json.load(open('$dashboard'))" 2>/dev/null; then
                print_ok "$DASHBOARD_NAME JSON structure valid (basic check)"
            else
                print_error "$DASHBOARD_NAME JSON parsing failed"
            fi
        fi
    fi
done

if [ "$DASHBOARD_COUNT" -eq 0 ]; then
    print_error "No Grafana dashboards found (expected: grafana-dashboard-*.json)"
elif [ "$DASHBOARD_COUNT" -lt 4 ]; then
    print_warning "Found $DASHBOARD_COUNT Grafana dashboards (expected: 4)"
else
    print_ok "Found $DASHBOARD_COUNT Grafana dashboards"
fi
echo ""

# 5. Validate cost aggregator script
echo "5. Validating cost aggregator script..."
if [ -f "$TOOLS_DIR/cfs-cost-aggregator.sh" ]; then
    if bash -n "$TOOLS_DIR/cfs-cost-aggregator.sh" 2>/dev/null; then
        print_ok "cfs-cost-aggregator.sh bash syntax valid"
    else
        print_error "cfs-cost-aggregator.sh has bash syntax errors"
        bash -n "$TOOLS_DIR/cfs-cost-aggregator.sh" || true
    fi

    if [ -x "$TOOLS_DIR/cfs-cost-aggregator.sh" ]; then
        print_ok "cfs-cost-aggregator.sh is executable"
    else
        print_warning "cfs-cost-aggregator.sh is not executable (chmod +x needed)"
    fi

    if head -1 "$TOOLS_DIR/cfs-cost-aggregator.sh" | grep -q "#!/bin/bash"; then
        print_ok "cfs-cost-aggregator.sh has correct shebang"
    else
        print_warning "cfs-cost-aggregator.sh missing bash shebang"
    fi
else
    print_error "cfs-cost-aggregator.sh not found"
fi
echo ""

# Summary
echo "=== Validation Summary ==="
echo "Errors:   $ERRORS"
echo "Warnings: $WARNINGS"

if [ "$ERRORS" -eq 0 ]; then
    echo -e "${GREEN}✓ All validations passed!${NC}"
    exit 0
else
    echo -e "${RED}✗ Validation failed with $ERRORS errors${NC}"
    exit 1
fi
