#![cfg(test)]

use soroban_sdk::{testutils::Ledger, Env};
use soroban_sdk::{Address,testutils::Address as TestAddress};
use crate::payroll::PayrollContract;
#[test]
fn test() {
    let env = Env::default();
    env.ledger().with_mut(|li| {
        li.protocol_version = 20;
    });
}
#[test]
fn test_modification_timeout_cleanup(){
    let env=Env::default();
    env.ledger().with_mut(|li|{
        li.protocol_version=20;
        li.timestamp=1000;
    });
    env.ledger().with_mut(|li|{
        li.timestamp=2000;
    });
    let caller:Address=<Address as TestAddress>::generate(&env);
    PayrollContract::cleanup_timed_out_modification(env.clone(),caller);

}
