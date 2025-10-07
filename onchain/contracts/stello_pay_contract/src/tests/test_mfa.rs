#![cfg(test)]

extern crate alloc;

use crate::payroll::{PayrollContract, PayrollContractClient};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger, LedgerInfo},
    Address, Bytes, BytesN, Env, Vec,
};

type HmacSha1 = Hmac<Sha1>;

fn build_secret(env: &Env, raw: &[u8]) -> Bytes {
    Bytes::from_slice(env, raw)
}

fn hotp(secret: &[u8], counter: u64, digits: u32) -> u32 {
    let mut mac = HmacSha1::new_from_slice(secret).expect("valid secret");
    mac.update(&counter.to_be_bytes());
    let result = mac.finalize().into_bytes();
    let offset = (result[result.len() - 1] & 0x0f) as usize;
    let binary = ((result[offset] & 0x7f) as u32) << 24
        | (result[offset + 1] as u32) << 16
        | (result[offset + 2] as u32) << 8
        | (result[offset + 3] as u32);
    let modulus = 10u32.pow(digits);
    binary % modulus
}

fn current_totp(secret: &[u8], digits: u32, period: u64, timestamp: u64) -> u32 {
    let counter = timestamp / period;
    hotp(secret, counter, digits)
}

fn set_timestamp(env: &Env, timestamp: u64) {
    env.ledger().set(LedgerInfo {
        timestamp,
        protocol_version: 22,
        sequence_number: env.ledger().sequence(),
        network_id: Default::default(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6_312_000,
    });
}

#[test]
#[should_panic(expected = "Error(Contract, #44)")]
fn payroll_creation_requires_mfa_session() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    client.initialize(&owner);

    let secret = build_secret(&env, b"seed_for_payroll");
    let digits = 6u32;
    let period = 30u64;
    let emergency_codes = Vec::<BytesN<32>>::new(&env);

    client.enable_mfa(
        &owner,
        &secret,
        &digits,
        &period,
        &emergency_codes,
        &false,
        &None,
    );

    client.create_or_update_escrow(
        &owner,
        &employee,
        &token,
        &1_000i128,
        &86_400u64,
        &2_592_000u64,
    );
}

#[test]
fn totp_session_allows_transfer() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    let new_owner = Address::generate(&env);

    client.initialize(&owner);

    let raw_secret = b"12345678901234567890";
    let secret = build_secret(&env, raw_secret);
    let digits = 6u32;
    let period = 30u64;
    let emergency_codes = Vec::<BytesN<32>>::new(&env);

    client.enable_mfa(
        &owner,
        &secret,
        &digits,
        &period,
        &emergency_codes,
        &false,
        &None,
    );

    set_timestamp(&env, 1_700_000_000);
    let challenge_id = client.begin_mfa_challenge(&owner, &symbol_short!("transfr"), &false);
    let totp = Some(current_totp(
        raw_secret,
        digits,
        period,
        env.ledger().timestamp(),
    ));
    let no_emergency: Option<BytesN<32>> = None;
    client.complete_mfa_challenge(&owner, &challenge_id, &totp, &no_emergency);

    client.transfer_ownership(&owner, &new_owner);
    assert_eq!(client.get_owner(), Some(new_owner));
}

#[test]
#[should_panic(expected = "Error(Contract, #44)")]
fn transfer_without_session_is_blocked() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    let new_owner = Address::generate(&env);

    client.initialize(&owner);

    let secret = build_secret(&env, b"seed_for_transfer");
    let digits = 6u32;
    let period = 30u64;
    let emergency_codes = Vec::<BytesN<32>>::new(&env);

    client.enable_mfa(
        &owner,
        &secret,
        &digits,
        &period,
        &emergency_codes,
        &false,
        &None,
    );

    client.transfer_ownership(&owner, &new_owner);
}

#[test]
#[should_panic(expected = "Error(Contract, #44)")]
fn session_expiration_blocks_operation() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    client.initialize(&owner);

    let raw_secret = b"ABCDEFGHIJKLMNOPQRST";
    let secret = build_secret(&env, raw_secret);
    let digits = 6u32;
    let period = 30u64;
    let emergency_codes = Vec::<BytesN<32>>::new(&env);
    let timeout_override = Some(60u64);

    client.enable_mfa(
        &owner,
        &secret,
        &digits,
        &period,
        &emergency_codes,
        &false,
        &timeout_override,
    );

    set_timestamp(&env, 1_800_000_000);
    let challenge_id = client.begin_mfa_challenge(&owner, &symbol_short!("payroll"), &false);
    let totp = Some(current_totp(
        raw_secret,
        digits,
        period,
        env.ledger().timestamp(),
    ));
    let no_emergency: Option<BytesN<32>> = None;
    client.complete_mfa_challenge(&owner, &challenge_id, &totp, &no_emergency);

    // Advance beyond session timeout window
    set_timestamp(&env, 1_800_000_000 + 180);

    client.create_or_update_escrow(
        &owner,
        &employee,
        &token,
        &1_500i128,
        &54_000u64,
        &1_296_000u64,
    );
}

#[test]
fn emergency_bypass_allows_transfer() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    let new_owner = Address::generate(&env);

    client.initialize(&owner);

    let secret = build_secret(&env, b"FEDCBA0987654321LMNO");
    let digits = 6u32;
    let period = 30u64;
    let mut emergency_codes = Vec::<BytesN<32>>::new(&env);
    let emergency_hash = BytesN::from_array(&env, &[7u8; 32]);
    emergency_codes.push_back(emergency_hash.clone());

    client.enable_mfa(
        &owner,
        &secret,
        &digits,
        &period,
        &emergency_codes,
        &true,
        &None,
    );

    set_timestamp(&env, 1_900_000_000);
    let challenge_id = client.begin_mfa_challenge(&owner, &symbol_short!("transfr"), &true);
    let emergency_opt = Some(emergency_hash);
    let totp_none: Option<u32> = None;
    client.complete_mfa_challenge(&owner, &challenge_id, &totp_none, &emergency_opt);

    client.transfer_ownership(&owner, &new_owner);
    assert_eq!(client.get_owner(), Some(new_owner));
}
