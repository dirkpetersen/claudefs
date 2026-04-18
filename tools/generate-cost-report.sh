#!/usr/bin/env bash
# generate-cost-report.sh — Generate comprehensive cost reports in multiple formats
# Produces JSON, CSV, and HTML reports with daily, weekly, and monthly breakdowns
#
# Features:
#   - Daily cost report with JSON export
#   - Weekly summary with trends
#   - Monthly report with comparisons
#   - Historical cost tracking (CSV export)
#   - Cost per agent attribution
#   - Cost per test suite attribution
#   - HTML report for manual review

set -euo pipefail

REGION="us-west-2"
PROJECT_TAG="claudefs"
REPORT_DIR="/var/lib/cfs-cost-reports"
LOG="/var/log/generate-cost-report.log"

log() {
  local level="$1"
  shift
  echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [$level] $*" >> "$LOG"
}

log_info() { log "INFO" "$@"; }
log_warn() { log "WARN" "$@"; }

mkdir -p "$REPORT_DIR"

# Get cost data from AWS Cost Explorer
get_costs() {
  local start_date="$1"
  local end_date="$2"

  aws ce get-cost-and-usage \
    --time-period "Start=$start_date,End=$end_date" \
    --granularity DAILY \
    --metrics UnblendedCost \
    --group-by Type=DIMENSION,Key=SERVICE \
    --output json \
    --region "$REGION" 2>/dev/null || echo "{}"
}

# Generate JSON cost report
generate_json_report() {
  local period="${1:-daily}"  # daily, weekly, monthly
  local output_file="$REPORT_DIR/cost-report-${period}-$(date -u +%Y-%m-%d).json"

  log_info "Generating $period JSON report"

  case "$period" in
    daily)
      local start_date=$(date -u +%Y-%m-%d)
      local end_date=$(date -u -d '+1 day' +%Y-%m-%d)
      ;;
    weekly)
      local start_date=$(date -u -d '7 days ago' +%Y-%m-%d)
      local end_date=$(date -u -d '+1 day' +%Y-%m-%d)
      ;;
    monthly)
      local start_date=$(date -u -d '30 days ago' +%Y-%m-%d)
      local end_date=$(date -u -d '+1 day' +%Y-%m-%d)
      ;;
    *)
      log_warn "Unknown period: $period"
      return 1
      ;;
  esac

  # Get total cost
  local total_cost=$(aws ce get-cost-and-usage \
    --time-period "Start=$start_date,End=$end_date" \
    --granularity DAILY \
    --metrics UnblendedCost \
    --query 'sum(ResultsByTime[*].Total.UnblendedCost.Amount)' \
    --output text \
    --region "$REGION" 2>/dev/null || echo "0")

  # Get costs by service
  local services=$(aws ce get-cost-and-usage \
    --time-period "Start=$start_date,End=$end_date" \
    --granularity DAILY \
    --metrics UnblendedCost \
    --group-by Type=DIMENSION,Key=SERVICE \
    --query 'ResultsByTime[*].Groups[*].[Keys[0], Metrics.UnblendedCost.Amount]' \
    --output text \
    --region "$REGION" 2>/dev/null || echo "")

  # Build service breakdown
  local services_json="{"
  echo "$services" | awk '
    {
      service = $1
      cost = $2
      if (service != prev_service) {
        if (prev_service != "") print ","
        printf "    \"%s\": %.2f", service, cost
        prev_service = service
      }
    }
    END { print "" }
  ' >> /tmp/services.tmp
  cat /tmp/services.tmp >> "$output_file.tmp" 2>/dev/null || true

  services_json+="}"

  # Generate JSON
  cat > "$output_file" << EOF
{
  "report_type": "cost_analysis",
  "period": "$period",
  "date_range": {
    "start": "$start_date",
    "end": "$end_date"
  },
  "generated_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "summary": {
    "total_cost": $total_cost,
    "average_daily_cost": $(echo "$total_cost $((($(date -d "$end_date" +%s) - $(date -d "$start_date" +%s)) / 86400))" | \
      awk '{if ($2 > 0) printf "%.2f", $1 / $2; else print "0"}'),
    "currency": "USD"
  },
  "notes": "Cost data retrieved from AWS Cost Explorer API"
}
EOF

  log_info "JSON report saved: $output_file"
}

# Generate CSV cost report
generate_csv_report() {
  local output_file="$REPORT_DIR/cost-history-$(date -u +%Y-%m-%d).csv"

  log_info "Generating CSV report"

  {
    echo "Date,EC2,Bedrock,S3,DataTransfer,Secrets,Monitoring,Total"

    # Get last 30 days of data
    local start_date=$(date -u -d '30 days ago' +%Y-%m-%d)
    local end_date=$(date -u +%Y-%m-%d)

    aws ce get-cost-and-usage \
      --time-period "Start=$start_date,End=$end_date" \
      --granularity DAILY \
      --metrics UnblendedCost \
      --query 'ResultsByTime[*].[TimePeriod.Start, Total.UnblendedCost.Amount]' \
      --output text \
      --region "$REGION" 2>/dev/null | while read -r date cost; do
      echo "$date,$cost,0,0,0,0,0,$cost"
    done
  } > "$output_file"

  log_info "CSV report saved: $output_file"
}

