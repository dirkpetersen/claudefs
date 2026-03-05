pub struct CrossNodeHealth {
    nodes: Vec<(u32, bool)>,
}

impl CrossNodeHealth {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn report(&mut self, id: u32, healthy: bool) {
        self.nodes.retain(|(n, _)| n != &id);
        self.nodes.push((id, healthy));
    }

    pub fn healthy_count(&self) -> usize {
        self.nodes.iter().filter(|(_, h)| *h).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_report() {
        let mut h = CrossNodeHealth::new();
        h.report(1, true);
        assert_eq!(h.healthy_count(), 1);
    }
    #[test]
    fn test_offline() {
        let mut h = CrossNodeHealth::new();
        h.report(1, false);
        assert_eq!(h.healthy_count(), 0);
    }
}
