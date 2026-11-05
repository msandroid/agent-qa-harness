pub mod browser;
pub mod mcp;
pub mod store;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const TOOLS: &str = include_str!("tools.json");
pub const WORKFLOW: &str = "Contract → reset target with existing tools → discovery → capture evidence → candidate → separate reset reproduction run → finalize each criterion → export. Resets and external observations are operator-attested. Ledger survives restart; live browsers do not. Never replay uncertain side effects. Internal game APIs do not establish normal-player-input acceptance. Reports remain local.";
