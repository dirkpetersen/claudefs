use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct MatrixDimension {
    pub name: String,
    pub values: Vec<String>,
}

impl MatrixDimension {
    pub fn new(name: &str, values: Vec<&str>) -> Self {
        MatrixDimension {
            name: name.to_string(),
            values: values.iter().map(|s| s.to_string()).collect(),
        }
    }

    pub fn count(&self) -> usize {
        self.values.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixPoint {
    pub dimensions: HashMap<String, String>,
}

impl MatrixPoint {
    pub fn new(dimensions: HashMap<String, String>) -> Self {
        MatrixPoint { dimensions }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.dimensions.get(key).map(|s| s.as_str())
    }

    pub fn label(&self) -> String {
        let mut pairs: Vec<String> = self
            .dimensions
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        pairs.sort();
        pairs.join(",")
    }
}

pub struct TestMatrix {
    pub dimensions: Vec<MatrixDimension>,
    pub excludes: Vec<HashMap<String, String>>,
}

impl TestMatrix {
    pub fn new() -> Self {
        TestMatrix {
            dimensions: Vec::new(),
            excludes: Vec::new(),
        }
    }

    pub fn add_dimension(&mut self, dim: MatrixDimension) -> &mut Self {
        self.dimensions.push(dim);
        self
    }

    pub fn exclude(&mut self, combo: HashMap<String, String>) -> &mut Self {
        self.excludes.push(combo);
        self
    }

    pub fn expand(&self) -> Vec<MatrixPoint> {
        if self.dimensions.is_empty() {
            return vec![MatrixPoint::new(HashMap::new())];
        }

        fn cartesian_product(
            dimensions: &[MatrixDimension],
            index: usize,
        ) -> Vec<HashMap<String, String>> {
            if index >= dimensions.len() {
                return vec![HashMap::new()];
            }

            let mut result = Vec::new();
            let dim = &dimensions[index];
            let rest = cartesian_product(dimensions, index + 1);

            for value in &dim.values {
                for mut inner in rest.iter().clone() {
                    let mut new_map = inner.clone();
                    new_map.insert(dim.name.clone(), value.clone());
                    result.push(new_map);
                }
            }

            result
        }

        let mut points: Vec<MatrixPoint> = cartesian_product(&self.dimensions, 0)
            .into_iter()
            .map(MatrixPoint::new)
            .collect();

        points.retain(|point| {
            !self.excludes.iter().any(|ex| {
                ex.iter()
                    .all(|(k, v)| point.get(k).map(|pv| pv == v).unwrap_or(false))
            })
        });

        points
    }

    pub fn count(&self) -> usize {
        self.expand().len()
    }
}

impl Default for TestMatrix {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct CiJob {
    pub name: String,
    pub matrix_point: MatrixPoint,
    pub steps: Vec<CiStep>,
    pub timeout_minutes: u32,
}

#[derive(Debug, Clone)]
pub struct CiStep {
    pub name: String,
    pub command: String,
    pub env: HashMap<String, String>,
}

impl CiJob {
    pub fn new(name: &str, point: MatrixPoint) -> Self {
        CiJob {
            name: name.to_string(),
            matrix_point: point,
            steps: Vec::new(),
            timeout_minutes: 30,
        }
    }

    pub fn add_step(&mut self, step: CiStep) -> &mut Self {
        self.steps.push(step);
        self
    }

    pub fn to_github_actions_yaml(&self) -> String {
        let mut yaml = format!("  {}:\n", self.name);
        yaml.push_str(&format!("    timeout-minutes: {}\n", self.timeout_minutes));
        yaml.push_str("    steps:\n");
        for step in &self.steps {
            yaml.push_str(&format!("      - name: {}\n", step.name));
            yaml.push_str(&format!("        run: {}\n", step.command));
            if !step.env.is_empty() {
                yaml.push_str("        env:\n");
                for (k, v) in &step.env {
                    yaml.push_str(&format!("          {}: {}\n", k, v));
                }
            }
        }
        yaml
    }
}

pub fn default_claudefs_matrix() -> TestMatrix {
    let mut matrix = TestMatrix::new();
    matrix.add_dimension(MatrixDimension::new(
        "os",
        vec!["ubuntu-24.04", "ubuntu-26.04"],
    ));
    matrix.add_dimension(MatrixDimension::new(
        "compression",
        vec!["none", "lz4", "zstd"],
    ));
    matrix.add_dimension(MatrixDimension::new("erasure", vec!["none", "4+2"]));
    matrix
}

pub fn generate_ci_jobs(matrix: &TestMatrix, test_suite: &str) -> Vec<CiJob> {
    let points = matrix.expand();
    points
        .into_iter()
        .enumerate()
        .map(|(i, point)| {
            let name = format!("{}-{}-{}", test_suite, i + 1, point.label());
            let mut job = CiJob::new(&name, point);
            job.add_step(CiStep {
                name: "Run tests".to_string(),
                command: format!("cargo test --{}", test_suite),
                env: HashMap::new(),
            });
            job
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matrix_dimension_creation() {
        let dim = MatrixDimension::new("os", vec!["ubuntu", "rhel"]);
        assert_eq!(dim.name, "os");
        assert_eq!(dim.values, vec!["ubuntu", "rhel"]);
    }

    #[test]
    fn test_matrix_dimension_count() {
        let dim = MatrixDimension::new("test", vec!["a", "b", "c"]);
        assert_eq!(dim.count(), 3);
    }

    #[test]
    fn test_matrix_point_get() {
        let mut dims = HashMap::new();
        dims.insert("os".to_string(), "ubuntu".to_string());
        let point = MatrixPoint::new(dims);
        assert_eq!(point.get("os"), Some("ubuntu"));
        assert_eq!(point.get("missing"), None);
    }

    #[test]
    fn test_matrix_point_label() {
        let mut dims = HashMap::new();
        dims.insert("os".to_string(), "ubuntu".to_string());
        dims.insert("compression".to_string(), "lz4".to_string());
        let point = MatrixPoint::new(dims);
        let label = point.label();
        assert!(label.contains("compression=lz4"));
        assert!(label.contains("os=ubuntu"));
    }

    #[test]
    fn test_test_matrix_add_dimension() {
        let mut matrix = TestMatrix::new();
        matrix.add_dimension(MatrixDimension::new("os", vec!["ubuntu"]));
        assert_eq!(matrix.dimensions.len(), 1);
    }

    #[test]
    fn test_test_matrix_expand_2x2() {
        let mut matrix = TestMatrix::new();
        matrix.add_dimension(MatrixDimension::new("os", vec!["ubuntu", "rhel"]));
        matrix.add_dimension(MatrixDimension::new("arch", vec!["x86_64", "arm64"]));
        let points = matrix.expand();
        assert_eq!(points.len(), 4);
    }

    #[test]
    fn test_test_matrix_expand_2x2x2() {
        let mut matrix = TestMatrix::new();
        matrix.add_dimension(MatrixDimension::new("a", vec!["1", "2"]));
        matrix.add_dimension(MatrixDimension::new("b", vec!["1", "2", "3"]));
        matrix.add_dimension(MatrixDimension::new("c", vec!["x", "y"]));
        let points = matrix.expand();
        assert_eq!(points.len(), 12); // 2 * 3 * 2 = 12
    }

    #[test]
    fn test_test_matrix_expand_empty() {
        let matrix = TestMatrix::new();
        let points = matrix.expand();
        assert_eq!(points.len(), 1);
    }

    #[test]
    fn test_test_matrix_exclude() {
        let mut matrix = TestMatrix::new();
        matrix.add_dimension(MatrixDimension::new("os", vec!["ubuntu", "rhel"]));
        matrix.add_dimension(MatrixDimension::new("feature", vec!["a", "b"]));

        let mut exclude = HashMap::new();
        exclude.insert("os".to_string(), "ubuntu".to_string());
        exclude.insert("feature".to_string(), "a".to_string());
        matrix.exclude(exclude);

        let points = matrix.expand();
        assert_eq!(points.len(), 3); // 2*2 - 1 = 3
    }

    #[test]
    fn test_test_matrix_count() {
        let mut matrix = TestMatrix::new();
        matrix.add_dimension(MatrixDimension::new("os", vec!["ubuntu", "rhel"]));
        matrix.add_dimension(MatrixDimension::new("arch", vec!["x86_64"]));
        assert_eq!(matrix.count(), 2);
    }

    #[test]
    fn test_test_matrix_count_matches_expand() {
        let mut matrix = TestMatrix::new();
        matrix.add_dimension(MatrixDimension::new("a", vec!["1", "2", "3"]));
        matrix.add_dimension(MatrixDimension::new("b", vec!["x", "y"]));
        assert_eq!(matrix.count(), matrix.expand().len());
    }

    #[test]
    fn test_ci_job_creation() {
        let point = MatrixPoint::new(HashMap::new());
        let job = CiJob::new("test-job", point);
        assert_eq!(job.name, "test-job");
        assert_eq!(job.timeout_minutes, 30);
    }

    #[test]
    fn test_ci_job_add_step() {
        let point = MatrixPoint::new(HashMap::new());
        let mut job = CiJob::new("test", point);
        job.add_step(CiStep {
            name: "Build".to_string(),
            command: "cargo build".to_string(),
            env: HashMap::new(),
        });
        assert_eq!(job.steps.len(), 1);
    }

    #[test]
    fn test_ci_job_to_github_actions_yaml() {
        let mut dims = HashMap::new();
        dims.insert("os".to_string(), "ubuntu".to_string());
        let point = MatrixPoint::new(dims);
        let mut job = CiJob::new("test-1-os=ubuntu", point);
        job.add_step(CiStep {
            name: "Build".to_string(),
            command: "cargo build".to_string(),
            env: HashMap::new(),
        });

        let yaml = job.to_github_actions_yaml();
        assert!(yaml.contains("test-1-os=ubuntu"));
        assert!(yaml.contains("timeout-minutes:"));
        assert!(yaml.contains("Build"));
    }

    #[test]
    fn test_ci_step_with_env() {
        let step = CiStep {
            name: "Test".to_string(),
            command: "cargo test".to_string(),
            env: {
                let mut env = HashMap::new();
                env.insert("RUST_BACKTRACE".to_string(), "1".to_string());
                env
            },
        };
        assert_eq!(step.env.get("RUST_BACKTRACE"), Some(&"1".to_string()));
    }

    #[test]
    fn test_default_claudefs_matrix_dimensions() {
        let matrix = default_claudefs_matrix();
        assert!(!matrix.dimensions.is_empty());

        let dim_names: Vec<&str> = matrix.dimensions.iter().map(|d| d.name.as_str()).collect();
        assert!(dim_names.contains(&"os"));
        assert!(dim_names.contains(&"compression"));
        assert!(dim_names.contains(&"erasure"));
    }

    #[test]
    fn test_default_claudefs_matrix_values() {
        let matrix = default_claudefs_matrix();
        let os_dim = matrix.dimensions.iter().find(|d| d.name == "os").unwrap();
        assert!(os_dim.values.contains(&"ubuntu-24.04".to_string()));
        assert!(os_dim.values.contains(&"ubuntu-26.04".to_string()));
    }

    #[test]
    fn test_generate_ci_jobs_count() {
        let matrix = default_claudefs_matrix();
        let jobs = generate_ci_jobs(&matrix, "unit");
        assert_eq!(jobs.len(), matrix.expand().len());
    }

    #[test]
    fn test_generate_ci_jobs_have_names() {
        let mut matrix = TestMatrix::new();
        matrix.add_dimension(MatrixDimension::new("os", vec!["linux"]));

        let jobs = generate_ci_jobs(&matrix, "unit");
        for job in &jobs {
            assert!(job.name.starts_with("unit-"));
        }
    }

    #[test]
    fn test_generate_ci_jobs_have_steps() {
        let mut matrix = TestMatrix::new();
        matrix.add_dimension(MatrixDimension::new("test", vec!["a"]));

        let jobs = generate_ci_jobs(&matrix, "unit");
        for job in &jobs {
            assert!(!job.steps.is_empty());
        }
    }

    #[test]
    fn test_exclude_reduces_count() {
        let mut matrix = TestMatrix::new();
        matrix.add_dimension(MatrixDimension::new("a", vec!["1", "2", "3"]));
        matrix.add_dimension(MatrixDimension::new("b", vec!["x", "y"]));

        let count_before = matrix.count();

        let mut exclude = HashMap::new();
        exclude.insert("a".to_string(), "1".to_string());
        exclude.insert("b".to_string(), "y".to_string());
        matrix.exclude(exclude);

        let count_after = matrix.count();
        assert_eq!(count_before - count_after, 1);
    }

    #[test]
    fn test_multiple_excludes() {
        let mut matrix = TestMatrix::new();
        matrix.add_dimension(MatrixDimension::new("a", vec!["1", "2"]));
        matrix.add_dimension(MatrixDimension::new("b", vec!["x", "y"]));

        let mut exclude1 = HashMap::new();
        exclude1.insert("a".to_string(), "1".to_string());
        exclude1.insert("b".to_string(), "x".to_string());
        matrix.exclude(exclude1);

        let mut exclude2 = HashMap::new();
        exclude2.insert("a".to_string(), "2".to_string());
        exclude2.insert("b".to_string(), "y".to_string());
        matrix.exclude(exclude2);

        assert_eq!(matrix.count(), 2); // 4 - 2 = 2
    }

    #[test]
    fn test_exclude_partial_match() {
        let mut matrix = TestMatrix::new();
        matrix.add_dimension(MatrixDimension::new("a", vec!["1", "2"]));
        matrix.add_dimension(MatrixDimension::new("b", vec!["x", "y"]));

        // Only exclude a=1 (doesn't matter what b is)
        let mut exclude = HashMap::new();
        exclude.insert("a".to_string(), "1".to_string());
        matrix.exclude(exclude);

        assert_eq!(matrix.count(), 2); // 2 * 2 - 2 = 2
    }

    #[test]
    fn test_matrix_point_eq() {
        let mut d1 = HashMap::new();
        d1.insert("k".to_string(), "v".to_string());
        let p1 = MatrixPoint::new(d1);

        let mut d2 = HashMap::new();
        d2.insert("k".to_string(), "v".to_string());
        let p2 = MatrixPoint::new(d2);

        assert_eq!(p1, p2);
    }

    #[test]
    fn test_ci_job_clone() {
        let point = MatrixPoint::new(HashMap::new());
        let job = CiJob::new("test", point);
        let cloned = job.clone();
        assert_eq!(cloned.name, job.name);
    }

    #[test]
    fn test_dimension_with_many_values() {
        let values: Vec<&str> = (0..100)
            .map(|i| Box::leak(format!("val{}", i).into_boxed_str()) as &str)
            .collect();
        let dim = MatrixDimension::new("test", values);
        assert_eq!(dim.count(), 100);
    }

    #[test]
    fn test_3_dimension_matrix_cartesian_product() {
        let mut matrix = TestMatrix::new();
        matrix.add_dimension(MatrixDimension::new("d1", vec!["a", "b"]));
        matrix.add_dimension(MatrixDimension::new("d2", vec!["1", "2", "3"]));
        matrix.add_dimension(MatrixDimension::new("d3", vec!["x", "y"]));

        assert_eq!(matrix.count(), 12); // 2 * 3 * 2
    }

    #[test]
    fn test_label_consistency() {
        let mut d1 = HashMap::new();
        d1.insert("a".to_string(), "1".to_string());
        d1.insert("b".to_string(), "2".to_string());
        let p1 = MatrixPoint::new(d1);

        let mut d2 = HashMap::new();
        d2.insert("b".to_string(), "2".to_string());
        d2.insert("a".to_string(), "1".to_string());
        let p2 = MatrixPoint::new(d2);

        assert_eq!(p1.label(), p2.label());
    }

    #[test]
    fn test_ci_step_clone() {
        let step = CiStep {
            name: "test".to_string(),
            command: "echo hi".to_string(),
            env: HashMap::new(),
        };
        let cloned = step.clone();
        assert_eq!(cloned.name, step.name);
    }

    #[test]
    fn test_matrix_expand_single_dimension() {
        let mut matrix = TestMatrix::new();
        matrix.add_dimension(MatrixDimension::new("test", vec!["a", "b", "c"]));

        let points = matrix.expand();
        assert_eq!(points.len(), 3);
    }

    #[test]
    fn test_empty_exclude_list() {
        let mut matrix = TestMatrix::new();
        matrix.add_dimension(MatrixDimension::new("a", vec!["1", "2"]));
        matrix.add_dimension(MatrixDimension::new("b", vec!["x", "y"]));

        let count_before = matrix.count();
        assert_eq!(count_before, 4);
    }
}
