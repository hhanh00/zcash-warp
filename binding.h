#if !defined(__APPLE__) || !defined(TARGET_OS_IPHONE)
typedef signed char int8_t;
typedef unsigned char uint8_t;
typedef unsigned short int uint16_t;
typedef long long int int64_t;
typedef unsigned long long int uint64_t;
typedef unsigned long int uintptr_t;
typedef int int32_t;
typedef unsigned int uint32_t;
#ifndef __cplusplus
typedef char bool;
#endif
#endif
typedef void *DartPostCObjectFnType;


typedef struct CResult_u8 {
  uint8_t value;
  char *error;
  uint32_t len;
} CResult_u8;

typedef struct CResult______u8 {
  const uint8_t *value;
  char *error;
  uint32_t len;
} CResult______u8;

typedef struct CResult_bool {
  bool value;
  char *error;
  uint32_t len;
} CResult_bool;

typedef struct CParam {
  uint8_t *value;
  uint32_t len;
} CParam;

typedef struct CResult_u32 {
  uint32_t value;
  char *error;
  uint32_t len;
} CResult_u32;

typedef struct CResult_____c_char {
  char *value;
  char *error;
  uint32_t len;
} CResult_____c_char;

typedef struct CResult_u64 {
  uint64_t value;
  char *error;
  uint32_t len;
} CResult_u64;

struct CResult_u8 c_add_contact(uint8_t coin,
                                uint32_t account,
                                char *name,
                                char *address,
                                bool saved);

struct CResult______u8 c_get_txs(uint8_t coin, uint32_t account, uint32_t bc_height);

struct CResult_bool c_reset_tables(uint8_t coin, bool upgrade);

struct CResult______u8 c_list_accounts(uint8_t coin);

struct CResult______u8 c_list_account_transparent_addresses(uint8_t coin, uint32_t account);

struct CResult______u8 c_get_balance(uint8_t coin, uint32_t account, uint32_t height);

struct CResult______u8 c_get_account_signing_capabilities(uint8_t coin, uint32_t account);

struct CResult______u8 c_get_account_property(uint8_t coin, uint32_t account, char *name);

struct CResult_u8 c_set_account_property(uint8_t coin,
                                         uint32_t account,
                                         char *name,
                                         struct CParam value);

struct CResult______u8 c_get_spendings(uint8_t coin, uint32_t account, uint32_t timestamp);

struct CResult_u32 c_create_new_account(uint8_t coin,
                                        char *name,
                                        char *key,
                                        uint32_t acc_index,
                                        uint32_t birth);

struct CResult_u8 c_new_transparent_address(uint8_t coin, uint32_t account);

struct CResult_u8 c_edit_account_name(uint8_t coin, uint32_t account, char *name);

struct CResult_u8 c_edit_account_birth(uint8_t coin, uint32_t account, uint32_t birth);

struct CResult_u8 c_delete_account(uint8_t coin, uint32_t account);

struct CResult_u8 c_set_backup_reminder(uint8_t coin, uint32_t account, bool saved);

struct CResult_u8 c_downgrade_account(uint8_t coin, uint32_t account, struct CParam capabilities);

struct CResult_u32 c_get_sync_height(uint8_t coin);

struct CResult_u8 c_rewind(uint8_t coin, uint32_t height);

struct CResult______u8 c_list_checkpoints(uint8_t coin);

struct CResult_u32 c_store_contact(uint8_t coin, struct CParam contact);

struct CResult______u8 c_list_contact_cards(uint8_t coin);

struct CResult______u8 c_get_contact_card(uint8_t coin, uint32_t id);

struct CResult_u8 c_edit_contact_name(uint8_t coin, uint32_t id, char *name);

struct CResult_u8 c_edit_contact_address(uint8_t coin, uint32_t id, char *address);

struct CResult_u8 c_delete_contact(uint8_t coin, uint32_t id);

struct CResult______u8 c_list_messages(uint8_t coin, uint32_t account);

struct CResult_u8 c_mark_all_read(uint8_t coin, uint32_t account, bool reverse);

struct CResult_u8 c_mark_read(uint8_t coin, uint32_t id, bool reverse);

struct CResult______u8 c_get_unspent_notes(uint8_t coin, uint32_t account, uint32_t bc_height);

struct CResult______u8 c_get_unspent_utxos(uint8_t coin, uint32_t account, uint32_t bc_height);

