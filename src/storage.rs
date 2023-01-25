elrond_wasm::imports!();
elrond_wasm::derive_imports!();

#[derive(TopEncode, TopDecode, NestedEncode, NestedDecode, TypeAbi)]
pub struct BalancesAfterCancel<M: ManagedTypeApi> {
    pub sender_balance: BigUint<M>,
    pub recipient_balance: BigUint<M>
}

#[derive(TopEncode, TopDecode, NestedEncode, TypeAbi)]
pub struct Stream<M: ManagedTypeApi> {
    pub sender: ManagedAddress<M>,
    pub recipient: ManagedAddress<M>,
    pub payment_token: EgldOrEsdtTokenIdentifier<M>,
    pub payment_nonce: u64,
    pub deposit: BigUint<M>,
    pub claimed_amount: BigUint<M>,
    pub can_cancel: bool,
    pub start_time: u64,
    pub end_time: u64,
    pub balances_after_cancel: Option<BalancesAfterCancel<M>>
}

#[elrond_wasm::module]
pub trait StorageModule {
    #[storage_mapper("streamById")]
    fn stream_by_id(&self, stream_id: u64) -> SingleValueMapper<Stream<Self::Api>>;

    #[view(getStreamListByAddress)]
    #[storage_mapper("streamsList")]
    fn streams_list(&self, address: ManagedAddress) -> UnorderedSetMapper<u64>;

    #[view(getLastStreamId)]
    #[storage_mapper("lastStreamId")]
    fn last_stream_id(&self) -> SingleValueMapper<u64>;
}