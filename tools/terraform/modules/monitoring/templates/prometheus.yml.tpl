global:
  scrape_interval: ${scrape_interval}

scrape_configs:
  - job_name: 'claudefs-storage'
    static_configs:
      - targets: [${storage_endpoints}]
        labels:
          role: 'storage'
    metrics_path: '/metrics'

  - job_name: 'claudefs-orchestrator'
    static_configs:
      - targets: ['localhost:9400']
        labels:
          role: 'orchestrator'

  - job_name: 'claudefs-clients'
    static_configs:
      - targets: ['localhost:9800']
        labels:
          role: 'client'