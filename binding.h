#include <cstdarg>
#include <cstdint>
#include <cstdlib>
#include <ostream>
#include <new>

constexpr static const VOffsetT Backup_VT_NAME = 4;

constexpr static const VOffsetT Backup_VT_SEED = 6;

constexpr static const VOffsetT Backup_VT_INDEX = 8;

constexpr static const VOffsetT Backup_VT_SK = 10;

constexpr static const VOffsetT Backup_VT_FVK = 12;

constexpr static const VOffsetT Backup_VT_UVK = 14;

constexpr static const VOffsetT Backup_VT_TSK = 16;

constexpr static const VOffsetT Backup_VT_SAVED = 18;

constexpr static const VOffsetT TransactionInfo_VT_ID = 4;

constexpr static const VOffsetT TransactionInfo_VT_TXID = 6;

constexpr static const VOffsetT TransactionInfo_VT_HEIGHT = 8;

constexpr static const VOffsetT TransactionInfo_VT_CONFIRMATIONS = 10;

constexpr static const VOffsetT TransactionInfo_VT_TIMESTAMP = 12;

constexpr static const VOffsetT TransactionInfo_VT_AMOUNT = 14;

constexpr static const VOffsetT TransactionInfo_VT_ADDRESS = 16;

constexpr static const VOffsetT TransactionInfo_VT_CONTACT = 18;

constexpr static const VOffsetT TransactionInfo_VT_MEMO = 20;

constexpr static const VOffsetT TransactionInfoExtended_VT_TINS = 10;

constexpr static const VOffsetT TransactionInfoExtended_VT_TOUTS = 12;

constexpr static const VOffsetT TransactionInfoExtended_VT_SINS = 14;

constexpr static const VOffsetT TransactionInfoExtended_VT_SOUTS = 16;

constexpr static const VOffsetT TransactionInfoExtended_VT_OINS = 18;

constexpr static const VOffsetT TransactionInfoExtended_VT_OOUTS = 20;

constexpr static const VOffsetT InputTransparent_VT_VOUT = 6;

constexpr static const VOffsetT InputTransparent_VT_VALUE = 10;

constexpr static const VOffsetT InputShielded_VT_NF = 4;

constexpr static const VOffsetT InputShielded_VT_RCM = 10;

constexpr static const VOffsetT InputShielded_VT_RHO = 12;

constexpr static const VOffsetT OutputShielded_VT_INCOMING = 4;

constexpr static const VOffsetT OutputShielded_VT_CMX = 6;

constexpr static const VOffsetT ShieldedNote_VT_ORCHARD = 12;

constexpr static const VOffsetT ShieldedMessage_VT_ID_TX = 4;

constexpr static const VOffsetT ShieldedMessage_VT_NOUT = 12;

constexpr static const VOffsetT ShieldedMessage_VT_SENDER = 14;

constexpr static const VOffsetT ShieldedMessage_VT_RECIPIENT = 16;

constexpr static const VOffsetT ShieldedMessage_VT_SUBJECT = 18;

constexpr static const VOffsetT ShieldedMessage_VT_BODY = 20;
