#![no_std]

elrond_wasm::imports!();
elrond_wasm::derive_imports!();

pub mod storage;
mod events;
pub mod errors;
use storage::{Stream, BalancesAfterCancel};

use errors::{
    ERR_STREAM_TO_SC,
    ERR_STREAM_TO_CALLER,
    ERR_STREAM_ONLY_FUNGIBLE,
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
#[elrond_wasm::contract]
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
        require!(recipient != self.blockchain().get_sc_address(), ERR_STREAM_TO_SC);
        require!(recipient != self.blockchain().get_caller(), ERR_STREAM_TO_CALLER);

        let (token_identifier, token_nonce, token_amount) = self.call_value().egld_or_single_esdt().into_tuple();

        require!(token_nonce == 0, ERR_STREAM_ONLY_FUNGIBLE);

        require!(token_amount > 0, ERR_ZERO_DEPOSIT);

        let current_time = self.blockchain().get_block_timestamp();
        require!(start_time >= current_time, ERR_START_TIME);
        require!(end_time > start_time, ERR_END_TIME);

        let stream_id = self.last_stream_id().get() + 1;
        self.last_stream_id().set(&stream_id);

        let duration = end_time - start_time;
        let rate_per_second = token_amount.clone() / BigUint::from(duration);

        let caller = self.blockchain().get_caller();
        let can_cancel: bool = (&_can_cancel.into_option()).unwrap_or(true);

        let stream = Stream {
            sender: caller.clone(),
            recipient: recipient.clone(),
            payment_token: token_identifier.clone(),
            payment_nonce: token_nonce,
            deposit: token_amount.clone(),
            remaining_balance: token_amount.clone(),
            last_claim: start_time,
            rate_per_second,
            can_cancel,
            start_time,
            end_time,
            balances_after_cancel: None
        };
        self.stream_by_id(stream_id).set(&stream);

        self.streams_list(caller.clone()).insert(stream_id);
        self.streams_list(recipient.clone()).insert(stream_id);

        self.create_stream_event(stream_id, &caller, &recipient, &token_identifier, token_nonce, &token_amount, start_time, end_time);
    }

    /// The number of seconds that the recipient hasn't claimed yet
    /// |----|*******|--|
    /// S   L.C      C  E
    /// S = start time
    /// L.C = last claim time
    /// C = current time
    /// E = end time
    /// The zone marked with "****..." represents the delta
    fn delta_of_recipient(&self, stream_id: u64) -> u64 {
        let stream = self.get_stream(stream_id);
        let current_time = self.blockchain().get_block_timestamp();
        if current_time <= stream.last_claim {
            return 0;
        }
        if current_time < stream.end_time {
            return current_time - stream.last_claim;
        }

        stream.end_time - stream.last_claim
    }

    ///
    /// Calculates the recipient balance based on the recipient delta and the rate per second
    /// |----|*******|--|
    /// S   L.C      C  E
    /// S = start time
    /// L.C = last claim time
    /// C = current time
    /// E = end time
    /// The zone marked with "****..." represents the recipient balance
    fn recipient_balance(&self, stream_id: u64) -> BigUint {
        let stream = self.get_stream(stream_id);
        let delta = self.delta_of_recipient(stream_id);

        let recipient_balance = stream.rate_per_second.mul(delta);

        recipient_balance
    }

    /// Calculates the sender balance based on the recipient balance and the remaining balance
    /// |----|-------|**|
    /// S   L.C      C  E
    /// S = start time
    /// L.C = last claim time
    /// C = current time
    /// E = end time
    /// The zone marked with "**" represents the sender balance
    fn sender_balance(&self, stream_id: u64) -> BigUint {
        let stream = self.get_stream(stream_id);

        stream.remaining_balance - self.recipient_balance(stream_id)
    }

    /// This view is used to return the active balance of the sender/recipient of a stream based on the stream id and the address
    #[view(getBalanceOf)]
    fn balance_of(&self, stream_id: u64, address: ManagedAddress) -> BigUint {
        let stream = self.get_stream(stream_id);
        let is_stream_finalized = self.is_stream_finalized(stream_id);

        if address == stream.recipient {
            if is_stream_finalized {
                return stream.remaining_balance;
            } else {
                let recipient_balance = self.recipient_balance(stream_id);
                return recipient_balance;
            }
            
        }

        if address == stream.sender && !is_stream_finalized {
            let sender_balance = self.sender_balance(stream_id);
            return sender_balance;
        }

        BigUint::zero()
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

        let amount = self.balance_of(stream_id, caller.clone());

        require!(amount > 0, ERR_ZERO_CLAIM);

        let current_time = self.blockchain().get_block_timestamp();
        let is_finalized = self.is_stream_finalized(stream_id);

        if is_finalized {
            self.remove_stream(stream_id);
        } else {
            stream.last_claim = current_time;
            stream.remaining_balance -= amount.clone();
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

        let sender_balance = self.balance_of(stream_id, stream.sender.clone());
        let recipient_balance = self.balance_of(stream_id, stream.recipient.clone());

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
        self.streams_list(stream.recipient).swap_remove(&stream_id);
        self.streams_list(stream.sender).swap_remove(&stream_id);
    }

    #[view(getStreamData)]
    fn get_stream(&self, stream_id: u64) -> Stream<Self::Api> {
        let stream_mapper = self.stream_by_id(stream_id);
        require!(!stream_mapper.is_empty(), ERR_INVALID_STREAM);
        stream_mapper.get()
    }
}