use elrond_wasm::types::{Address, BigUint};
use elrond_wasm_debug::{rust_biguint, testing_framework::*, DebugApi, managed_address};
use staking::*;
use std::time::{SystemTime, UNIX_EPOCH};
use elrond_wasm::{
    elrond_codec::multi_types::{OptionalValue},
};

const WASM_PATH: &'static str = "output/staking.wasm";
pub const TOKEN_ID: &[u8] = b"STRM-df6f26";

struct ContractSetup<ContractObjBuilder>
where
    ContractObjBuilder: 'static + Copy + Fn() -> staking::ContractObj<DebugApi>,
{
    pub blockchain_wrapper: BlockchainStateWrapper,
    pub owner_address: Address,
    pub contract_wrapper: ContractObjWrapper<staking::ContractObj<DebugApi>, ContractObjBuilder>,
    pub first_user_address: Address,
    pub second_user_address: Address,
    pub third_user_address: Address,
}

fn setup_contract<ContractObjBuilder>(
    cf_builder: ContractObjBuilder,
) -> ContractSetup<ContractObjBuilder>
where
    ContractObjBuilder: 'static + Copy + Fn() -> staking::ContractObj<DebugApi>,
{
    let rust_zero = rust_biguint!(0u64);
    let mut blockchain_wrapper = BlockchainStateWrapper::new();

    // Create a wallet for SC and assign 5M tokens
    let owner_address = blockchain_wrapper.create_user_account(&rust_zero);
    blockchain_wrapper.set_esdt_balance(&owner_address, TOKEN_ID, &rust_biguint!(5_000_000));

    // Create 3 dummy wallets to interact with the protocol
    let first_user_address = blockchain_wrapper.create_user_account(&rust_zero);
    let second_user_address = blockchain_wrapper.create_user_account(&rust_zero);
    let third_user_address = blockchain_wrapper.create_user_account(&rust_zero);
    
    let cf_wrapper = blockchain_wrapper.create_sc_account(
        &rust_zero,
        Some(&owner_address),
        cf_builder,
        WASM_PATH,
    );

    blockchain_wrapper
        .execute_tx(&owner_address, &cf_wrapper, &rust_zero, |sc| {
            sc.init();
        })
        .assert_ok();

    blockchain_wrapper.add_mandos_set_account(cf_wrapper.address_ref());

    ContractSetup {
        blockchain_wrapper,
        owner_address,
        contract_wrapper: cf_wrapper,
        first_user_address,
        second_user_address,
        third_user_address,
    }
}

#[test]
fn deploy_test() {
    let mut setup = setup_contract(staking::contract_obj);

    // simulate deploy
    setup
        .blockchain_wrapper
        .execute_tx(
            &setup.owner_address,
            &setup.contract_wrapper,
            &rust_biguint!(0u64),
            |sc| {
                sc.init();
            },
        )
        .assert_ok();
}

/**
 * Utility function to get current timestamp
 */
fn get_current_timestamp() -> u64 {
    return 1668518731;
    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    return since_the_epoch.as_secs();
}