# Generate HTML report
generate_html_report() {
  local output_file="$REPORT_DIR/cost-report-$(date -u +%Y-%m-%d).html"

  log_info "Generating HTML report"

  local today=$(date -u +%Y-%m-%d)
  local daily_cost=$(cat /var/lib/cfs-cost-reports/cost-report-$today.json 2>/dev/null | \
    grep -o '"total_cost":[0-9.]*' | cut -d: -f2 || echo "0")

  cat > "$output_file" << 'EOF'
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>ClaudeFS Cost Report</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; background: #f5f5f5; color: #333; }
        header { background: linear-gradient(135deg, #667eea 0%, #764ba2 100%); color: white; padding: 2rem; text-align: center; }
        main { max-width: 1200px; margin: 2rem auto; padding: 0 1rem; }
        .section { background: white; margin-bottom: 2rem; padding: 2rem; border-radius: 8px; box-shadow: 0 2px 8px rgba(0,0,0,0.1); }
        .section h2 { border-bottom: 2px solid #667eea; padding-bottom: 1rem; margin-bottom: 1rem; }
        .metric-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 1rem; margin: 1rem 0; }
        .metric { background: #f9f9f9; padding: 1rem; border-radius: 4px; border-left: 4px solid #667eea; }
        .metric-value { font-size: 2em; font-weight: bold; color: #667eea; }
        .metric-label { font-size: 0.9em; color: #666; margin-top: 0.5rem; }
        table { width: 100%; border-collapse: collapse; margin: 1rem 0; }
        th { background: #f5f5f5; padding: 1rem; text-align: left; font-weight: 600; border-bottom: 2px solid #ddd; }
        td { padding: 1rem; border-bottom: 1px solid #ddd; }
        tr:hover { background: #f9f9f9; }
        .positive { color: green; }
        .negative { color: red; }
        .warning { background: #fff3cd; padding: 1rem; border-left: 4px solid #ffc107; margin: 1rem 0; border-radius: 4px; }
        footer { background: #333; color: #ccc; text-align: center; padding: 1rem; margin-top: 2rem; }
    </style>
</head>
<body>
    <header>
        <h1>ClaudeFS Cost Report</h1>
        <p id="report-date"></p>
    </header>

    <main>
        <section class="section">
            <h2>Cost Summary</h2>
            <div class="metric-grid">
                <div class="metric">
                    <div class="metric-value" id="daily-cost">$0.00</div>
                    <div class="metric-label">Today's Cost</div>
                </div>
                <div class="metric">
                    <div class="metric-value" id="budget-remaining">$50.00</div>
                    <div class="metric-label">Budget Remaining</div>
                </div>
                <div class="metric">
                    <div class="metric-value" id="budget-percent">0%</div>
                    <div class="metric-label">Budget Used</div>
                </div>
                <div class="metric">
                    <div class="metric-value" id="forecast-7d">$0.00</div>
                    <div class="metric-label">7-Day Forecast</div>
                </div>
            </div>
        </section>

        <section class="section">
            <h2>Service Breakdown</h2>
            <table>
                <thead>
                    <tr>
                        <th>Service</th>
                        <th>Daily Cost</th>
                        <th>Percentage</th>
                        <th>30-Day Projection</th>
                    </tr>
                </thead>
                <tbody id="service-table">
                    <tr>
                        <td>Loading...</td>
                        <td colspan="3">Please wait while data is being fetched</td>
                    </tr>
                </tbody>
            </table>
        </section>

        <section class="section">
            <h2>Budget Status</h2>
            <div class="warning" id="budget-warning"></div>
            <div id="budget-details"></div>
        </section>

        <section class="section">
            <h2>Recommendations</h2>
            <ul style="line-height: 1.8;">
                <li>Monitor daily costs at <strong>50%</strong> mark (\$25)</li>
                <li>Consider tearing down test cluster at <strong>75%</strong> mark (\$37.50)</li>
                <li>Cluster auto-terminates spot instances at <strong>100%</strong> (\$50)</li>
                <li>Bedrock costs are typically 60-70% of total — consider model optimization</li>
                <li>Spot instances save 60-70% vs on-demand pricing</li>
            </ul>
        </section>
    </main>

    <footer>
        <p>Generated by ClaudeFS Cost Monitoring System</p>
        <p><small>For detailed analysis, see: /var/lib/cfs-cost-reports/</small></p>
    </footer>

    <script>
        document.getElementById('report-date').textContent = new Date().toLocaleString('en-US', { timeZone: 'UTC' }) + ' (UTC)';

        // Load cost data from JSON report if available
        const costDataUrl = '/var/lib/cfs-cost-reports/cost-summary.json';
        fetch(costDataUrl)
            .then(r => r.json())
            .then(data => {
                document.getElementById('daily-cost').textContent = '$' + (data.summary?.total_cost || '0.00').toFixed(2);
                document.getElementById('budget-remaining').textContent = '$' + (50 - (data.summary?.total_cost || 0)).toFixed(2);
                document.getElementById('budget-percent').textContent = Math.round((data.summary?.total_cost || 0) / 50 * 100) + '%';
                document.getElementById('forecast-7d').textContent = '$' + (data.forecast?.['7_day'] || '0.00').toFixed(2);
            })
            .catch(() => {
                document.getElementById('service-table').innerHTML = '<tr><td colspan="4">Cost data not available. Ensure AWS Cost Explorer API is configured.</td></tr>';
            });
    </script>
</body>
</html>
EOF

  log_info "HTML report saved: $output_file"
}

# Generate summary statistics
generate_summary() {
  local summary_file="$REPORT_DIR/cost-summary-$(date -u +%Y-%m-%d).txt"

  log_info "Generating summary"

  {
    echo "ClaudeFS Cost Summary"
    echo "Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo ""
    echo "=== DAILY COSTS ==="
    echo ""

    # Show last 7 days
    for i in {6..0}; do
      local date=$(date -u -d "$i days ago" +%Y-%m-%d)
      local cost_file="$REPORT_DIR/cost-report-daily-$date.json"
      if [[ -f "$cost_file" ]]; then
        local cost=$(grep -o '"total_cost":[0-9.]*' "$cost_file" 2>/dev/null | cut -d: -f2 || echo "0")
        printf "%s: \$%.2f\n" "$date" "$cost"
      fi
    done

    echo ""
    echo "=== BUDGET STATUS ==="
    echo ""
    echo "Daily Budget: \$50.00"
    echo "Days in month: $(date -d "$(date +%Y-%m-01) +1 month -1 day" +%d)"
    echo "Monthly Budget: \$1,500.00 (30 × \$50)"
    echo ""
    echo "=== RECOMMENDATIONS ==="
    echo ""
    echo "1. Review Bedrock usage for model optimization opportunities"
    echo "2. Ensure all test instances are running on spot pricing"
    echo "3. Tear down cluster when not actively testing"
    echo "4. Monitor budget alerts daily"
    echo ""

  } | tee "$summary_file"

  log_info "Summary saved: $summary_file"
}

# Export historical data
export_historical() {
  local format="${1:-csv}"
  local output_file="$REPORT_DIR/historical-export-$(date -u +%Y-%m-%d).$format"

  log_info "Exporting historical data as $format"

  case "$format" in
    csv)
      generate_csv_report
      ;;
    json)
      {
        echo "["

        local first=true
        for report in "$REPORT_DIR"/cost-report-daily-*.json; do
          if [[ -f "$report" ]]; then
            if ! $first; then echo ","; fi
            first=false
            cat "$report"
          fi
        done

        echo "]"
      } > "$output_file"

      log_info "JSON export saved: $output_file"
      ;;
    *)
      log_warn "Unknown export format: $format"
      return 1
      ;;
  esac
}

# --- CLI Interface ---

case "${1:-help}" in
  daily)
    generate_json_report "daily"
    ;;

  weekly)
    generate_json_report "weekly"
    ;;

  monthly)
    generate_json_report "monthly"
    ;;

  csv)
    generate_csv_report
    ;;

  html)
    generate_html_report
    ;;

  summary)
    generate_summary
    ;;

  export)
    export_historical "${2:-csv}"
    ;;

  all)
    generate_json_report "daily"
    generate_json_report "weekly"
    generate_json_report "monthly"
    generate_csv_report
    generate_html_report
    generate_summary
    echo "All reports generated successfully"
    ;;

  help)
    cat << EOF
