use crate::collector::CoreMetrics;
use crate::topology::SystemTopology;
use std::collections::HashMap;

pub struct App {
    pub topo: SystemTopology,
    pub metrics: HashMap<u32, CoreMetrics>,
    pub should_quit: bool,
    pub show_help: bool,
}

impl App {
    pub fn new(topo: SystemTopology) -> Self {
        Self {
            topo,
            metrics: HashMap::new(),
            should_quit: false,
            show_help: false,
        }
    }
    pub fn update_metrics(&mut self, new_metrics: HashMap<u32, CoreMetrics>) {
        self.metrics = new_metrics;
    }
    pub fn quit(&mut self) {
        self.should_quit = true;
    }
    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }
}
