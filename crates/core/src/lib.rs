pub mod api;
pub mod applications;
pub mod build;
pub mod client;
pub mod collector;
pub mod config;
pub mod fdinfo;
pub mod fsutil;
pub mod launch;
pub mod launchers;
pub mod logging;

pub const DASHBOARD_BUS: &str = "io.github.igpu_man.Dashboard";
pub const DASHBOARD_OBJECT: &str = "/io/github/igpu_man/Dashboard";
pub const DASHBOARD_INTERFACE: &str = "io.github.igpu_man.Dashboard1";
pub mod model;
pub mod recording;
pub mod storage;

pub const BUS: &str = "io.github.igpu_man.Collector";
pub const OBJECT: &str = "/io/github/igpu_man/Collector";
pub const INTERFACE: &str = "io.github.igpu_man.Collector1";
pub const API_VERSION: u32 = 1;