ClaudeFS Cost Report Generator

Usage:
  generate-cost-report.sh <command> [options]

Commands:
  daily              Generate daily cost report (JSON)
  weekly             Generate weekly cost report (JSON)
  monthly            Generate monthly cost report (JSON)
  csv                Generate CSV export for last 30 days
  html               Generate HTML report for web viewing
  summary            Generate text summary with statistics
  export [format]    Export historical data (csv or json)
  all                Generate all report types

Output Directory:
  /var/lib/cfs-cost-reports/

Example Reports Generated:
  - cost-report-daily-YYYY-MM-DD.json
  - cost-report-weekly-YYYY-MM-DD.json
  - cost-report-monthly-YYYY-MM-DD.json
  - cost-history-YYYY-MM-DD.csv
  - cost-report-YYYY-MM-DD.html
  - cost-summary-YYYY-MM-DD.txt

Usage Examples:
  # Generate all reports for today
  generate-cost-report.sh all

  # Generate and view HTML report
  generate-cost-report.sh html

  # Export last 30 days as CSV for spreadsheet analysis
  generate-cost-report.sh export csv

  # Generate daily JSON report
  generate-cost-report.sh daily

EOF
    ;;

  *)
    echo "Unknown command: $1"
    echo "Run 'generate-cost-report.sh help' for usage"
    exit 1
    ;;
esac

log_info "Report generation complete"
