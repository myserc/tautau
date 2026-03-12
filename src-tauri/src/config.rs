use lazy_static::lazy_static;
use crate::core::Unit;

pub struct Config {
    pub mode: String,
    pub limit: usize,
    pub total_book_counts: i64,
    pub standard_mint_scarcity: u64,
    pub units: Vec<Unit>,
    pub num_agents: u32,
}

lazy_static! {
    pub static ref CONFIG: Config = {
        let _mode = "finn".to_string(); // In mobile, default to finn or get from app settings
        Config {
            mode: "finn".to_string(), limit: 1_200_000, total_book_counts: 10_800, standard_mint_scarcity: 114_113,
            units: vec![Unit::Day, Unit::Degree, Unit::Twin], num_agents: 250_000,
        }
    };
}
