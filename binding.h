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

struct CResult_u8 c_init_coin(uint8_t coin);

struct CResult______u8 c_list_accounts(uint8_t coin);

struct CResult______u8 c_get_balance(uint8_t coin, uint32_t account, uint32_t height);

struct CResult______u8 c_get_account_property(uint8_t coin, uint32_t account, char *name);

struct CResult_u8 c_set_account_property(uint8_t coin,
                                         uint32_t account,
                                         char *name,
                                         uint8_t *value);

void c_setup(void);

struct CResult_u8 c_test(uint8_t coin, uint32_t account, char *s);
