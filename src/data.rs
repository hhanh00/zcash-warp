use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct BackupT {
    pub name: String,
    pub seed: String,
    pub index: u32,
    pub sk: String,
    pub fvk: String,
    pub uvk: String,
    pub tsk: String,
    pub txsk: String,
    pub tvk: String,
    pub taddr: String,
    pub birth: u32,
    pub saved: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TransactionInfoT {
    pub id: u32,
    pub txid: Vec<u8>,
    pub height: u32,
    pub confirmations: u32,
    pub timestamp: u32,
    pub amount: i64,
    pub address: String,
    pub contact: String,
    pub memo: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TransactionInfoExtendedT {
    pub height: u32,
    pub timestamp: u32,
    pub txid: Vec<u8>,
    pub tins: Vec<InputTransparentT>,
    pub touts: Vec<OutputTransparentT>,
    pub sins: Vec<InputShieldedT>,
    pub souts: Vec<OutputShieldedT>,
    pub oins: Vec<InputShieldedT>,
    pub oouts: Vec<OutputShieldedT>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct InputTransparentT {
    pub txid: Vec<u8>,
    pub vout: u32,
    pub address: String,
    pub value: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct OutputTransparentT {
    pub address: String,
    pub value: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct InputShieldedT {
    pub orchard: bool,
    pub nf: Vec<u8>,
    pub address: String,
    pub value: u64,
    pub rcm: Vec<u8>,
    pub rho: Vec<u8>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct OutputShieldedT {
    pub orchard: bool,
    pub incoming: bool,
    pub cmx: Vec<u8>,
    pub address: String,
    pub value: u64,
    pub rcm: Vec<u8>,
    pub rho: Vec<u8>,
    pub memo: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ShieldedNoteT {
    pub id_note: u32,
    pub height: u32,
    pub confirmations: u32,
    pub timestamp: u32,
    pub value: u64,
    pub orchard: bool,
    pub excluded: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ShieldedMessageT {
    pub id_msg: u32,
    pub account: u32,
    pub id_tx: u32,
    pub txid: Vec<u8>,
    pub height: u32,
    pub timestamp: u32,
    pub incoming: bool,
    pub contact: String,
    pub nout: u32,
    pub memo: UserMemoT,
    pub read: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct UAReceiversT {
    pub tex: bool,
    pub transparent: String,
    pub sapling: String,
    pub orchard: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct RecipientT {
    pub address: String,
    pub amount: u64,
    pub pools: u8,
    pub memo: Option<UserMemoT>,
    pub memo_bytes: Vec<u8>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PaymentRequestT {
    pub recipients: Vec<RecipientT>,
    pub src_pools: u8,
    pub sender_pay_fees: bool,
    pub use_change: bool,
    pub height: u32,
    pub expiration: u32,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AccountNameT {
    pub coin: u8,
    pub id: u32,
    pub name: String,
    pub birth: u32,
    pub icon: Vec<u8>,
    pub balance: u64,
    pub hidden: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AccountNameListT {
    pub items: Vec<AccountNameT>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ContactCardT {
    pub id: u32,
    pub account: u32,
    pub name: String,
    pub address: String,
    pub saved: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TransactionRecipientT {
    pub address: String,
    pub amount: u64,
    pub change: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TransactionSummaryT {
    pub height: u32,
    pub recipients: Vec<TransactionRecipientT>,
    pub transparent_ins: u64,
    pub sapling_net: i64,
    pub orchard_net: i64,
    pub fee: u64,
    pub privacy_level: u8,
    pub num_inputs: Vec<u8>,
    pub num_outputs: Vec<u8>,
    pub data: Vec<u8>,
    pub message: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AGEKeysT {
    pub public_key: String,
    pub secret_key: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct BalanceT {
    pub transparent: u64,
    pub sapling: u64,
    pub orchard: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PacketT {
    pub len: u32,
    pub data: Vec<u8>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PacketsT {
    pub packets: Vec<PacketT>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct CheckpointT {
    pub height: u32,
    pub hash: Vec<u8>,
    pub timestamp: u32,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SpendingT {
    pub recipient: String,
    pub amount: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ProgressT {
    pub trial_decryptions: u32,
    pub downloaded: u64,
    pub height: u32,
    pub timestamp: u32,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct UserMemoT {
    pub reply_to: bool,
    pub sender: String,
    pub recipient: String,
    pub body: String,
    pub subject: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ZIP32KeysT {
    pub aindex: u32,
    pub addr_index: u32,
    pub tsk: String,
    pub taddress: String,
    pub zsk: String,
    pub zaddress: String,
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct ConfigT {
    pub db_path: String,
    pub servers: Vec<String>,
    pub warp_url: String,
    pub warp_end_height: u32,
    pub confirmations: u32,
    pub regtest: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AccountSigningCapabilitiesT {
    pub seed: bool,
    pub transparent: u8,
    pub sapling: u8,
    pub orchard: u8,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SchemaVersionT {
    pub major: u8,
    pub minor: u8,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ZipDbConfigT {
    pub directory: String,
    pub file_list: Vec<String>,
    pub target_path: String,
    pub public_key: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TransparentAddressT {
    pub account: u32,
    pub external: u32,
    pub addr_index: u32,
    pub address: String,
    pub amount: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct IdNoteT {
    pub pool: u8,
    pub id: u32,
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct TransactionBytesT {
    pub notes: Vec<IdNoteT>,
    pub data: Vec<u8>,
    pub message: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct UnconfirmedTxT {
    pub account: u32,
    pub txid: Vec<u8>,
    pub value: i64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SwapT {
    pub provider: String,
    pub provider_id: String,
    pub timestamp: u32,
    pub from_currency: String,
    pub from_amount: String,
    pub from_address: String,
    pub from_image: String,
    pub to_currency: String,
    pub to_amount: String,
    pub to_address: String,
    pub to_image: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SwapListT {
    pub items: Vec<SwapT>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SpendableT {
    pub total: u64,
    pub unconfirmed: u64,
    pub immature: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ContactCardTT {
    pub id: u32,
    pub account: u32,
    pub name: String,
    pub address: String,
    pub saved: bool,
}
