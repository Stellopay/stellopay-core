#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, Vec,
};

#[contract]
pub struct PaymentRetryContract;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaymentStatus {
    Pending,
    Completed,
    Failed,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentRequest {
    pub id: u128,
    pub payer: Address,
    pub recipient: Address,
    pub token: Address,
    pub amount: i128,
    pub created_at: u64,
    pub next_retry_at: u64,
    pub retry_count: u32,
    pub max_retry_attempts: u32,
    pub retry_intervals: Vec<u64>,
    pub failure_notifier: Address,
    pub status: PaymentStatus,
}

#[contracttype]
#[derive(Clone)]
enum StorageKey {
    Initialized,
    Owner,
    NextPaymentId,
    Payment(u128),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentCreatedEvent {
    pub payment_id: u128,
    pub payer: Address,
    pub recipient: Address,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetryScheduledEvent {
    pub payment_id: u128,
    pub retry_count: u32,
    pub next_retry_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentSucceededEvent {
    pub payment_id: u128,
    pub recipient: Address,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentFailedEvent {
    pub payment_id: u128,
    pub retry_count: u32,
    pub max_retry_attempts: u32,
    pub notifier: Address,
}

const MAX_RETRY_ATTEMPTS: u32 = 100;
const MAX_RETRY_INTERVALS: u32 = 100;
const MAX_SINGLE_RETRY_INTERVAL_SECONDS: u64 = 31_536_000;

fn require_initialized(env: &Env) {
    let initialized = env
        .storage()
        .persistent()
        .get::<_, bool>(&StorageKey::Initialized)
        .unwrap_or(false);
    assert!(initialized, "Contract not initialized");
}

fn read_payment(env: &Env, payment_id: u128) -> PaymentRequest {
    env.storage()
        .persistent()
        .get::<_, PaymentRequest>(&StorageKey::Payment(payment_id))
        .expect("Payment not found")
}

fn write_payment(env: &Env, payment: &PaymentRequest) {
    env.storage()
        .persistent()
        .set(&StorageKey::Payment(payment.id), payment);
}

fn next_payment_id(env: &Env) -> u128 {
    let current = env
        .storage()
        .persistent()
        .get::<_, u128>(&StorageKey::NextPaymentId)
        .unwrap_or(0);
    let next = current.checked_add(1).expect("Payment id overflow");
    env.storage()
        .persistent()
        .set(&StorageKey::NextPaymentId, &next);
    next
}

fn validate_retry_configuration(max_retry_attempts: u32, retry_intervals: &Vec<u64>) {
    assert!(
        max_retry_attempts <= MAX_RETRY_ATTEMPTS,
        "Too many retry attempts"
    );
    assert!(
        retry_intervals.len() <= MAX_RETRY_INTERVALS,
        "Too many retry intervals"
    );

    if max_retry_attempts > 0 {
        assert!(
            retry_intervals.len() > 0,
            "Retry intervals required when retries are enabled"
        );
    }

    let mut i: u32 = 0;
    while i < retry_intervals.len() {
        let interval = retry_intervals
            .get(i)
            .expect("Retry interval missing");
        assert!(interval > 0, "Retry interval must be positive");
        assert!(
            interval <= MAX_SINGLE_RETRY_INTERVAL_SECONDS,
            "Retry interval too large"
        );
        i = i.saturating_add(1);
    }
}

fn interval_for_retry(retry_intervals: &Vec<u64>, retry_count: u32) -> u64 {
    if retry_intervals.is_empty() {
        return 0;
    }

    let mut index = retry_count.saturating_sub(1);
    let max_index = retry_intervals.len().saturating_sub(1);
    if index > max_index {
        index = max_index;
    }

    retry_intervals
        .get(index)
        .expect("Retry interval missing")
}

#[contractimpl]
impl PaymentRetryContract {
    /// @notice Initializes the payment retry contract.
    /// @dev Can only be executed once.
    /// @param owner Administrative owner address.
    pub fn initialize(env: Env, owner: Address) {
        owner.require_auth();

        let initialized = env
            .storage()
            .persistent()
            .get::<_, bool>(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(!initialized, "Contract already initialized");

        env.storage().persistent().set(&StorageKey::Owner, &owner);
        env.storage()
            .persistent()
            .set(&StorageKey::Initialized, &true);
    }

    /// @notice Creates a payment request with retry policy.
    /// @dev The payment is attempted by `process_due_payments` and retried automatically
    ///      when escrow balance is insufficient.
    /// @param payer Original payer authorized to fund/cancel the request.
    /// @param recipient Destination recipient.
    /// @param token Token contract used for settlement.
    /// @param amount Amount to transfer on success.
    /// @param max_retry_attempts Maximum number of failed retries before terminal failure.
    /// @param retry_intervals Retry delays in seconds; when shorter than attempts, the last value is reused.
    /// @param failure_notifier Address included in terminal failure notifications.
    /// @return payment_id Newly created payment request id.
    pub fn create_payment_request(
        env: Env,
        payer: Address,
        recipient: Address,
        token: Address,
        amount: i128,
        max_retry_attempts: u32,
        retry_intervals: Vec<u64>,
        failure_notifier: Address,
    ) -> u128 {
        require_initialized(&env);
        payer.require_auth();
        assert!(amount > 0, "Amount must be positive");
        validate_retry_configuration(max_retry_attempts, &retry_intervals);

        let payment_id = next_payment_id(&env);
        let now = env.ledger().timestamp();

        let payment = PaymentRequest {
            id: payment_id,
            payer: payer.clone(),
            recipient: recipient.clone(),
            token,
            amount,
            created_at: now,
            next_retry_at: now,
            retry_count: 0,
            max_retry_attempts,
            retry_intervals,
            failure_notifier,
            status: PaymentStatus::Pending,
        };

        write_payment(&env, &payment);

        env.events().publish(
            ("payment_created", payment_id),
            PaymentCreatedEvent {
                payment_id,
                payer,
                recipient,
                amount,
            },
        );

        payment_id
    }

    /// @notice Funds escrow balance for a payment request.
    /// @dev Tokens are transferred from payer into this contract.
    /// @param payer Payer that owns the request and approves the transfer.
    /// @param payment_id Request id.
    /// @param amount Funding amount.
    pub fn fund_payment(env: Env, payer: Address, payment_id: u128, amount: i128) {
        require_initialized(&env);
        payer.require_auth();
        assert!(amount > 0, "Amount must be positive");

        let payment = read_payment(&env, payment_id);
        assert!(payment.payer == payer, "Only payer can fund payment");
        assert!(payment.status == PaymentStatus::Pending, "Payment is not pending");

        let token_client = token::Client::new(&env, &payment.token);
        token_client.transfer(&payer, &env.current_contract_address(), &amount);
    }

    /// @notice Attempts and retries due payments.
    /// @dev Anyone can call this entrypoint to execute due jobs in a cron-like pattern.
    /// @param max_payments Maximum number of due payments processed in this call.
    /// @return processed Number of payment requests processed.
    pub fn process_due_payments(env: Env, max_payments: u32) -> u32 {
        require_initialized(&env);

        if max_payments == 0 {
            return 0;
        }

        let now = env.ledger().timestamp();
        let mut processed = 0u32;

        let highest_id = env
            .storage()
            .persistent()
            .get::<_, u128>(&StorageKey::NextPaymentId)
            .unwrap_or(0);

        if highest_id == 0 {
            return 0;
        }

        let mut payment_id = 1u128;
        while payment_id <= highest_id && processed < max_payments {
            if let Some(mut payment) = env
                .storage()
                .persistent()
                .get::<_, PaymentRequest>(&StorageKey::Payment(payment_id))
            {
                if payment.status == PaymentStatus::Pending && now >= payment.next_retry_at {
                    let token_client = token::Client::new(&env, &payment.token);
                    let escrow_balance = token_client.balance(&env.current_contract_address());

                    if escrow_balance >= payment.amount {
                        token_client.transfer(
                            &env.current_contract_address(),
                            &payment.recipient,
                            &payment.amount,
                        );

                        payment.status = PaymentStatus::Completed;
                        write_payment(&env, &payment);

                        env.events().publish(
                            ("payment_succeeded", payment.id),
                            PaymentSucceededEvent {
                                payment_id: payment.id,
                                recipient: payment.recipient,
                                amount: payment.amount,
                            },
                        );
                    } else {
                        payment.retry_count = payment.retry_count.saturating_add(1);

                        if payment.retry_count > payment.max_retry_attempts {
                            payment.status = PaymentStatus::Failed;
                            write_payment(&env, &payment);

                            env.events().publish(
                                ("payment_failed", payment.id),
                                PaymentFailedEvent {
                                    payment_id: payment.id,
                                    retry_count: payment.retry_count,
                                    max_retry_attempts: payment.max_retry_attempts,
                                    notifier: payment.failure_notifier,
                                },
                            );
                        } else {
                            let retry_interval = interval_for_retry(
                                &payment.retry_intervals,
                                payment.retry_count,
                            );
                            payment.next_retry_at = now.saturating_add(retry_interval);
                            write_payment(&env, &payment);

                            env.events().publish(
                                ("retry_scheduled", payment.id),
                                RetryScheduledEvent {
                                    payment_id: payment.id,
                                    retry_count: payment.retry_count,
                                    next_retry_at: payment.next_retry_at,
                                },
                            );
                        }
                    }

                    processed = processed.saturating_add(1);
                }
            }

            payment_id = payment_id.saturating_add(1);
        }

        processed
    }

    /// @notice Cancels a pending payment request.
    /// @param payer Request owner.
    /// @param payment_id Request id.
    pub fn cancel_payment(env: Env, payer: Address, payment_id: u128) {
        require_initialized(&env);
        payer.require_auth();

        let mut payment = read_payment(&env, payment_id);
        assert!(payment.payer == payer, "Only payer can cancel payment");
        assert!(payment.status == PaymentStatus::Pending, "Payment is not pending");

        payment.status = PaymentStatus::Cancelled;
        write_payment(&env, &payment);
    }

    /// @notice Reads a payment request by id.
    pub fn get_payment(env: Env, payment_id: u128) -> Option<PaymentRequest> {
        env.storage()
            .persistent()
            .get::<_, PaymentRequest>(&StorageKey::Payment(payment_id))
    }

    /// @notice Returns the contract owner.
    pub fn get_owner(env: Env) -> Option<Address> {
        env.storage().persistent().get::<_, Address>(&StorageKey::Owner)
    }
}
