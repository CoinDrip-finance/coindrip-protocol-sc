#![no_std]

multiversx_sc::imports!();
multiversx_sc::derive_imports!();

pub mod storage;
mod events;
pub mod errors;
use storage::{Stream, BalancesAfterCancel};

use errors::{
    ERR_STREAM_TO_SC,
    ERR_STREAM_TO_CALLER,
    ERR_ZERO_DEPOSIT,
    ERR_START_TIME,
    ERR_END_TIME,
    ERR_ONLY_RECIPIENT_CLAIM,
    ERR_ZERO_CLAIM,
    ERR_CANT_CANCEL,
    ERR_CANCEL_ONLY_OWNERS,
    ERR_INVALID_STREAM,
    ERR_STREAM_IS_CANCELLED,
    ERR_STREAM_IS_NOT_CANCELLED,
    ERR_ONLY_RECIPIENT_SENDER_CAN_CLAIM
};
#[multiversx_sc::contract]
pub trait CoinDrip:
    storage::StorageModule
    + events::EventsModule {
    #[init]
    fn init(
        &self
    ) {
    }

    #[payable("*")]
    #[endpoint(createStream)]
    fn create_stream(
        &self,
        recipient: ManagedAddress,
        start_time: u64,
        end_time: u64,
        _can_cancel: OptionalValue<bool>
    ) {
        let caller = self.blockchain().get_caller();
        require!(recipient != self.blockchain().get_sc_address(), ERR_STREAM_TO_SC);
        require!(recipient != caller , ERR_STREAM_TO_CALLER);

        let (token_identifier, token_nonce, token_amount) = self.call_value().egld_or_single_esdt().into_tuple();

        require!(token_amount > 0, ERR_ZERO_DEPOSIT);

        let current_time = self.blockchain().get_block_timestamp();
        require!(start_time >= current_time, ERR_START_TIME);
        require!(end_time > start_time, ERR_END_TIME);

        let stream_id = self.last_stream_id().get() + 1;
        self.last_stream_id().set(&stream_id);

        let can_cancel: bool = (&_can_cancel.into_option()).unwrap_or(true);

        self.streams_list(&caller).insert(stream_id);
        self.streams_list(&recipient).insert(stream_id);

        self.create_stream_event(stream_id, &caller, &recipient, &token_identifier, token_nonce, &token_amount, start_time, end_time);
        
        let stream = Stream {
            sender: caller,
            recipient,
            payment_token: token_identifier,
            payment_nonce: token_nonce,
            deposit: token_amount,
            claimed_amount: BigUint::zero(),
            can_cancel,
            start_time,
            end_time,
            balances_after_cancel: None
        };
        self.stream_by_id(stream_id).set(&stream);
    }

    ///
    /// Calculates the recipient balance based on the amount stream so far and the already claimed amount
    /// |xxxx|*******|--|
    /// S            C  E
    /// S = start time
    /// xxxx = already claimed amount
    /// C = current time
    /// E = end time
    /// The zone marked with "****..." represents the recipient balance
    #[view(recipientBalance)]
    fn recipient_balance(&self, stream_id: u64) -> BigUint {
        let stream = self.get_stream(stream_id);
        let current_time = self.blockchain().get_block_timestamp();

        if current_time < stream.start_time {
            return BigUint::zero();
        }

        let streamed_so_far = &stream.deposit * (current_time - stream.start_time) / (stream.end_time - stream.start_time);
        let recipient_balance = streamed_so_far.min(stream.deposit) - (stream.claimed_amount);

        recipient_balance
    }

    /// Calculates the sender balance based on the recipient balance and the claimed balance
    /// |----|-------|**|
    /// S   L.C      C  E
    /// S = start time
    /// L.C = last claimed amount
    /// C = current time
    /// E = end time
    /// The zone marked with "**" represents the sender balance
    #[view(senderBalance)]
    fn sender_balance(&self, stream_id: u64) -> BigUint {
        let stream = self.get_stream(stream_id);

        stream.deposit - self.recipient_balance(stream_id) - stream.claimed_amount
    }

    fn is_stream_finalized(&self, stream_id: u64) -> bool {
        let stream = self.get_stream(stream_id);
        let current_time = self.blockchain().get_block_timestamp();
        let is_finalized = current_time >= stream.end_time;
        return is_finalized;
    }

    /// This endpoint can be used by the recipient of the stream to claim the stream amount of tokens
    #[endpoint(claimFromStream)]
    fn claim_from_stream(
        &self,
        stream_id: u64
    ) {
        let mut stream = self.get_stream(stream_id);

        require!(stream.balances_after_cancel.is_none(), ERR_STREAM_IS_CANCELLED);

        let caller = self.blockchain().get_caller();
        require!(caller == stream.recipient, ERR_ONLY_RECIPIENT_CLAIM);

        let amount = self.recipient_balance(stream_id);

        require!(amount > 0, ERR_ZERO_CLAIM);

        let is_finalized = self.is_stream_finalized(stream_id);

        if is_finalized {
            self.remove_stream(stream_id);
        } else {
            stream.claimed_amount += &amount;
            self.stream_by_id(stream_id).set(&stream);
        }

        self.send().direct(&caller, &stream.payment_token, stream.payment_nonce, &amount);

        self.claim_from_stream_event(stream_id, &amount, is_finalized);
    }

    /// This endpoint can be used the by sender or recipient of a stream to cancel the stream.
    /// !!! The stream needs to be cancelable (a property that is set when the stream is created by the sender)
    #[endpoint(cancelStream)]
    fn cancel_stream(
        &self,
        stream_id: u64,
        _with_claim: OptionalValue<bool>
    ) {
        let mut stream = self.get_stream(stream_id);

        require!(stream.balances_after_cancel.is_none(), ERR_STREAM_IS_CANCELLED);

        require!(stream.can_cancel, ERR_CANT_CANCEL);

        let caller = self.blockchain().get_caller();
        require!(caller == stream.recipient || caller == stream.sender, ERR_CANCEL_ONLY_OWNERS);

        let sender_balance = self.sender_balance(stream_id);
        let recipient_balance = self.recipient_balance(stream_id);

        stream.balances_after_cancel = Some(BalancesAfterCancel {
            sender_balance,
            recipient_balance
        });

        self.stream_by_id(stream_id).set(stream);

        let with_claim: bool = (&_with_claim.into_option()).unwrap_or(true);
        if with_claim {
            self.claim_from_stream_after_cancel(stream_id);
        }

        self.cancel_stream_event(stream_id, &caller);
    }

    /// After a stream was cancelled, you can call this endpoint to claim the streamed tokens as a recipient or the remaining tokens as a sender
    /// This endpoint is especially helpful when the recipient/sender is a non-payable smart contract
    /// For convenience, this endpoint is automatically called by default from the cancel_stream endpoint (is not instructed otherwise by the "_with_claim" param)
    #[endpoint(claimFromStreamAfterCancel)]
    fn claim_from_stream_after_cancel(
        &self,
        stream_id: u64
    ) {
        let stream = self.get_stream(stream_id);

        require!(stream.balances_after_cancel.is_some(), ERR_STREAM_IS_NOT_CANCELLED);

        let caller = self.blockchain().get_caller();
        require!(caller == stream.recipient || caller == stream.sender, ERR_ONLY_RECIPIENT_SENDER_CAN_CLAIM);

        let balances_after_cancel = stream.balances_after_cancel.unwrap();
        
        if balances_after_cancel.recipient_balance > 0 {
            self.send().direct(&stream.recipient, &stream.payment_token, stream.payment_nonce, &balances_after_cancel.recipient_balance);
            self.claim_from_stream_event(stream_id, &balances_after_cancel.recipient_balance, false);
        }

        if balances_after_cancel.sender_balance > 0{
            self.send().direct(&stream.sender, &stream.payment_token, stream.payment_nonce, &balances_after_cancel.sender_balance);
        }

        self.remove_stream(stream_id);
    }

    fn remove_stream(&self, stream_id: u64) {
        let stream = self.get_stream(stream_id);

        self.stream_by_id(stream_id).clear();
        self.streams_list(&stream.recipient).swap_remove(&stream_id);
        self.streams_list(&stream.sender).swap_remove(&stream_id);
    }

    #[view(getStreamData)]
    fn get_stream(&self, stream_id: u64) -> Stream<Self::Api> {
        let stream_mapper = self.stream_by_id(stream_id);
        require!(!stream_mapper.is_empty(), ERR_INVALID_STREAM);
        stream_mapper.get()
    }
}
