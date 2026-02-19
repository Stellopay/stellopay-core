use soroban_sdk::{contractevent, Address, Env};

/// Event: Payment recorded
#[contractevent]
#[derive(Clone, Debug)]
pub struct PaymentRecorded {
    pub agreement_id: u128,
    pub token: Address,
    pub amount: i128,
    pub from: Address,
    pub to: Address,
    pub timestamp: u64,
}

pub fn emit_payment_recorded(e: &Env, event: PaymentRecorded) {
    event.publish(e);
}