#[test]
fn create_stream_test() {
    let mut setup = setup_contract(staking::contract_obj);
    let b_wrapper = &mut setup.blockchain_wrapper;
    let current_timestamp = get_current_timestamp();
    b_wrapper.set_block_timestamp(current_timestamp);
    let c_wrapper = &mut setup.contract_wrapper;
    let first_user = setup.first_user_address;
    let owner_address  = setup.owner_address;
    

    // Create a valid stream of 3K tokens
    b_wrapper
        .execute_esdt_transfer(
            &owner_address,
            c_wrapper,
            TOKEN_ID,
            0, 
            &rust_biguint!(3_000),
            |sc| {
                let current_timestamp = get_current_timestamp();
                 sc.create_stream(managed_address!(&first_user), current_timestamp + 60, current_timestamp + 60 * 60);

                let user_deposit = sc.streams_list(managed_address!(&first_user));
                let expected_deposit = user_deposit.len();
                assert_eq!(expected_deposit, 1);
            },
        )
        .assert_ok();

    // Create an invalid stream of 0 tokens
    b_wrapper
    .execute_esdt_transfer(
        &owner_address,
        c_wrapper,
        TOKEN_ID,
        0, 
        &rust_biguint!(0),
        |sc| {
            let current_timestamp = get_current_timestamp();
             sc.create_stream(managed_address!(&first_user), current_timestamp + 60, current_timestamp + 60 * 60);
        },
    )
    .assert_user_error("deposit is zero");

    // Stream towards the SC
    b_wrapper
    .execute_esdt_transfer(
        &owner_address,
        c_wrapper,
        TOKEN_ID,
        0, 
        &rust_biguint!(3_000),
        |sc| {
            let current_timestamp = get_current_timestamp();
            sc.create_stream(managed_address!(c_wrapper.address_ref()), current_timestamp + 60, current_timestamp + 60 * 60);
        },
    )
    .assert_user_error("stream to the current smart contract");

    // Stream towards the caller
    b_wrapper
    .execute_esdt_transfer(
        &owner_address,
        c_wrapper,
        TOKEN_ID,
        0, 
        &rust_biguint!(3_000),
        |sc| {
            let current_timestamp = get_current_timestamp();
            sc.create_stream(managed_address!(&owner_address), current_timestamp + 60, current_timestamp + 60 * 60);
        },
    )
    .assert_user_error("stream to the caller");

    // Start time before current time
    b_wrapper
    .execute_esdt_transfer(
        &owner_address,
        c_wrapper,
        TOKEN_ID,
        0, 
        &rust_biguint!(3_000),
        |sc| {
            let current_timestamp = get_current_timestamp();
            sc.create_stream(managed_address!(&first_user), current_timestamp - 60, current_timestamp + 60 * 60);
        },
    )
    .assert_user_error("start time before current time");

     // End time before start time
     b_wrapper
     .execute_esdt_transfer(
         &owner_address,
         c_wrapper,
         TOKEN_ID,
         0, 
         &rust_biguint!(3_000),
         |sc| {
             let current_timestamp = get_current_timestamp();
             sc.create_stream(managed_address!(&first_user), current_timestamp + 60 * 60, current_timestamp + 60);
         },
     )
     .assert_user_error("end time before the start time");
}

#[test]
fn claim_from_stream_test() {
    let mut setup = setup_contract(staking::contract_obj);
    let b_wrapper = &mut setup.blockchain_wrapper;
    let current_timestamp = get_current_timestamp();
    b_wrapper.set_block_timestamp(current_timestamp);
    let c_wrapper = &mut setup.contract_wrapper;
    let first_user = setup.first_user_address;
    let owner_address  = setup.owner_address;
    

    // Create a valid stream of 3K tokens
    b_wrapper
        .execute_esdt_transfer(
            &owner_address,
            c_wrapper,
            TOKEN_ID,
            0, 
            &rust_biguint!(3_000),
            |sc| {
                let current_timestamp = get_current_timestamp();
                 sc.create_stream(managed_address!(&first_user), current_timestamp + 60, current_timestamp + 60 * 3);
            },
        ).assert_ok();
        

        // Claim from stream wrong recipient
        b_wrapper
        .execute_tx(
            &owner_address,
            c_wrapper,
            &rust_biguint!(0), 
            |sc| {
                sc.claim_from_stream(1, OptionalValue::None);
            },
        )
        .assert_user_error("Only recipient can claim");

          // Amount to claim is zero
          b_wrapper
          .execute_tx(
              &first_user,
              c_wrapper,
              &rust_biguint!(0), 
              |sc| {
                  sc.claim_from_stream(1, OptionalValue::None);
              },
          )
          .assert_user_error("amount is zero");

          // Amount is bigger than streamed amount
          b_wrapper
          .execute_tx(
              &first_user,
              c_wrapper,
              &rust_biguint!(0), 
              |sc| {
                  sc.claim_from_stream(1, OptionalValue::Some(BigUint::from(100u64)));
              },
          )
          .assert_user_error("amount exceeds the available balance");

          b_wrapper.set_block_timestamp(current_timestamp + 60 * 2);

          // Claim 1.5K tokens
          b_wrapper
          .execute_tx(
              &first_user,
              c_wrapper,
              &rust_biguint!(0), 
              |sc| {
                  sc.claim_from_stream(1, OptionalValue::None);
              },
          )
          .assert_ok();

          b_wrapper.check_esdt_balance(&first_user, TOKEN_ID, &rust_biguint!(1500));

          b_wrapper.set_block_timestamp(current_timestamp + 60 * 5);

          // Claim rest of the 1.5K tokens
          b_wrapper
          .execute_tx(
              &first_user,
              c_wrapper,
              &rust_biguint!(0), 
              |sc| {
                  sc.claim_from_stream(1, OptionalValue::None);
              },
          )
          .assert_ok();

          b_wrapper.check_esdt_balance(&first_user, TOKEN_ID, &rust_biguint!(3000));

}