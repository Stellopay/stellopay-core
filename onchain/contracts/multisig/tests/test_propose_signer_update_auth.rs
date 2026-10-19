#![cfg(test)]

//! Authorisation-path coverage for `propose_signer_update` (issue #1351).
//!
//! `propose_signer_update` is a public entry point that forwards to
//! `propose_update_signers`. These tests pin its authorisation behaviour so a
//! refactor cannot silently drop the `owner.require_auth()` check:
//!
//! 1. A caller without authorisation is refused, and the refusal surfaces as
//!    the host's auth error (`Context` / `InvalidAction`) rather than "any"
//!    error — a validation panic or a `MultisigError` variant would mean the
//!    refusal came from the wrong place.
//! 2. The refused call mutates no state, whether the contract holds no
//!    proposal or a pending one.
//! 3. The authorised caller still succeeds exactly as before, timelock
//!    included.

use multisig::{MultisigContract, MultisigContractClient, SignerChangeProposal};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    xdr::{ScErrorCode, ScErrorType},
    Address, Env, Error, Vec,
};

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_1of1(env: &Env) -> (MultisigContractClient<'static>, Address, Vec<Address>) {
    #[allow(deprecated)]
    let id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(env, &id);

    let owner = Address::generate(env);
    let signer = Address::generate(env);
    let mut signers = Vec::new(env);
    signers.push_back(signer);

    client.initialize(&owner, &signers, &1u32, &None);
    (client, owner, signers)
}

fn new_signer_set(env: &Env) -> Vec<Address> {
    let mut signers = Vec::new(env);
    signers.push_back(Address::generate(env));
    signers
}

// ==================== propose_signer_update authorisation (issue #1351) ====================

/// A caller without authorisation must be refused by the auth check itself —
/// the error is the host's auth error, not a `MultisigError` and not a
/// validation panic — and the refusal leaves no trace in storage.
#[test]
fn propose_signer_update_refuses_unauthorised_caller() {
    let env = create_env();
    let (client, _owner, signers) = setup_1of1(&env);
    let threshold_before = client.get_threshold();

    // Strip all authorisations so no address — owner included — may pass
    // `require_auth`.
    env.mock_auths(&[]);
    let res = client.try_propose_signer_update(&new_signer_set(&env), &1u32);

    let host_err = res
        .expect_err("an unauthorised propose_signer_update must be refused")
        .expect("the refusal must surface as a host error, not a VM abort");
    assert_eq!(
        host_err,
        Error::from_type_and_code(ScErrorType::Context, ScErrorCode::InvalidAction),
        "the refusal must be the host auth error raised by owner.require_auth(), \
         not a MultisigError variant or a validation panic",
    );

    // No state was written by the refused call.
    assert_eq!(client.get_signer_change_proposal(), None);
    assert_eq!(client.get_signer_update_proposal(), None);
    assert_eq!(client.get_signers(), signers);
    assert_eq!(client.get_threshold(), threshold_before);

    // The authorised caller is still able to propose afterwards — the refusal
    // did not poison the entry point.
    env.mock_all_auths();
    client.propose_signer_update(&new_signer_set(&env), &1u32);
    assert!(
        client.get_signer_change_proposal().is_some(),
        "the authorised caller must still succeed after a refused attempt"
    );
}

/// A refused call must not clobber a proposal that an authorised caller
/// already has in flight.
#[test]
fn propose_signer_update_refused_call_does_not_clobber_pending_proposal() {
    let env = create_env();
    let (client, _owner, signers) = setup_1of1(&env);
    let threshold_before = client.get_threshold();

    // An authorised proposal is already in flight.
    let pending = new_signer_set(&env);
    client.propose_signer_update(&pending, &1u32);
    let recorded: SignerChangeProposal = client
        .get_signer_change_proposal()
        .expect("the authorised proposal must be stored");

    // An unauthorised caller proposes a *different* signer set — refused.
    env.mock_auths(&[]);
    let res = client.try_propose_signer_update(&new_signer_set(&env), &1u32);
    let host_err = res
        .expect_err("an unauthorised propose_signer_update must be refused")
        .expect("the refusal must surface as a host error, not a VM abort");
    assert_eq!(
        host_err,
        Error::from_type_and_code(ScErrorType::Context, ScErrorCode::InvalidAction),
        "the refusal must be the host auth error raised by owner.require_auth()",
    );

    // The pending proposal is exactly what the authorised caller left behind.
    assert_eq!(client.get_signer_change_proposal(), Some(recorded.clone()));
    assert_eq!(client.get_signer_update_proposal(), Some(recorded));
    assert_eq!(client.get_signers(), signers);
    assert_eq!(client.get_threshold(), threshold_before);
}

/// The authorised caller still succeeds, and the alias behaves exactly like
/// `propose_update_signers`: the proposal is stored behind the timelock and
/// only takes effect once the delay has elapsed.
#[test]
fn propose_signer_update_authorised_caller_succeeds_unchanged() {
    let env = create_env();
    let (client, _owner, signers) = setup_1of1(&env);

    let now = env.ledger().timestamp();
    let delay = client.get_signer_change_delay();
    let new_signers = new_signer_set(&env);

    client.propose_signer_update(&new_signers, &1u32);

    let proposal = client
        .get_signer_change_proposal()
        .expect("the authorised proposal must be stored");
    assert_eq!(proposal.new_signers, new_signers);
    assert_eq!(proposal.new_threshold, 1u32);
    assert_eq!(proposal.proposed_at, now);
    assert_eq!(proposal.activation_timestamp, now + delay);
    assert_eq!(client.get_signer_update_proposal(), Some(proposal));

    // The proposal is timelocked: the signer set on chain is unchanged and
    // differs from the proposed one.
    assert_eq!(client.get_signers(), signers);
    assert_ne!(client.get_signers(), new_signers);

    // Advance past the timelock and execute through the alias — the authorised
    // path is unchanged end to end.
    env.ledger().with_mut(|li| {
        li.timestamp += delay;
    });
    client.execute_signer_update();
    assert_eq!(client.get_signers(), new_signers);
    assert_eq!(client.get_threshold(), 1u32);
    assert_eq!(client.get_signer_change_proposal(), None);
}
