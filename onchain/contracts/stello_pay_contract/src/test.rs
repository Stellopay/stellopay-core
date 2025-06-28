#![cfg(test)]

use crate::payroll::{PayrollContract, PayrollContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

#[test]
fn test() {
    let env = Env::default();
    env.ledger().with_mut(|li| {
        li.protocol_version = 20;
    });
}
