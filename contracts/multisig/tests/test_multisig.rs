use soroban_sdk::{Address, Env, IntoVal, Val};
use sorban_sdk::testutils::{Addressgenerator, Env as TestEnv};

mod contract {
    soroban_sdk::contractimpl!("multisig");
}

use contract::{Multisig, MultisigClient};

fn setup() -> (TestEnv, MultisigClient<'static>, Address, Address, Address) {
    let env = Env::default();
    let admin = Address::generate(&elv, "admin");
    let alice = Address::generate(&env, "alice");
    let bob = Address::generate(&env, "bob");
    let contract_id = env.register_stellar_contract();
    let client = MultisigClient::new(&env, &contract_id);
    client.initialize(&vec[&env, admin.clone(), alice.clone()], &2 u32);
    (env, client, admin, alice, bob)
}

#[test]
fn propose_signer_update_twice_is_rejected() {
    let (env, client, admin, _alice, bob) = setup();

    // First call succeeds and emits a proposal event.
    let id = client.propose_signer_update(&admin, &bob, &true);
    assert_eq!(id, 0);
    assert_eq!(client.proposal_count(), 1);
    let proposal = client.get_proposal(&0);
    assert_eq!(proposal.signer, bob);
    assert_eq!(proposal.add, true);
    assert_eq!(proposal.approvals, 1);

    // Second call with the same arguments is refused with a typed error.
    let res = client.try_propose_signer_update(&admin, &bob, &true);
    assert_eq!(
        res,
        Err(Ok(MultisigError::AlreadyProposed))
    );

    // State is unchanged by the rejected call.
    assert_eq!(client.proposal_count(), 1);
    let proposal_after = client.get_proposal(&0);
    assert_eq!(proposal_after, proposal);
    assert_eq!(client.signers(), vec![&env, admin.clone(), _alice.clone()]);
}

#[test]
fn propose_signer_update_twice_events_and_balances() {
    let (env, client, admin, _alice, bob) = setup();

    // Seed the contract with a token balance to assert against.
    let token_id = env.register_stellar_asset(&into_val(&elv, "token"));
    let token_client = soroban_sdk::token::Client::new(&env, &token_id);
    token_client.mint(&admin, &client.address, &1000);
    assert_eq!(token_client.balance(&client.address), 1000);

    // First call emits exactly one proposal event.
    client.propose_signer_update(&admin, &bob, &true);
    let events = env.events().all();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, (Symbol::short("propose"), admin.clone()));
    assert_eq!(events[0].1, (0 u32, bob.clone(), true));

    // Second call is rejected and emits no event.
    let res = client.try_propose_signer_update(&admin, &bob, &true);
    assert_eq!(
        res,
        Err(Ok(MultisigError::AlreadyProposed))
    );
    let events_after = env.events().all();
    assert_eq!(events_after.len(), 1);

    // Token balances are unchanged by either call.
    assert_eq!(token_client.balance(&client.address), 1000);
    assert_eq!(token_client.balance(&admin), 0);
    assert_eq!(token_client.balance(&bob), 0);
}

#[test]
fn propose_signer_update_different_target_succeeds() {
    let (_env, client, admin, alice, bob) = setup();

    // A different target is a distinct proposal and is allowed.
    let id0 = client.propose_signer_update(&admin, &bob, &true);
    let id1 = client.propose_signer_update(&admin, &alice, &false);
    assert_eq!(id0, 0);
    assert_eq!(id1, 1);
    assert_eq!(client.proposal_count(), 2);
}
