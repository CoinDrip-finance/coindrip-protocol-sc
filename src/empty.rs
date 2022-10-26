#![no_std]

elrond_wasm::imports!();
elrond_wasm::derive_imports!();
#[derive(TopEncode, TopDecode, NestedEncode, TypeAbi)]
pub struct Stream<M: ManagedTypeApi> {
    pub sender: ManagedAddress<M>,
    pub recipient: ManagedAddress<M>,
    pub payment_token: EgldOrEsdtTokenIdentifier<M>,
    pub payment_nonce: u64,
    pub deposit: BigUint<M>,
    pub remaining_balance: BigUint<M>,
    pub rate_per_second: BigUint<M>,

    pub start_time: u64,
    pub end_time: u64
}

#[elrond_wasm::contract]
pub trait EmptyContract {
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
        end_time: u64
    ) {
        require!(recipient != self.blockchain().get_sc_address(), "stream to the current smart contract");
        require!(recipient != self.blockchain().get_caller(), "stream to the caller");

        let (token_identifier, token_nonce, token_amount) = self.call_value().egld_or_single_esdt().into_tuple();

        require!(token_nonce == 0, "you can only stream fungible tokens");

        require!(token_amount > BigUint::zero(), "deposit is zero");

        let current_time = self.blockchain().get_block_timestamp();
        require!(start_time >= current_time, "start time before current time");
        require!(end_time > start_time, "end time before the start time");

        let stream_id = self.last_stream_id().get() + 1;
        self.last_stream_id().set(&stream_id);

        let duration = end_time - start_time;
        let rate_per_second = token_amount.clone() / BigUint::from(duration);

        let caller = self.blockchain().get_caller();

        let stream = Stream {
            sender: caller.clone(),
            recipient: recipient.clone(),
            payment_token: token_identifier,
            payment_nonce: token_nonce,
            deposit: token_amount.clone(),
            remaining_balance: token_amount.clone(),
            rate_per_second,
            start_time,
            end_time
        };
        self.stream_by_id(stream_id).set(&stream);

        self.streams_list(caller).insert(stream_id);
        self.streams_list(recipient).insert(stream_id);
    }

    fn delta_of(&self, stream_id: u64) -> u64 {
        let stream = self.get_stream(stream_id);
        let current_time = self.blockchain().get_block_timestamp();
        if current_time <= stream.start_time {
            return 0;
        }
        if current_time < stream.end_time {
            return current_time - stream.start_time;
        }

        return stream.end_time - stream.start_time;
    }

    #[view(streamedSoFar)]
    fn streamed_so_far(&self, stream_id: u64) -> BigUint {
        let stream = self.get_stream(stream_id);
        let delta = self.delta_of(stream_id);

        let mut recipient_balance;
        if delta == stream.end_time - stream.start_time {
            recipient_balance = stream.remaining_balance.clone();
        } else {
            recipient_balance = stream.rate_per_second * BigUint::from(delta);

            if stream.deposit > stream.remaining_balance {
                let claimed_amount = stream.deposit - stream.remaining_balance.clone();
                recipient_balance = recipient_balance - claimed_amount;
            }
        }

        return recipient_balance;
    }

    #[view(getBalanceOf)]
    fn balance_of(&self, stream_id: u64, address: ManagedAddress) -> BigUint {
        let stream = self.get_stream(stream_id);

        let recipient_balance = self.streamed_so_far(stream_id);
        
        if address == stream.recipient {
            return recipient_balance;
        }

        if address == stream.sender {
            let sender_balance = stream.remaining_balance - recipient_balance;
            return sender_balance;
        }

        return BigUint::zero();
    }

    #[endpoint(claimFromStream)]
    fn claim_from_stream(
        &self,
        stream_id: u64,
        _amount: OptionalValue<BigUint>
    ) {
        let mut stream = self.get_stream(stream_id);

        let caller = self.blockchain().get_caller();
        require!(caller == stream.recipient, "Only recipient can claim");

        let balance_of = self.balance_of(stream_id, caller.clone());
        let amount = (_amount.into_option()).unwrap_or(balance_of.clone());

        require!(amount > BigUint::zero(), "amount is zero");

        require!(balance_of >= amount, "amount exceeds the available balance");

        let remaining_balance = stream.remaining_balance - amount.clone();

        if remaining_balance == BigUint::zero() {
            self.remove_stream(stream_id);
        } else {
            stream.remaining_balance = remaining_balance;
            self.stream_by_id(stream_id).set(&stream);
        }

        self.send().direct(&caller, &stream.payment_token, stream.payment_nonce, &amount);
    }

    #[endpoint(cancelStream)]
    fn cancel_stream(
        &self,
        stream_id: u64
    ) {
        let stream = self.get_stream(stream_id);

        let caller = self.blockchain().get_caller();
        require!(caller == stream.recipient || caller == stream.sender, "Only recipient or sender can cancel stream");

        let sender_balance = self.balance_of(stream_id, stream.sender.clone());
        let recipient_balance = self.balance_of(stream_id, stream.recipient.clone());

        self.remove_stream(stream_id);

        if sender_balance > BigUint::zero() {
            self.send().direct(&stream.sender, &stream.payment_token, stream.payment_nonce, &sender_balance);
        }

        if recipient_balance > BigUint::zero() {
            self.send().direct(&stream.recipient, &stream.payment_token, stream.payment_nonce, &recipient_balance);
        }
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
        require!(!stream_mapper.is_empty(), "Stream does not exist");
        stream_mapper.get()
    }
    
    #[view(getStreamListWithDetails)]
    fn get_stream_list_with_details(&self, 
        address: ManagedAddress,
        page: usize,
        _page_size: OptionalValue<usize>) -> MultiValueEncoded<MultiValue2<u64, Stream<Self::Api>>> {
        let streams_list_by_address = self.streams_list(address);
        require!(!streams_list_by_address.is_empty(), "Address have no streams");
        require!(streams_list_by_address.len() > 0, "Address have no streams");
        let page_size: usize = (&_page_size.into_option()).unwrap_or(100);
        let mut streams_list_by_address_iter = streams_list_by_address.iter().skip(page * page_size).take(page_size);

        let mut result = MultiValueEncoded::new();

        while let Some(stream_id) = streams_list_by_address_iter.next() {
            let stream = self.get_stream(stream_id);
            result.push(MultiValue2::from((stream_id, stream)));
        }

        return result;
    }

    #[storage_mapper("streamById")]
    fn stream_by_id(&self, stream_id: u64) -> SingleValueMapper<Stream<Self::Api>>;

    #[view(getStreamListByAddress)]
    #[storage_mapper("streamsList")]
    fn streams_list(&self, address: ManagedAddress) -> UnorderedSetMapper<u64>;

    #[view(getLastStreamId)]
    #[storage_mapper("lastStreamId")]
    fn last_stream_id(&self) -> SingleValueMapper<u64>;
}
