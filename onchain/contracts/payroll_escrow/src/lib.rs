#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

/// PayrollEscrow Contract for managing fund deposits, releases, and refunds.
///
/// This contract provides secure escrow functionality that can be reused across
/// multiple agreement types. It enforces strict access control where only the
/// designated manager contract can authorize fund movements.
///
/// # Security Model
///
/// - Only the manager contract can release or refund funds
/// - Only the admin can initialize or upgrade the contract
/// - Per-agreement balance tracking prevents cross-agreement fund mixing
/// - All operations emit events for auditability
#[contract]
pub struct PayrollEscrowContract;

/// Storage keys for the escrow contract
#[contracttype]
#[derive(Clone)]
pub enum StorageKey {
    /// Agreement balance: agreement_id -> i128
    AgreementBalance(u128),
    /// Agreement employer: agreement_id -> Address
    AgreementEmployer(u128),
    /// Token address used for this escrow
    Token,
    /// Manager contract address (authorized to release/refund)
    Manager,
    /// Admin address (authorized to initialize/upgrade)
    Admin,
    /// Initialization flag
    Initialized,
}

/// Events emitted by the escrow contract
#[contracttype]
#[derive(Clone, Debug)]
pub struct FundedEvent {
    pub agreement_id: u128,
    pub employer: Address,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct ReleasedEvent {
    pub agreement_id: u128,
    pub to: Address,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct RefundedEvent {
    pub agreement_id: u128,
    pub to: Address,
    pub amount: i128,
}

#[contractimpl]
impl PayrollEscrowContract {
    /// Initializes the escrow contract.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment
    /// * `admin` - The admin address (must authenticate)
    /// * `token` - The token address to use for escrow
    /// * `manager` - The manager contract address authorized to release/refund funds
    ///
    /// # Requirements
    ///
    /// * Contract must not be already initialized
    /// * Admin must authenticate
    ///
    /// # Access Control
    ///
    /// Only callable once. The authenticated admin becomes the admin.
    pub fn initialize(env: Env, admin: Address, token: Address, manager: Address) {
        admin.require_auth();

        // Check if already initialized
        let initialized: bool = env
            .storage()
            .persistent()
            .get(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(!initialized, "Contract already initialized");

        // Store configuration
        env.storage().persistent().set(&StorageKey::Token, &token);
        env.storage().persistent().set(&StorageKey::Manager, &manager);
        env.storage().persistent().set(&StorageKey::Admin, &admin);
        env.storage().persistent().set(&StorageKey::Initialized, &true);
    }

    /// Funds an agreement with tokens.
    ///
    /// Transfers tokens from the caller to this contract and records the balance
    /// for the specified agreement.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment
    /// * `from` - The address funding the agreement (must authenticate)
    /// * `agreement_id` - The unique identifier for the agreement
    /// * `employer` - The employer address funding the agreement
    /// * `amount` - The amount of tokens to deposit
    ///
    /// # Requirements
    ///
    /// * Contract must be initialized
    /// * Amount must be positive
    /// * Caller must have approved sufficient tokens for transfer
    ///
    /// # Events
    ///
    /// Emits `Funded` event on success.
    pub fn fund_agreement(env: Env, from: Address, agreement_id: u128, employer: Address, amount: i128) {
        from.require_auth();

        // Validate contract is initialized
        let initialized: bool = env
            .storage()
            .persistent()
            .get(&StorageKey::Initialized)
            .unwrap_or(false);
        assert!(initialized, "Contract not initialized");

        // Validate amount
        assert!(amount > 0, "Amount must be positive");

        // Get token address
        let token: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::Token)
            .expect("Token not set");

        // Transfer tokens from caller to this contract
        let token_client = soroban_sdk::token::Client::new(&env, &token);
        token_client.transfer(&from, &env.current_contract_address(), &amount);

        // Update agreement balance
        let current_balance: i128 = env
            .storage()
            .persistent()
            .get(&StorageKey::AgreementBalance(agreement_id))
            .unwrap_or(0);
        let new_balance = current_balance
            .checked_add(amount)
            .expect("Balance overflow");
        env.storage()
            .persistent()
            .set(&StorageKey::AgreementBalance(agreement_id), &new_balance);

        // Store employer if not already set
        if env
            .storage()
            .persistent()
            .get::<_, Address>(&StorageKey::AgreementEmployer(agreement_id))
            .is_none()
        {
            env.storage()
                .persistent()
                .set(&StorageKey::AgreementEmployer(agreement_id), &employer);
        }

        // Emit event
        env.events().publish(
            ("funded", agreement_id),
            FundedEvent {
                agreement_id,
                employer,
                amount,
            },
        );
    }

    /// Releases funds from escrow to a recipient.
    ///
    /// Only the manager contract can call this function. This ensures that
    /// funds are only released when authorized by the main payroll contract.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment
    /// * `caller` - The caller address (must be manager, must authenticate)
    /// * `agreement_id` - The unique identifier for the agreement
    /// * `to` - The recipient address
    /// * `amount` - The amount of tokens to release
    ///
    /// # Requirements
    ///
    /// * Caller must be the manager contract
    /// * Agreement must have sufficient balance
    /// * Amount must be positive
    ///
    /// # Access Control
    ///
    /// Only the manager contract can release funds.
    ///
    /// # Events
    ///
    /// Emits `Released` event on success.
    pub fn release(env: Env, caller: Address, agreement_id: u128, to: Address, amount: i128) {
        caller.require_auth();

        // Check manager authorization
        let manager: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::Manager)
            .expect("Manager not set");
        assert!(caller == manager, "Only manager can release funds");

        // Validate amount
        assert!(amount > 0, "Amount must be positive");

        // Check balance
        let balance: i128 = env
            .storage()
            .persistent()
            .get(&StorageKey::AgreementBalance(agreement_id))
            .unwrap_or(0);
        assert!(balance >= amount, "Insufficient balance");

        // Get token address
        let token: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::Token)
            .expect("Token not set");

        // Transfer tokens
        let token_client = soroban_sdk::token::Client::new(&env, &token);
        token_client.transfer(&env.current_contract_address(), &to, &amount);

        // Update balance
        let new_balance = balance - amount;
        env.storage()
            .persistent()
            .set(&StorageKey::AgreementBalance(agreement_id), &new_balance);

        // Emit event
        env.events().publish(
            ("released", agreement_id),
            ReleasedEvent {
                agreement_id,
                to,
                amount,
            },
        );
    }

