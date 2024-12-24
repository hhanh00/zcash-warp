pub fn create_account() -> Result<u32, String> {
    crate::db::account_manager::create_account(
        connection, name, seed, acc_index, addr_index, birth, is_new,
    );
    // create_account(
    //     connection: &Connection,
    //     name: &str,
    //     seed: Option<&str>,
    //     acc_index: u32,
    //     addr_index: u32,
    //     birth: u32,
    //     is_new: bool,
    // );
}
