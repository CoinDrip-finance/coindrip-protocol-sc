elrond_wasm::imports!();
elrond_wasm::derive_imports!();

#[elrond_wasm::module]
pub trait EventsModule {
    #[event("createStream")]
    fn create_stream_event(
        &self,
        #[indexed] stream_id: u64,
        #[indexed] sender: &ManagedAddress,
        #[indexed] recipient: &ManagedAddress,
        #[indexed] payment_token: &EgldOrEsdtTokenIdentifier,
        #[indexed] payment_nonce: u64,
        #[indexed] deposit: &BigUint,
        #[indexed] start_time: u64,
        #[indexed] end_time: u64,
    );

    #[event("claimFromStream")]
    fn claim_from_stream_event(
        &self,
        #[indexed] stream_id: u64,
        #[indexed] amount: &BigUint,
        #[indexed] finalized: bool,
    );

    #[event("cancelStream")]
    fn cancel_stream_event(
        &self,
        #[indexed] stream_id: u64,
        #[indexed] canceled_by: &ManagedAddress,
    );
}  