    /// Refunds remaining balance to the employer.
    ///
    /// Only the manager contract can call this function. This is typically used
    /// when an agreement is cancelled and the grace period has expired.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment
    /// * `caller` - The caller address (must be manager, must authenticate)
    /// * `agreement_id` - The unique identifier for the agreement
    ///
    /// # Requirements
    ///
    /// * Caller must be the manager contract
    /// * Agreement must have a balance
    /// * Agreement must have an employer address
    ///
    /// # Access Control
    ///
    /// Only the manager contract can refund funds.
    ///
    /// # Events
    ///
    /// Emits `Refunded` event on success.
    pub fn refund_remaining(env: Env, caller: Address, agreement_id: u128) {
        caller.require_auth();

        // Check manager authorization
        let manager: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::Manager)
            .expect("Manager not set");
        assert!(caller == manager, "Only manager can refund funds");

        // Get balance
        let balance: i128 = env
            .storage()
            .persistent()
            .get(&StorageKey::AgreementBalance(agreement_id))
            .unwrap_or(0);
        assert!(balance > 0, "No balance to refund");

        // Get employer
        let employer: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::AgreementEmployer(agreement_id))
            .expect("Employer not found");

        // Get token address
        let token: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::Token)
            .expect("Token not set");

        // Transfer tokens
        let token_client = soroban_sdk::token::Client::new(&env, &token);
        token_client.transfer(&env.current_contract_address(), &employer, &balance);

        // Clear balance
        env.storage()
            .persistent()
            .set(&StorageKey::AgreementBalance(agreement_id), &0i128);

        // Emit event
        env.events().publish(
            ("refunded", agreement_id),
            RefundedEvent {
                agreement_id,
                to: employer,
                amount: balance,
            },
        );
    }

    /// Gets the current balance for an agreement.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment
    /// * `agreement_id` - The unique identifier for the agreement
    ///
    /// # Returns
    ///
    /// The current balance (0 if no balance exists)
    pub fn get_agreement_balance(env: Env, agreement_id: u128) -> i128 {
        env.storage()
            .persistent()
            .get(&StorageKey::AgreementBalance(agreement_id))
            .unwrap_or(0)
    }

    /// Gets the employer address for an agreement.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment
    /// * `agreement_id` - The unique identifier for the agreement
    ///
    /// # Returns
    ///
    /// The employer address if found, None otherwise
    pub fn get_agreement_employer(env: Env, agreement_id: u128) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&StorageKey::AgreementEmployer(agreement_id))
    }
}

