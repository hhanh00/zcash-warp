/// CompactBlock is a packaging of ONLY the data from a block that's needed to:
///    1. Detect a payment to your shielded Sapling address
///    2. Detect a spend of your shielded Sapling notes
///    3. Update your witnesses to generate new Sapling spend proofs.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct CompactBlock {
    /// the version of this wire format, for storage
    #[prost(uint32, tag = "1")]
    pub proto_version: u32,
    /// the height of this block
    #[prost(uint64, tag = "2")]
    pub height: u64,
    /// the ID (hash) of this block, same as in block explorers
    #[prost(bytes = "vec", tag = "3")]
    pub hash: ::prost::alloc::vec::Vec<u8>,
    /// the ID (hash) of this block's predecessor
    #[prost(bytes = "vec", tag = "4")]
    pub prev_hash: ::prost::alloc::vec::Vec<u8>,
    /// Unix epoch time when the block was mined
    #[prost(uint32, tag = "5")]
    pub time: u32,
    /// (hash, prevHash, and time) OR (full header)
    #[prost(bytes = "vec", tag = "6")]
    pub header: ::prost::alloc::vec::Vec<u8>,
    /// zero or more compact transactions from this block
    #[prost(message, repeated, tag = "7")]
    pub vtx: ::prost::alloc::vec::Vec<CompactTx>,
}
/// CompactTx contains the minimum information for a wallet to know if this transaction
/// is relevant to it (either pays to it or spends from it) via shielded elements
/// only. This message will not encode a transparent-to-transparent transaction.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct CompactTx {
    /// the index within the full block
    #[prost(uint64, tag = "1")]
    pub index: u64,
    /// the ID (hash) of this transaction, same as in block explorers
    #[prost(bytes = "vec", tag = "2")]
    pub hash: ::prost::alloc::vec::Vec<u8>,
    /// The transaction fee: present if server can provide. In the case of a
    /// stateless server and a transaction with transparent inputs, this will be
    /// unset because the calculation requires reference to prior transactions.
    /// in a pure-Sapling context, the fee will be calculable as:
    ///     valueBalance + (sum(vPubNew) - sum(vPubOld) - sum(tOut))
    #[prost(uint32, tag = "3")]
    pub fee: u32,
    /// inputs
    #[prost(message, repeated, tag = "4")]
    pub spends: ::prost::alloc::vec::Vec<CompactSaplingSpend>,
    /// outputs
    #[prost(message, repeated, tag = "5")]
    pub outputs: ::prost::alloc::vec::Vec<CompactSaplingOutput>,
    #[prost(message, repeated, tag = "6")]
    pub actions: ::prost::alloc::vec::Vec<CompactOrchardAction>,
    #[prost(message, optional, tag = "101")]
    pub sapling_bridge: ::core::option::Option<Bridge>,
    #[prost(message, optional, tag = "102")]
    pub orchard_bridge: ::core::option::Option<Bridge>,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Bridge {
    #[prost(uint32, tag = "1")]
    pub len: u32,
    #[prost(message, optional, tag = "2")]
    pub start: ::core::option::Option<Edge>,
    #[prost(message, optional, tag = "3")]
    pub end: ::core::option::Option<Edge>,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Edge {
    #[prost(message, repeated, tag = "1")]
    pub levels: ::prost::alloc::vec::Vec<OptLevel>,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct OptLevel {
    #[prost(bytes = "vec", tag = "1")]
    pub hash: ::prost::alloc::vec::Vec<u8>,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Either {
    #[prost(oneof = "either::Side", tags = "1, 2")]
    pub side: ::core::option::Option<either::Side>,
}
/// Nested message and enum types in `Either`.
pub mod either {
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Side {
        #[prost(bytes, tag = "1")]
        Left(::prost::alloc::vec::Vec<u8>),
        #[prost(bytes, tag = "2")]
        Right(::prost::alloc::vec::Vec<u8>),
    }
}
/// CompactSaplingSpend is a Sapling Spend Description as described in 7.3 of the Zcash
/// protocol specification.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct CompactSaplingSpend {
    /// nullifier (see the Zcash protocol specification)
    #[prost(bytes = "vec", tag = "1")]
    pub nf: ::prost::alloc::vec::Vec<u8>,
}
/// output is a Sapling Output Description as described in section 7.4 of the
/// Zcash protocol spec. Total size is 948.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct CompactSaplingOutput {
    /// note commitment u-coordinate
    #[prost(bytes = "vec", tag = "1")]
    pub cmu: ::prost::alloc::vec::Vec<u8>,
    /// ephemeral public key
    #[prost(bytes = "vec", tag = "2")]
    pub epk: ::prost::alloc::vec::Vec<u8>,
    /// first 52 bytes of ciphertext
    #[prost(bytes = "vec", tag = "3")]
    pub ciphertext: ::prost::alloc::vec::Vec<u8>,
}
/// <https://github.com/zcash/zips/blob/main/zip-0225.rst#orchard-action-description-orchardaction>
/// (but not all fields are needed)
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct CompactOrchardAction {
    /// \[32\] The nullifier of the input note
    #[prost(bytes = "vec", tag = "1")]
    pub nullifier: ::prost::alloc::vec::Vec<u8>,
    /// \[32\] The x-coordinate of the note commitment for the output note
    #[prost(bytes = "vec", tag = "2")]
    pub cmx: ::prost::alloc::vec::Vec<u8>,
    /// \[32\] An encoding of an ephemeral Pallas public key
    #[prost(bytes = "vec", tag = "3")]
    pub ephemeral_key: ::prost::alloc::vec::Vec<u8>,
    /// \[52\] The note plaintext component of the encCiphertext field
    #[prost(bytes = "vec", tag = "4")]
    pub ciphertext: ::prost::alloc::vec::Vec<u8>,
}
/// A BlockID message contains identifiers to select a block: a height or a
/// hash. Specification by hash is not implemented, but may be in the future.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BlockId {
    #[prost(uint64, tag = "1")]
    pub height: u64,
    #[prost(bytes = "vec", tag = "2")]
    pub hash: ::prost::alloc::vec::Vec<u8>,
}
/// BlockRange specifies a series of blocks from start to end inclusive.
/// Both BlockIDs must be heights; specification by hash is not yet supported.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BlockRange {
    #[prost(message, optional, tag = "1")]
    pub start: ::core::option::Option<BlockId>,
    #[prost(message, optional, tag = "2")]
    pub end: ::core::option::Option<BlockId>,
    #[prost(uint64, tag = "3")]
    pub spam_filter_threshold: u64,
}
/// A TxFilter contains the information needed to identify a particular
/// transaction: either a block and an index, or a direct transaction hash.
/// Currently, only specification by hash is supported.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TxFilter {
    /// block identifier, height or hash
    #[prost(message, optional, tag = "1")]
    pub block: ::core::option::Option<BlockId>,
    /// index within the block
    #[prost(uint64, tag = "2")]
    pub index: u64,
    /// transaction ID (hash, txid)
    #[prost(bytes = "vec", tag = "3")]
    pub hash: ::prost::alloc::vec::Vec<u8>,
}
/// RawTransaction contains the complete transaction data. It also optionally includes
/// the block height in which the transaction was included, or, when returned
/// by GetMempoolStream(), the latest block height.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RawTransaction {
    /// exact data returned by Zcash 'getrawtransaction'
    #[prost(bytes = "vec", tag = "1")]
    pub data: ::prost::alloc::vec::Vec<u8>,
    /// height that the transaction was mined (or -1)
    #[prost(uint64, tag = "2")]
    pub height: u64,
}
/// A SendResponse encodes an error code and a string. It is currently used
/// only by SendTransaction(). If error code is zero, the operation was
/// successful; if non-zero, it and the message specify the failure.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SendResponse {
    #[prost(int32, tag = "1")]
    pub error_code: i32,
    #[prost(string, tag = "2")]
    pub error_message: ::prost::alloc::string::String,
}
/// Chainspec is a placeholder to allow specification of a particular chain fork.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ChainSpec {}
/// Empty is for gRPCs that take no arguments, currently only GetLightdInfo.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Empty {}
/// LightdInfo returns various information about this lightwalletd instance
/// and the state of the blockchain.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct LightdInfo {
    #[prost(string, tag = "1")]
    pub version: ::prost::alloc::string::String,
    #[prost(string, tag = "2")]
    pub vendor: ::prost::alloc::string::String,
    /// true
    #[prost(bool, tag = "3")]
    pub taddr_support: bool,
    /// either "main" or "test"
    #[prost(string, tag = "4")]
    pub chain_name: ::prost::alloc::string::String,
    /// depends on mainnet or testnet
    #[prost(uint64, tag = "5")]
    pub sapling_activation_height: u64,
    /// protocol identifier, see consensus/upgrades.cpp
    #[prost(string, tag = "6")]
    pub consensus_branch_id: ::prost::alloc::string::String,
    /// latest block on the best chain
    #[prost(uint64, tag = "7")]
    pub block_height: u64,
    #[prost(string, tag = "8")]
    pub git_commit: ::prost::alloc::string::String,
    #[prost(string, tag = "9")]
    pub branch: ::prost::alloc::string::String,
    #[prost(string, tag = "10")]
    pub build_date: ::prost::alloc::string::String,
    #[prost(string, tag = "11")]
    pub build_user: ::prost::alloc::string::String,
    /// less than tip height if zcashd is syncing
    #[prost(uint64, tag = "12")]
    pub estimated_height: u64,
    /// example: "v4.1.1-877212414"
    #[prost(string, tag = "13")]
    pub zcashd_build: ::prost::alloc::string::String,
    /// example: "/MagicBean:4.1.1/"
    #[prost(string, tag = "14")]
    pub zcashd_subversion: ::prost::alloc::string::String,
}
/// TransparentAddressBlockFilter restricts the results to the given address
/// or block range.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TransparentAddressBlockFilter {
    /// t-address
    #[prost(string, tag = "1")]
    pub address: ::prost::alloc::string::String,
    /// start, end heights
    #[prost(message, optional, tag = "2")]
    pub range: ::core::option::Option<BlockRange>,
}
/// Duration is currently used only for testing, so that the Ping rpc
/// can simulate a delay, to create many simultaneous connections. Units
/// are microseconds.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Duration {
    #[prost(int64, tag = "1")]
    pub interval_us: i64,
}
/// PingResponse is used to indicate concurrency, how many Ping rpcs
/// are executing upon entry and upon exit (after the delay).
/// This rpc is used for testing only.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PingResponse {
    #[prost(int64, tag = "1")]
    pub entry: i64,
    #[prost(int64, tag = "2")]
    pub exit: i64,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Address {
    #[prost(string, tag = "1")]
    pub address: ::prost::alloc::string::String,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct AddressList {
    #[prost(string, repeated, tag = "1")]
    pub addresses: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Balance {
    #[prost(int64, tag = "1")]
    pub value_zat: i64,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Exclude {
    #[prost(bytes = "vec", repeated, tag = "1")]
    pub txid: ::prost::alloc::vec::Vec<::prost::alloc::vec::Vec<u8>>,
}
/// The TreeState is derived from the Zcash z_gettreestate rpc.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TreeState {
    /// "main" or "test"
    #[prost(string, tag = "1")]
    pub network: ::prost::alloc::string::String,
    /// block height
    #[prost(uint64, tag = "2")]
    pub height: u64,
    /// block id
    #[prost(string, tag = "3")]
    pub hash: ::prost::alloc::string::String,
    /// Unix epoch time when the block was mined
    #[prost(uint32, tag = "4")]
    pub time: u32,
    /// sapling commitment tree state
    #[prost(string, tag = "5")]
    pub sapling_tree: ::prost::alloc::string::String,
    /// orchard commitment tree state
    #[prost(string, tag = "6")]
    pub orchard_tree: ::prost::alloc::string::String,
}
/// Results are sorted by height, which makes it easy to issue another
/// request that picks up from where the previous left off.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetAddressUtxosArg {
    #[prost(string, repeated, tag = "1")]
    pub addresses: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
    #[prost(uint64, tag = "2")]
    pub start_height: u64,
    /// zero means unlimited
    #[prost(uint32, tag = "3")]
    pub max_entries: u32,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetAddressUtxosReply {
    #[prost(string, tag = "6")]
    pub address: ::prost::alloc::string::String,
    #[prost(bytes = "vec", tag = "1")]
    pub txid: ::prost::alloc::vec::Vec<u8>,
    #[prost(int32, tag = "2")]
    pub index: i32,
    #[prost(bytes = "vec", tag = "3")]
    pub script: ::prost::alloc::vec::Vec<u8>,
    #[prost(int64, tag = "4")]
    pub value_zat: i64,
    #[prost(uint64, tag = "5")]
    pub height: u64,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetAddressUtxosReplyList {
    #[prost(message, repeated, tag = "1")]
    pub address_utxos: ::prost::alloc::vec::Vec<GetAddressUtxosReply>,
}
/// Generated client implementations.
pub mod compact_tx_streamer_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct CompactTxStreamerClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl CompactTxStreamerClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: std::convert::TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> CompactTxStreamerClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::BoxBody>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> CompactTxStreamerClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::BoxBody>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::BoxBody>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::BoxBody>,
            >>::Error: Into<StdError> + Send + Sync,
        {
            CompactTxStreamerClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Return the height of the tip of the best chain
        pub async fn get_latest_block(
            &mut self,
            request: impl tonic::IntoRequest<super::ChainSpec>,
        ) -> Result<tonic::Response<super::BlockId>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/cash.z.wallet.sdk.rpc.CompactTxStreamer/GetLatestBlock",
            );
            self.inner.unary(request.into_request(), path, codec).await
        }
        /// Return the compact block corresponding to the given block identifier
        /// rpc GetBlock(BlockID) returns (CompactBlock) {}
        /// Return a list of consecutive compact blocks
        pub async fn get_block_range(
            &mut self,
            request: impl tonic::IntoRequest<super::BlockRange>,
        ) -> Result<
            tonic::Response<tonic::codec::Streaming<super::CompactBlock>>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/cash.z.wallet.sdk.rpc.CompactTxStreamer/GetBlockRange",
            );
            self.inner.server_streaming(request.into_request(), path, codec).await
        }
        pub async fn get_pruned_block_range(
            &mut self,
            request: impl tonic::IntoRequest<super::BlockRange>,
        ) -> Result<
            tonic::Response<tonic::codec::Streaming<super::CompactBlock>>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/cash.z.wallet.sdk.rpc.CompactTxStreamer/GetPrunedBlockRange",
            );
            self.inner.server_streaming(request.into_request(), path, codec).await
        }
        /// Return the requested full (not compact) transaction (as from zcashd)
        pub async fn get_transaction(
            &mut self,
            request: impl tonic::IntoRequest<super::TxFilter>,
        ) -> Result<tonic::Response<super::RawTransaction>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/cash.z.wallet.sdk.rpc.CompactTxStreamer/GetTransaction",
            );
            self.inner.unary(request.into_request(), path, codec).await
        }
        /// Submit the given transaction to the Zcash network
        pub async fn send_transaction(
            &mut self,
            request: impl tonic::IntoRequest<super::RawTransaction>,
        ) -> Result<tonic::Response<super::SendResponse>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/cash.z.wallet.sdk.rpc.CompactTxStreamer/SendTransaction",
            );
            self.inner.unary(request.into_request(), path, codec).await
        }
        /// Return the txids corresponding to the given t-address within the given block range
        pub async fn get_taddress_txids(
            &mut self,
            request: impl tonic::IntoRequest<super::TransparentAddressBlockFilter>,
        ) -> Result<
            tonic::Response<tonic::codec::Streaming<super::RawTransaction>>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/cash.z.wallet.sdk.rpc.CompactTxStreamer/GetTaddressTxids",
            );
            self.inner.server_streaming(request.into_request(), path, codec).await
        }
        /// GetTreeState returns the note commitment tree state corresponding to the given block.
        /// See section 3.7 of the Zcash protocol specification. It returns several other useful
        /// values also (even though they can be obtained using GetBlock).
        /// The block can be specified by either height or hash.
        pub async fn get_tree_state(
            &mut self,
            request: impl tonic::IntoRequest<super::BlockId>,
        ) -> Result<tonic::Response<super::TreeState>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/cash.z.wallet.sdk.rpc.CompactTxStreamer/GetTreeState",
            );
            self.inner.unary(request.into_request(), path, codec).await
        }
        /// rpc GetAddressUtxos(GetAddressUtxosArg) returns (GetAddressUtxosReplyList) {}
        pub async fn get_address_utxos_stream(
            &mut self,
            request: impl tonic::IntoRequest<super::GetAddressUtxosArg>,
        ) -> Result<
            tonic::Response<tonic::codec::Streaming<super::GetAddressUtxosReply>>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/cash.z.wallet.sdk.rpc.CompactTxStreamer/GetAddressUtxosStream",
            );
            self.inner.server_streaming(request.into_request(), path, codec).await
        }
        /// Return information about this lightwalletd instance and the blockchain
        pub async fn get_lightd_info(
            &mut self,
            request: impl tonic::IntoRequest<super::Empty>,
        ) -> Result<tonic::Response<super::LightdInfo>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/cash.z.wallet.sdk.rpc.CompactTxStreamer/GetLightdInfo",
            );
            self.inner.unary(request.into_request(), path, codec).await
        }
        /// Testing-only, requires lightwalletd --ping-very-insecure (do not enable in production)
        pub async fn ping(
            &mut self,
            request: impl tonic::IntoRequest<super::Duration>,
        ) -> Result<tonic::Response<super::PingResponse>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/cash.z.wallet.sdk.rpc.CompactTxStreamer/Ping",
            );
            self.inner.unary(request.into_request(), path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod compact_tx_streamer_server {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with CompactTxStreamerServer.
    #[async_trait]
    pub trait CompactTxStreamer: Send + Sync + 'static {
        /// Return the height of the tip of the best chain
        async fn get_latest_block(
            &self,
            request: tonic::Request<super::ChainSpec>,
        ) -> Result<tonic::Response<super::BlockId>, tonic::Status>;
        /// Server streaming response type for the GetBlockRange method.
        type GetBlockRangeStream: futures_core::Stream<
                Item = Result<super::CompactBlock, tonic::Status>,
            >
            + Send
            + 'static;
        /// Return the compact block corresponding to the given block identifier
        /// rpc GetBlock(BlockID) returns (CompactBlock) {}
        /// Return a list of consecutive compact blocks
        async fn get_block_range(
            &self,
            request: tonic::Request<super::BlockRange>,
        ) -> Result<tonic::Response<Self::GetBlockRangeStream>, tonic::Status>;
        /// Server streaming response type for the GetPrunedBlockRange method.
        type GetPrunedBlockRangeStream: futures_core::Stream<
                Item = Result<super::CompactBlock, tonic::Status>,
            >
            + Send
            + 'static;
        async fn get_pruned_block_range(
            &self,
            request: tonic::Request<super::BlockRange>,
        ) -> Result<tonic::Response<Self::GetPrunedBlockRangeStream>, tonic::Status>;
        /// Return the requested full (not compact) transaction (as from zcashd)
        async fn get_transaction(
            &self,
            request: tonic::Request<super::TxFilter>,
        ) -> Result<tonic::Response<super::RawTransaction>, tonic::Status>;
        /// Submit the given transaction to the Zcash network
        async fn send_transaction(
            &self,
            request: tonic::Request<super::RawTransaction>,
        ) -> Result<tonic::Response<super::SendResponse>, tonic::Status>;
        /// Server streaming response type for the GetTaddressTxids method.
        type GetTaddressTxidsStream: futures_core::Stream<
                Item = Result<super::RawTransaction, tonic::Status>,
            >
            + Send
            + 'static;
        /// Return the txids corresponding to the given t-address within the given block range
        async fn get_taddress_txids(
            &self,
            request: tonic::Request<super::TransparentAddressBlockFilter>,
        ) -> Result<tonic::Response<Self::GetTaddressTxidsStream>, tonic::Status>;
        /// GetTreeState returns the note commitment tree state corresponding to the given block.
        /// See section 3.7 of the Zcash protocol specification. It returns several other useful
        /// values also (even though they can be obtained using GetBlock).
        /// The block can be specified by either height or hash.
        async fn get_tree_state(
            &self,
            request: tonic::Request<super::BlockId>,
        ) -> Result<tonic::Response<super::TreeState>, tonic::Status>;
        /// Server streaming response type for the GetAddressUtxosStream method.
        type GetAddressUtxosStreamStream: futures_core::Stream<
                Item = Result<super::GetAddressUtxosReply, tonic::Status>,
            >
            + Send
            + 'static;
        /// rpc GetAddressUtxos(GetAddressUtxosArg) returns (GetAddressUtxosReplyList) {}
        async fn get_address_utxos_stream(
            &self,
            request: tonic::Request<super::GetAddressUtxosArg>,
        ) -> Result<tonic::Response<Self::GetAddressUtxosStreamStream>, tonic::Status>;
        /// Return information about this lightwalletd instance and the blockchain
        async fn get_lightd_info(
            &self,
            request: tonic::Request<super::Empty>,
        ) -> Result<tonic::Response<super::LightdInfo>, tonic::Status>;
        /// Testing-only, requires lightwalletd --ping-very-insecure (do not enable in production)
        async fn ping(
            &self,
            request: tonic::Request<super::Duration>,
        ) -> Result<tonic::Response<super::PingResponse>, tonic::Status>;
    }
    #[derive(Debug)]
    pub struct CompactTxStreamerServer<T: CompactTxStreamer> {
        inner: _Inner<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
    }
    struct _Inner<T>(Arc<T>);
    impl<T: CompactTxStreamer> CompactTxStreamerServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            let inner = _Inner(inner);
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for CompactTxStreamerServer<T>
    where
        T: CompactTxStreamer,
        B: Body + Send + 'static,
        B::Error: Into<StdError> + Send + 'static,
    {
        type Response = http::Response<tonic::body::BoxBody>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            let inner = self.inner.clone();
            match req.uri().path() {
                "/cash.z.wallet.sdk.rpc.CompactTxStreamer/GetLatestBlock" => {
                    #[allow(non_camel_case_types)]
                    struct GetLatestBlockSvc<T: CompactTxStreamer>(pub Arc<T>);
                    impl<
                        T: CompactTxStreamer,
                    > tonic::server::UnaryService<super::ChainSpec>
                    for GetLatestBlockSvc<T> {
                        type Response = super::BlockId;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ChainSpec>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move {
                                (*inner).get_latest_block(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = GetLatestBlockSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/cash.z.wallet.sdk.rpc.CompactTxStreamer/GetBlockRange" => {
                    #[allow(non_camel_case_types)]
                    struct GetBlockRangeSvc<T: CompactTxStreamer>(pub Arc<T>);
                    impl<
                        T: CompactTxStreamer,
                    > tonic::server::ServerStreamingService<super::BlockRange>
                    for GetBlockRangeSvc<T> {
                        type Response = super::CompactBlock;
                        type ResponseStream = T::GetBlockRangeStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::BlockRange>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move {
                                (*inner).get_block_range(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = GetBlockRangeSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            );
                        let res = grpc.server_streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/cash.z.wallet.sdk.rpc.CompactTxStreamer/GetPrunedBlockRange" => {
                    #[allow(non_camel_case_types)]
                    struct GetPrunedBlockRangeSvc<T: CompactTxStreamer>(pub Arc<T>);
                    impl<
                        T: CompactTxStreamer,
                    > tonic::server::ServerStreamingService<super::BlockRange>
                    for GetPrunedBlockRangeSvc<T> {
                        type Response = super::CompactBlock;
                        type ResponseStream = T::GetPrunedBlockRangeStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::BlockRange>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move {
                                (*inner).get_pruned_block_range(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = GetPrunedBlockRangeSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            );
                        let res = grpc.server_streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/cash.z.wallet.sdk.rpc.CompactTxStreamer/GetTransaction" => {
                    #[allow(non_camel_case_types)]
                    struct GetTransactionSvc<T: CompactTxStreamer>(pub Arc<T>);
                    impl<
                        T: CompactTxStreamer,
                    > tonic::server::UnaryService<super::TxFilter>
                    for GetTransactionSvc<T> {
                        type Response = super::RawTransaction;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::TxFilter>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move {
                                (*inner).get_transaction(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = GetTransactionSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/cash.z.wallet.sdk.rpc.CompactTxStreamer/SendTransaction" => {
                    #[allow(non_camel_case_types)]
                    struct SendTransactionSvc<T: CompactTxStreamer>(pub Arc<T>);
                    impl<
                        T: CompactTxStreamer,
                    > tonic::server::UnaryService<super::RawTransaction>
                    for SendTransactionSvc<T> {
                        type Response = super::SendResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RawTransaction>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move {
                                (*inner).send_transaction(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = SendTransactionSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/cash.z.wallet.sdk.rpc.CompactTxStreamer/GetTaddressTxids" => {
                    #[allow(non_camel_case_types)]
                    struct GetTaddressTxidsSvc<T: CompactTxStreamer>(pub Arc<T>);
                    impl<
                        T: CompactTxStreamer,
                    > tonic::server::ServerStreamingService<
                        super::TransparentAddressBlockFilter,
                    > for GetTaddressTxidsSvc<T> {
                        type Response = super::RawTransaction;
                        type ResponseStream = T::GetTaddressTxidsStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::TransparentAddressBlockFilter>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move {
                                (*inner).get_taddress_txids(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = GetTaddressTxidsSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            );
                        let res = grpc.server_streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/cash.z.wallet.sdk.rpc.CompactTxStreamer/GetTreeState" => {
                    #[allow(non_camel_case_types)]
                    struct GetTreeStateSvc<T: CompactTxStreamer>(pub Arc<T>);
                    impl<
                        T: CompactTxStreamer,
                    > tonic::server::UnaryService<super::BlockId>
                    for GetTreeStateSvc<T> {
                        type Response = super::TreeState;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::BlockId>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move {
                                (*inner).get_tree_state(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = GetTreeStateSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/cash.z.wallet.sdk.rpc.CompactTxStreamer/GetAddressUtxosStream" => {
                    #[allow(non_camel_case_types)]
                    struct GetAddressUtxosStreamSvc<T: CompactTxStreamer>(pub Arc<T>);
                    impl<
                        T: CompactTxStreamer,
                    > tonic::server::ServerStreamingService<super::GetAddressUtxosArg>
                    for GetAddressUtxosStreamSvc<T> {
                        type Response = super::GetAddressUtxosReply;
                        type ResponseStream = T::GetAddressUtxosStreamStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetAddressUtxosArg>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move {
                                (*inner).get_address_utxos_stream(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = GetAddressUtxosStreamSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            );
                        let res = grpc.server_streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/cash.z.wallet.sdk.rpc.CompactTxStreamer/GetLightdInfo" => {
                    #[allow(non_camel_case_types)]
                    struct GetLightdInfoSvc<T: CompactTxStreamer>(pub Arc<T>);
                    impl<T: CompactTxStreamer> tonic::server::UnaryService<super::Empty>
                    for GetLightdInfoSvc<T> {
                        type Response = super::LightdInfo;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::Empty>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move {
                                (*inner).get_lightd_info(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = GetLightdInfoSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/cash.z.wallet.sdk.rpc.CompactTxStreamer/Ping" => {
                    #[allow(non_camel_case_types)]
                    struct PingSvc<T: CompactTxStreamer>(pub Arc<T>);
                    impl<
                        T: CompactTxStreamer,
                    > tonic::server::UnaryService<super::Duration> for PingSvc<T> {
                        type Response = super::PingResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::Duration>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).ping(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = PingSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        Ok(
                            http::Response::builder()
                                .status(200)
                                .header("grpc-status", "12")
                                .header("content-type", "application/grpc")
                                .body(empty_body())
                                .unwrap(),
                        )
                    })
                }
            }
        }
    }
    impl<T: CompactTxStreamer> Clone for CompactTxStreamerServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
            }
        }
    }
    impl<T: CompactTxStreamer> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: CompactTxStreamer> tonic::server::NamedService
    for CompactTxStreamerServer<T> {
        const NAME: &'static str = "cash.z.wallet.sdk.rpc.CompactTxStreamer";
    }
}