struct CResult_u8 c_exclude_note(uint8_t coin, uint32_t id, bool reverse);

struct CResult_u8 c_reverse_note_exclusion(uint8_t coin, uint32_t account);

struct CResult______u8 c_get_tx_details(uint8_t coin, uint32_t id_tx);

struct CResult_____c_char c_generate_random_mnemonic_phrase_os_rng(uint8_t coin);

struct CResult_u32 c_get_last_height(uint8_t coin);

struct CResult_u64 c_ping(uint8_t coin, char *lwd_url);

struct CResult_u8 c_init_sapling_prover(uint8_t coin, struct CParam spend, struct CParam output);

struct CResult_u8 c_scan_transparent_addresses(uint8_t coin, uint32_t account, uint32_t gap_limit);

struct CResult_u8 c_retrieve_tx_details(uint8_t coin);

void c_setup(void);

struct CResult_u8 c_configure(uint8_t coin, struct CParam config);

struct CResult______u8 c_derive_zip32_keys(uint8_t coin, uint32_t account, uint32_t acc_index);

struct CResult_u8 c_check_db_password(char *path, char *password);

struct CResult_u8 c_encrypt_db(uint8_t coin, char *password, char *new_db_path);

struct CResult______u8 c_create_backup(uint8_t coin, uint32_t account);

struct CResult_____c_char c_get_address(uint8_t coin,
                                        uint32_t account,
                                        uint32_t time,
                                        uint8_t mask);

struct CResult_u8 c_set_db_path_password(uint8_t coin, char *path, char *password);

uint32_t c_schema_version(void);

struct CResult_u8 c_create_db(uint8_t coin, char *path, char *password);

struct CResult_u8 c_migrate_db(uint8_t coin, uint8_t major, char *src, char *dest, char *password);

struct CResult______u8 c_decode_address(uint8_t coin, char *address);

struct CResult_____c_char c_filter_address(uint8_t coin, char *address, uint8_t pool_mask);

struct CResult_____c_char c_make_payment_uri(uint8_t coin, struct CParam payment);

struct CResult______u8 c_parse_payment_uri(uint8_t coin,
                                           char *uri,
                                           uint32_t height,
                                           uint32_t expiration);

struct CResult_u8 c_is_valid_address_or_uri(uint8_t coin, char *s);

struct CResult_u32 c_get_activation_date(uint8_t coin);

struct CResult_u32 c_get_height_by_time(uint8_t coin, uint32_t time);

struct CResult_u32 c_get_activation_height(uint8_t coin);

struct CResult_u32 c_get_time_by_height(uint8_t coin, uint32_t height);

struct CResult_u8 c_reset_chain(uint8_t coin, uint32_t height);

struct CResult______u8 c_prev_message(uint8_t coin, uint32_t account, uint32_t height);

struct CResult______u8 c_next_message(uint8_t coin, uint32_t account, uint32_t height);

struct CResult______u8 c_prev_message_thread(uint8_t coin,
                                             uint32_t account,
                                             uint32_t height,
                                             char *subject);

struct CResult______u8 c_next_message_thread(uint8_t coin,
                                             uint32_t account,
                                             uint32_t height,
                                             char *subject);

struct CResult_u8 c_encrypt_zip_database_files(uint8_t coin, struct CParam zip_db_config);

struct CResult_u8 c_decrypt_zip_database_files(uint8_t coin,
                                               char *file_path,
                                               char *target_directory,
                                               char *secret_key);

struct CResult______u8 c_generate_zip_database_keys(uint8_t coin);

struct CResult______u8 c_split(uint8_t coin, struct CParam data, uint32_t threshold);

struct CResult______u8 c_merge(uint8_t coin, struct CParam parts);

struct CResult______u8 c_prepare_payment(uint8_t coin, uint32_t account, struct CParam payment);

struct CResult_bool c_can_sign(uint8_t coin, uint32_t account, struct CParam summary);

struct CResult______u8 c_sign(uint8_t coin, struct CParam summary, uint32_t expiration_height);

struct CResult_____c_char c_tx_broadcast(uint8_t coin, struct CParam txbytes);

struct CResult______u8 c_save_contacts(uint8_t coin,
                                       uint32_t account,
                                       uint32_t height,
                                       uint32_t confirmations);

struct CResult______u8 c_fetch_tx_details(uint8_t coin, uint32_t account, uint32_t id);

struct CResult_u8 warp_synchronize(uint8_t coin, uint32_t end_height);
