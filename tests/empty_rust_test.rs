use elrond_wasm::types::{Address, ManagedAddress};
use elrond_wasm_debug::{rust_biguint, testing_framework::*, DebugApi, managed_address};
use staking::*;
use std::time::{SystemTime, UNIX_EPOCH};

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
    let owner_address = blockchain_wrapper.create_user_account(&rust_zero);
    blockchain_wrapper.set_esdt_balance(&owner_address, TOKEN_ID, &rust_biguint!(5_000_000));
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

fn get_current_timestamp() -> u64 {
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
    let first_user = setup.first_user_address;

    // create a valid stream
    b_wrapper
        .execute_esdt_transfer(
            &setup.owner_address,
            &setup.contract_wrapper,
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
}