#![no_std]

elrond_wasm::imports!();
elrond_wasm::derive_imports!();

pub mod storage;
mod events;
pub mod errors;
use storage::Stream;

use errors::{
    ERR_STREAM_TO_SC,
    ERR_STREAM_TO_CALLER,
    ERR_STREAM_ONLY_FUNGIBLE,
    ERR_ZERO_DEPOSIT,
    ERR_START_TIME,
    ERR_END_TIME,
    ERR_ONLY_RECIPIENT_CLAIM,
    ERR_ZERO_CLAIM,
    ERR_CLAIM_TOO_BIG,
    ERR_CANT_CANCEL,
    ERR_CANCEL_ONLY_OWNERS,
    ERR_INVALID_STREAM,
    ERR_NO_STREAM
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
            last_claim: start_time,
            rate_per_second,
            can_cancel,
            start_time,
            end_time
        };
        self.stream_by_id(stream_id).set(&stream);

        self.streams_list(caller.clone()).insert(stream_id);
        self.streams_list(recipient.clone()).insert(stream_id);

        self.create_stream_event(stream_id, &caller, &recipient, &token_identifier, token_nonce, &token_amount, start_time, end_time);
    }

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

    fn delta_of_sender(&self, stream_id: u64) -> u64 {
        let stream = self.get_stream(stream_id);
        let current_time = self.blockchain().get_block_timestamp();
        if current_time <= stream.start_time {
            return stream.end_time - stream.start_time;
        }
        if current_time < stream.end_time {
            return stream.end_time - current_time;
        }

        0
    }

    fn recipient_balance(&self, stream_id: u64) -> BigUint {
        let stream = self.get_stream(stream_id);
        let delta = self.delta_of_recipient(stream_id);

        let recipient_balance = stream.rate_per_second.mul(delta);

        recipient_balance
    }

    fn sender_balance(&self, stream_id: u64) -> BigUint {
        let stream = self.get_stream(stream_id);
        let delta = self.delta_of_sender(stream_id);

        let recipient_balance = stream.rate_per_second.mul(delta);

        recipient_balance
    }

    #[view(getBalanceOf)]
    fn balance_of(&self, stream_id: u64, address: ManagedAddress) -> BigUint {
        let stream = self.get_stream(stream_id);

        if address == stream.recipient {
            let recipient_balance = self.recipient_balance(stream_id);
            return recipient_balance;
        }

        if address == stream.sender {
            let sender_balance = self.sender_balance(stream_id);
            return sender_balance;
        }

        BigUint::zero()
    }

    #[endpoint(claimFromStream)]
    fn claim_from_stream(
        &self,
        stream_id: u64,
        _amount: OptionalValue<BigUint>
    ) {
        let mut stream = self.get_stream(stream_id);

        let caller = self.blockchain().get_caller();
        require!(caller == stream.recipient, ERR_ONLY_RECIPIENT_CLAIM);

        let balance_of = self.balance_of(stream_id, caller.clone());
        let amount = (_amount.into_option()).unwrap_or(balance_of.clone());

        require!(amount > 0, ERR_ZERO_CLAIM);

        require!(balance_of >= amount, ERR_CLAIM_TOO_BIG);

        let current_time = self.blockchain().get_block_timestamp();
        let is_finalized = current_time >= stream.end_time;

        if is_finalized {
            self.remove_stream(stream_id);
        } else {
            stream.last_claim = current_time;
            self.stream_by_id(stream_id).set(&stream);
        }

        self.send().direct(&caller, &stream.payment_token, stream.payment_nonce, &amount);

        self.claim_from_stream_event(stream_id, &amount, is_finalized);
    }

    #[endpoint(cancelStream)]
    fn cancel_stream(
        &self,
        stream_id: u64
    ) {
        let stream = self.get_stream(stream_id);

        require!(stream.can_cancel, ERR_CANT_CANCEL);

        let caller = self.blockchain().get_caller();
        require!(caller == stream.recipient || caller == stream.sender, ERR_CANCEL_ONLY_OWNERS);

        let sender_balance = self.balance_of(stream_id, stream.sender.clone());
        let recipient_balance = self.balance_of(stream_id, stream.recipient.clone());

        self.remove_stream(stream_id);

        if sender_balance > 0 {
            self.send().direct(&stream.sender, &stream.payment_token, stream.payment_nonce, &sender_balance);
        }

        if recipient_balance > 0 {
            self.send().direct(&stream.recipient, &stream.payment_token, stream.payment_nonce, &recipient_balance);
        }

        self.cancel_stream_event(stream_id, &caller);
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

    #[view(getStreamListWithDetails)]
    fn get_stream_list_with_details(&self,
        address: ManagedAddress,
        page: usize,
        _page_size: OptionalValue<usize>) -> MultiValueEncoded<MultiValue2<u64, Stream<Self::Api>>> {
        let streams_list_by_address = self.streams_list(address);
        require!(!streams_list_by_address.is_empty(), ERR_NO_STREAM);
        require!(streams_list_by_address.len() > 0, ERR_NO_STREAM);
        let page_size: usize = (&_page_size.into_option()).unwrap_or(100);
        let mut streams_list_by_address_iter = streams_list_by_address.iter().skip(page * page_size).take(page_size);

        let mut result = MultiValueEncoded::new();

        while let Some(stream_id) = streams_list_by_address_iter.next() {
            let stream = self.get_stream(stream_id);
            result.push(MultiValue2::from((stream_id, stream)));
        }

        result
    }
}