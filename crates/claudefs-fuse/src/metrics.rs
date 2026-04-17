//! Prometheus-compatible metrics for the FUSE client.
//!
//! Provides metrics export in Prometheus text exposition format.

use crate::perf::FuseMetrics;

impl FuseMetrics {
    /// Render all FUSE metrics in Prometheus text exposition format.
    pub fn render_prometheus(&self) -> String {
        let snapshot = self.snapshot();
        let mut output = String::new();

        output.push_str("# HELP claudefs_fuse_lookups_total Total number of lookup operations\n");
        output.push_str("# TYPE claudefs_fuse_lookups_total counter\n");
        output.push_str(&format!(
            "claudefs_fuse_lookups_total {}\n",
            snapshot.lookups
        ));

        output.push_str("# HELP claudefs_fuse_reads_total Total number of read operations\n");
        output.push_str("# TYPE claudefs_fuse_reads_total counter\n");
        output.push_str(&format!("claudefs_fuse_reads_total {}\n", snapshot.reads));

        output.push_str("# HELP claudefs_fuse_writes_total Total number of write operations\n");
        output.push_str("# TYPE claudefs_fuse_writes_total counter\n");
        output.push_str(&format!("claudefs_fuse_writes_total {}\n", snapshot.writes));

        output.push_str("# HELP claudefs_fuse_creates_total Total number of create operations\n");
        output.push_str("# TYPE claudefs_fuse_creates_total counter\n");
        output.push_str(&format!(
            "claudefs_fuse_creates_total {}\n",
            snapshot.creates
        ));

        output.push_str("# HELP claudefs_fuse_unlinks_total Total number of unlink operations\n");
        output.push_str("# TYPE claudefs_fuse_unlinks_total counter\n");
        output.push_str(&format!(
            "claudefs_fuse_unlinks_total {}\n",
            snapshot.unlinks
        ));

        output.push_str("# HELP claudefs_fuse_mkdirs_total Total number of mkdir operations\n");
        output.push_str("# TYPE claudefs_fuse_mkdirs_total counter\n");
        output.push_str(&format!("claudefs_fuse_mkdirs_total {}\n", snapshot.mkdirs));

        output.push_str("# HELP claudefs_fuse_rmdirs_total Total number of rmdir operations\n");
        output.push_str("# TYPE claudefs_fuse_rmdirs_total counter\n");
        output.push_str(&format!("claudefs_fuse_rmdirs_total {}\n", snapshot.rmdirs));

        output.push_str("# HELP claudefs_fuse_renames_total Total number of rename operations\n");
        output.push_str("# TYPE claudefs_fuse_renames_total counter\n");
        output.push_str(&format!(
            "claudefs_fuse_renames_total {}\n",
            snapshot.renames
        ));

        output.push_str("# HELP claudefs_fuse_getattrs_total Total number of getattr operations\n");
        output.push_str("# TYPE claudefs_fuse_getattrs_total counter\n");
        output.push_str(&format!(
            "claudefs_fuse_getattrs_total {}\n",
            snapshot.getattrs
        ));

        output.push_str("# HELP claudefs_fuse_setattrs_total Total number of setattr operations\n");
        output.push_str("# TYPE claudefs_fuse_setattrs_total counter\n");
        output.push_str(&format!(
            "claudefs_fuse_setattrs_total {}\n",
            snapshot.setattrs
        ));

        output.push_str("# HELP claudefs_fuse_readdirs_total Total number of readdir operations\n");
        output.push_str("# TYPE claudefs_fuse_readdirs_total counter\n");
        output.push_str(&format!(
            "claudefs_fuse_readdirs_total {}\n",
            snapshot.readdirs
        ));

        output
            .push_str("# HELP claudefs_fuse_errors_total Total number of FUSE operation errors\n");
        output.push_str("# TYPE claudefs_fuse_errors_total counter\n");
        output.push_str(&format!("claudefs_fuse_errors_total {}\n", snapshot.errors));

        output.push_str("# HELP claudefs_fuse_bytes_read_total Total bytes read\n");
        output.push_str("# TYPE claudefs_fuse_bytes_read_total counter\n");
        output.push_str(&format!(
            "claudefs_fuse_bytes_read_total {}\n",
            snapshot.bytes_read
        ));

        output.push_str("# HELP claudefs_fuse_bytes_written_total Total bytes written\n");
        output.push_str("# TYPE claudefs_fuse_bytes_written_total counter\n");
        output.push_str(&format!(
            "claudefs_fuse_bytes_written_total {}\n",
            snapshot.bytes_written
        ));

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_prometheus_contains_metrics() {
        let metrics = FuseMetrics::new();
        metrics.inc_lookup();
        metrics.inc_read(4096);
        metrics.inc_write(2048);
        metrics.inc_create();

        let output = metrics.render_prometheus();

        assert!(output.contains("claudefs_fuse_lookups_total"));
        assert!(output.contains("claudefs_fuse_reads_total"));
        assert!(output.contains("claudefs_fuse_writes_total"));
        assert!(output.contains("claudefs_fuse_creates_total"));
        assert!(output.contains("# HELP"));
        assert!(output.contains("# TYPE"));
    }

    #[test]
    fn test_render_prometheus_includes_values() {
        let metrics = FuseMetrics::new();
        metrics.inc_lookup();
        metrics.inc_lookup();
        metrics.inc_read(100);

        let output = metrics.render_prometheus();

        assert!(output.contains("claudefs_fuse_lookups_total 2"));
        assert!(output.contains("claudefs_fuse_reads_total 1"));
        assert!(output.contains("claudefs_fuse_bytes_read_total 100"));
    }
}
