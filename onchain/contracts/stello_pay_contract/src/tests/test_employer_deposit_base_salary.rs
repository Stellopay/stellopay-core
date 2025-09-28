#[cfg(test)]
mod tests {
    use crate::payroll::{PayrollContract, PayrollContractClient};
    use soroban_sdk::testutils::{Ledger, LedgerInfo, MockAuth, MockAuthInvoke};
    use soroban_sdk::token::{StellarAssetClient as TokenAdmin, TokenClient};
    use soroban_sdk::{testutils::Address as _, Address, Env, IntoVal};

    fn create_test_contract() -> (Env, Address, PayrollContractClient<'static>) {
        let env = Env::default();
        let contract_id = env.register(PayrollContract, ());
        let client = PayrollContractClient::new(&env, &contract_id);
        (env, contract_id, client)
    }

    fn setup_token(env: &Env) -> (Address, TokenAdmin) {
        let token_admin = Address::generate(env);
        let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone());
        (
            token_contract_id.address(),
            TokenAdmin::new(&env, &token_contract_id.address()),
        )
    }

    #[test]
    fn test_get_payroll_success() {
        let (env, _, client) = create_test_contract();
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token = Address::generate(&env);
        let amount = 1000i128;
        let interval = 86400u64;
        let recurrence_frequency = 2592000u64; // 30 days in seconds

        env.mock_all_auths();

        client.initialize(&employer);
        client.create_or_update_escrow(
            &employer,
            &employee,
            &token,
            &amount,
            &interval,
            &recurrence_frequency,
        );

        let payroll_data = client.get_payroll(&employee).unwrap();
        assert_eq!(payroll_data.employer, employer);
        assert_eq!(payroll_data.token, token);
        assert_eq!(payroll_data.amount, amount);
        assert_eq!(payroll_data.interval, interval);
        assert_eq!(payroll_data.recurrence_frequency, recurrence_frequency);
    }

    #[test]
    fn test_disburse_salary_success() {
        let (env, contract_id, client) = create_test_contract();
        let (token_address, token_admin) = setup_token(&env);
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let amount = 1000i128;
        let interval = 86400u64;
        let recurrence_frequency = 2592000u64; // 30 days in seconds

        env.mock_all_auths();

        // Fund the employer with tokens
        token_admin.mint(&employer, &10000);

        // Verify minting
        let token_client = TokenClient::new(&env, &token_address);
        let employer_balance = token_client.balance(&employer);
        assert_eq!(employer_balance, 10000);

        // Initialize contract and deposit tokens
        client.initialize(&employer);
        client.deposit_tokens(&employer, &token_address, &5000i128);

        // Verify deposit
        let payroll_contract_balance = token_client.balance(&contract_id);
        assert_eq!(payroll_contract_balance, 5000);

        client.create_or_update_escrow(
            &employer,
            &employee,
            &token_address,
            &amount,
            &interval,
            &recurrence_frequency,
        );

        let next_timestamp = env.ledger().timestamp() + recurrence_frequency + 1;
        env.ledger().set(LedgerInfo {
            timestamp: next_timestamp,
            protocol_version: 22,
            sequence_number: env.ledger().sequence(),
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });

        client.disburse_salary(&employer, &employee);

        // Verify employee received tokens
        let employee_balance = token_client.balance(&employee);
        assert_eq!(employee_balance, amount);
    }

    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #9)")]
    fn test_disburse_salary_interval_not_reached() {
        let (env, _, client) = create_test_contract();
        let (token_address, token_admin) = setup_token(&env);
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let amount = 1000i128;
        let interval = 86400u64;
        let recurrence_frequency = 2592000u64; // 30 days in seconds

        env.mock_all_auths();

        // Fund the employer with tokens
        token_admin.mint(&employer, &10000);

        // Initialize contract and deposit tokens
        client.initialize(&employer);
        client.deposit_tokens(&employer, &token_address, &5000i128);

        client.create_or_update_escrow(
            &employer,
            &employee,
            &token_address,
            &amount,
            &interval,
            &recurrence_frequency,
        );

        // Try to disburse immediately (without advancing time)
        client.disburse_salary(&employer, &employee);
    }

    #[test]
    #[should_panic(expected = "HostError: Error(Auth, InvalidAction)")]
    fn test_disburse_salary_unauthorized() {
        let (env, contract_id, client) = create_test_contract();
        let (token_address, token_admin) = setup_token(&env);
        let employer = Address::generate(&env);
        let unauthorized = Address::generate(&env);
        let employee = Address::generate(&env);
        let amount = 1000i128;
        let interval = 86400u64;
        let recurrence_frequency = 2592000u64; // 30 days in seconds

        // Set up the contract with proper authorization for setup operations
        env.mock_auths(&[
            MockAuth {
                address: &employer,
                invoke: &MockAuthInvoke {
                    contract: &contract_id,
                    fn_name: "initialize",
                    args: (&employer,).into_val(&env),
                    sub_invokes: &[],
                },
            },
            MockAuth {
                address: &employer,
                invoke: &MockAuthInvoke {
                    contract: &contract_id,
                    fn_name: "deposit_tokens",
                    args: (&employer, &token_address, &5000i128).into_val(&env),
                    sub_invokes: &[],
                },
            },
            MockAuth {
                address: &employer,
                invoke: &MockAuthInvoke {
                    contract: &contract_id,
                    fn_name: "create_or_update_escrow",
                    args: (
                        &employer,
                        &employee,
                        &token_address,
                        &amount,
                        &interval,
                        &recurrence_frequency,
                    )
                        .into_val(&env),
                    sub_invokes: &[],
                },
            },
        ]);

        // Fund the employer with tokens
        token_admin.mint(&employer, &10000);

        // Initialize contract and deposit tokens
        client.initialize(&employer);
        client.deposit_tokens(&employer, &token_address, &5000i128);

        client.create_or_update_escrow(
            &employer,
            &employee,
            &token_address,
            &amount,
            &interval,
            &recurrence_frequency,
        );

        // Try to disburse with unauthorized user - NO mock auth for this call
        // This should panic because unauthorized.require_auth() will fail
        client.disburse_salary(&unauthorized, &employee);
    }

    #[test]
    fn test_get_nonexistent_payroll() {
        let (env, _, client) = create_test_contract();
        let employee = Address::generate(&env);

        env.mock_all_auths();

        let result = client.get_payroll(&employee);
        assert!(result.is_none());
    }

    #[test]
    fn test_employee_withdraw_success() {
        let (env, contract_id, client) = create_test_contract();
        let (token_address, token_admin) = setup_token(&env);
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let amount = 1000i128;
        let interval = 86400u64;
        let recurrence_frequency = 2592000u64; // 30 days in seconds

        env.mock_all_auths();

        // Fund the employer with tokens
        token_admin.mint(&employer, &10000);

        // Initialize contract and deposit tokens
        client.initialize(&employer);
        client.deposit_tokens(&employer, &token_address, &5000i128);

        client.create_or_update_escrow(
            &employer,
            &employee,
            &token_address,
            &amount,
            &interval,
            &recurrence_frequency,
        );

        let next_timestamp = env.ledger().timestamp() + recurrence_frequency + 1;
        env.ledger().set(LedgerInfo {
            timestamp: next_timestamp,
            protocol_version: 22,
            sequence_number: env.ledger().sequence(),
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });

        client.employee_withdraw(&employee);

        // Verify employee received tokens
        let token_client = TokenClient::new(&env, &token_address);
        let employee_balance = token_client.balance(&employee);
        assert_eq!(employee_balance, amount);
    }

    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #9)")]
    fn test_employee_withdraw_interval_not_reached() {
        let (env, _, client) = create_test_contract();
        let (token_address, token_admin) = setup_token(&env);
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let amount = 1000i128;
        let interval = 86400u64;
        let recurrence_frequency = 2592000u64; // 30 days in seconds

        env.mock_all_auths();

        // Fund the employer with tokens
        token_admin.mint(&employer, &10000);

        // Initialize contract and deposit tokens
        client.initialize(&employer);
        client.deposit_tokens(&employer, &token_address, &5000i128);

        client.create_or_update_escrow(
            &employer,
            &employee,
            &token_address,
            &amount,
            &interval,
            &recurrence_frequency,
        );

        // Don't advance time - should fail
        client.employee_withdraw(&employee);
    }

    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #4)")]
    fn test_employee_withdraw_nonexistent_payroll() {
        let (env, _, client) = create_test_contract();
        let employee = Address::generate(&env);

        env.mock_all_auths();

        // Initialize contract
        let owner = Address::generate(&env);
        client.initialize(&owner);

        client.employee_withdraw(&employee);
    }

    #[test]
    fn test_multiple_disbursements() {
        let (env, contract_id, client) = create_test_contract();
        let (token_address, token_admin) = setup_token(&env);
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let amount = 1000i128;
        let interval = 86400u64;
        let recurrence_frequency = 2592000u64; // 30 days in seconds

        env.mock_all_auths();

        // Fund the employer with tokens
        token_admin.mint(&employer, &10000);

        // Initialize contract and deposit tokens (enough for 2 payments)
        client.initialize(&employer);
        client.deposit_tokens(&employer, &token_address, &5000i128);

        client.create_or_update_escrow(
            &employer,
            &employee,
            &token_address,
            &amount,
            &interval,
            &recurrence_frequency,
        );

        let next_timestamp = env.ledger().timestamp() + recurrence_frequency + 1;
        env.ledger().set(LedgerInfo {
            timestamp: next_timestamp,
            protocol_version: 22,
            sequence_number: env.ledger().sequence(),
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });

        // First disbursement
        client.disburse_salary(&employer, &employee);

        // Advance time again
        let next_timestamp = env.ledger().timestamp() + recurrence_frequency + 1;
        env.ledger().set(LedgerInfo {
            timestamp: next_timestamp,
            protocol_version: 22,
            sequence_number: env.ledger().sequence(),
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });

        // Second disbursement should succeed
        client.disburse_salary(&employer, &employee);

        // Verify employee received both payments
        let token_client = TokenClient::new(&env, &token_address);
        let employee_balance = token_client.balance(&employee);
        assert_eq!(employee_balance, 2 * amount);
    }

    #[test]
    fn test_boundary_values() {
        let (env, _, client) = create_test_contract();
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token = Address::generate(&env);
        let amount = 1i128; // Minimum positive amount
        let interval = 1u64; // Minimum interval
        let recurrence_frequency = 2592000u64; // 30 days in seconds

        env.mock_all_auths();

        client.initialize(&employer);
        client.create_or_update_escrow(
            &employer,
            &employee,
            &token,
            &amount,
            &interval,
            &recurrence_frequency,
        );

        let payroll_data = client.get_payroll(&employee).unwrap();
        assert_eq!(payroll_data.amount, amount);
        assert_eq!(payroll_data.interval, interval);
        assert_eq!(payroll_data.recurrence_frequency, recurrence_frequency);
    }

    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #6)")]
    fn test_disburse_salary_insufficient_balance() {
        let (env, _, client) = create_test_contract();
        let (token_address, token_admin) = setup_token(&env);
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let amount = 1000i128;
        let interval = 86400u64;
        let recurrence_frequency = 2592000u64; // 30 days in seconds

        env.mock_all_auths();

        // Fund the employer with tokens
        token_admin.mint(&employer, &10000);

        // Initialize contract but don't deposit enough tokens
        client.initialize(&employer);
        client.deposit_tokens(&employer, &token_address, &500i128); // Less than needed

        client.create_or_update_escrow(
            &employer,
            &employee,
            &token_address,
            &amount,
            &interval,
            &recurrence_frequency,
        );

        let next_timestamp = env.ledger().timestamp() + recurrence_frequency + 1;
        env.ledger().set(LedgerInfo {
            timestamp: next_timestamp,
            protocol_version: 22,
            sequence_number: env.ledger().sequence(),
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });

        // Should fail due to insufficient balance
        client.disburse_salary(&employer, &employee);
    }

    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #6)")]
    fn test_employee_withdraw_insufficient_balance() {
        let (env, _, client) = create_test_contract();
        let (token_address, token_admin) = setup_token(&env);
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let amount = 1000i128;
        let interval = 86400u64;
        let recurrence_frequency = 2592000u64; // 30 days in seconds

        env.mock_all_auths();

        // Fund the employer with tokens
        token_admin.mint(&employer, &10000);

        // Initialize contract but don't deposit enough tokens
        client.initialize(&employer);
        client.deposit_tokens(&employer, &token_address, &500i128); // Less than needed

        client.create_or_update_escrow(
            &employer,
            &employee,
            &token_address,
            &amount,
            &interval,
            &recurrence_frequency,
        );

        let next_timestamp = env.ledger().timestamp() + recurrence_frequency + 1;
        env.ledger().set(LedgerInfo {
            timestamp: next_timestamp,
            protocol_version: 22,
            sequence_number: env.ledger().sequence(),
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });

        // Should fail due to insufficient balance
        client.employee_withdraw(&employee);
    }
}
