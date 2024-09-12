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

struct CResult_u8 c_reset_tables(uint8_t coin);

struct CResult______u8 c_list_accounts(uint8_t coin);

struct CResult______u8 c_get_balance(uint8_t coin, uint32_t account, uint32_t height);

struct CResult______u8 c_get_account_property(uint8_t coin, uint32_t account, char *name);

struct CResult_u8 c_set_account_property(uint8_t coin,
                                         uint32_t account,
                                         char *name,
                                         struct CParam value);

struct CResult_u32 c_create_new_account(uint8_t coin,
                                        char *name,
                                        char *key,
                                        uint32_t acc_index,
                                        uint32_t birth);

struct CResult_u32 c_get_sync_height(uint8_t coin);

struct CResult_u32 c_get_last_height(uint8_t coin);

void c_setup(void);

struct CResult_u8 c_reset_chain(uint8_t coin, uint32_t height);

struct CResult______u8 c_pay(uint8_t coin,
                             uint32_t account,
                             struct CParam recipients,
                             uint8_t src_pools,
                             bool fee_paid_by_sender,
                             uint32_t confirmations);

struct CResult______u8 c_sign(uint8_t coin, struct CParam summary, uint32_t expiration_height);

struct CResult_____c_char c_tx_broadcast(uint8_t coin, struct CParam txbytes);

struct CResult_u8 warp_synchronize(uint8_t coin, uint32_t end_height);
