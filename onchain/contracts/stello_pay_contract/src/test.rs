#![cfg(test)]

use soroban_sdk::{
    testutils::Ledger,
    Env,
};

#[test]
fn test() {
    let env = Env::default();
    env.ledger().with_mut(|li| {
        li.protocol_version = 20;
    });
}
