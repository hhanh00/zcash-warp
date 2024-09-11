use zcash_warp::{cli::init_config, coin::init_coin, db::{self, account::{c_list_accounts, list_accounts}}, utils::init_tracing};

pub fn main() {
    init_tracing();
    init_config();
    init_coin().unwrap();
    c_list_accounts(0);
}
