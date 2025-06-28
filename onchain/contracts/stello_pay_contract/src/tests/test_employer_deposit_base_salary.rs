#[cfg(test)]
mod tests {
    use soroban_sdk::{testutils::Address as _, Env, Address};
    use crate::payroll::{PayrollContract, PayrollContractClient};
    use soroban_sdk::testutils::Ledger;

    fn create_test_contract() -> (Env, Address, PayrollContractClient<'static>) {
        let env = Env::default();
        let contract_id = env.register(PayrollContract, ());
        let client = PayrollContractClient::new(&env, &contract_id);
        (env, contract_id, client)
    }

    #[test]
    fn test_get_payroll_success() {
        let (env, _, client) = create_test_contract();
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token = Address::generate(&env);
        let amount = 1000i128;
        let interval = 86400u64;

        env.mock_all_auths();

        client.initialize(&employer);
        client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

        let payroll_data = client.get_payroll(&employee).unwrap();
        assert_eq!(payroll_data.employer, employer);
        assert_eq!(payroll_data.token, token);
        assert_eq!(payroll_data.amount, amount);
        assert_eq!(payroll_data.interval, interval);
    }

    #[test]
    fn test_disburse_salary_success() {
        let (env, _, client) = create_test_contract();
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token = Address::generate(&env);
        let amount = 1000i128;
        let interval = 86400u64;

        env.mock_all_auths();

        // Initialize contract and deposit tokens
        client.initialize(&employer);
        client.deposit_tokens(&employer, &token, &5000i128);

        client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

        let next_timestamp = env.ledger().timestamp() + interval + 1;
        env.ledger().with_mut(|li| {
            li.timestamp = next_timestamp;
        });

        client.disburse_salary(&employer, &employee);
    }

    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #2)")]
    fn test_disburse_salary_interval_not_reached() {
        let (env, _, client) = create_test_contract();
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token = Address::generate(&env);
        let amount = 1000i128;
        let interval = 86400u64;

        env.mock_all_auths();

        // Initialize contract and deposit tokens
        client.initialize(&employer);
        client.deposit_tokens(&employer, &token, &5000i128);

        client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

        let next_timestamp = env.ledger().timestamp() + interval + 1;
        env.ledger().with_mut(|li| {
            li.timestamp = next_timestamp;
        });

        client.disburse_salary(&employer, &employee);
        // Immediate second disbursement should fail
        client.disburse_salary(&employer, &employee);
    }

    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #1)")]
    fn test_disburse_salary_unauthorized() {
        let (env, _, client) = create_test_contract();
        let employer = Address::generate(&env);
        let unauthorized = Address::generate(&env);
        let employee = Address::generate(&env);
        let token = Address::generate(&env);
        let amount = 1000i128;
        let interval = 86400u64;

        env.mock_all_auths();

        // Initialize contract and deposit tokens
        client.initialize(&employer);
        client.deposit_tokens(&employer, &token, &5000i128);

        client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

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
        let (env, _, client) = create_test_contract();
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token = Address::generate(&env);
        let amount = 1000i128;
        let interval = 86400u64;

        env.mock_all_auths();

        // Initialize contract and deposit tokens
        client.initialize(&employer);
        client.deposit_tokens(&employer, &token, &5000i128);

        client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

        let next_timestamp = env.ledger().timestamp() + interval;
        env.ledger().with_mut(|li| {
            li.timestamp = next_timestamp;
        });

        client.employee_withdraw(&employee);
    }

    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #2)")]
    fn test_employee_withdraw_interval_not_reached() {
        let (env, _, client) = create_test_contract();
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token = Address::generate(&env);
        let amount = 1000i128;
        let interval = 86400u64;

        env.mock_all_auths();

        // Initialize contract and deposit tokens
        client.initialize(&employer);
        client.deposit_tokens(&employer, &token, &5000i128);

        client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

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
        let (env, _, client) = create_test_contract();
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token = Address::generate(&env);
        let amount = 1000i128;
        let interval = 86400u64;

        env.mock_all_auths();

        // Initialize contract and deposit tokens (enough for 2 payments)
        client.initialize(&employer);
        client.deposit_tokens(&employer, &token, &5000i128);

        client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

        let next_timestamp = env.ledger().timestamp() + interval + 1;
        env.ledger().with_mut(|li| {
            li.timestamp = next_timestamp;
        });

        // First disbursement
        client.disburse_salary(&employer, &employee);

        // Advance time again
        let next_timestamp = env.ledger().timestamp() + interval + 1;
        env.ledger().with_mut(|li| {
            li.timestamp = next_timestamp;
        });

        // Second disbursement should succeed
        client.disburse_salary(&employer, &employee);
    }

    #[test]
    fn test_boundary_values() {
        let (env, _, client) = create_test_contract();
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token = Address::generate(&env);
        let amount = 1i128; // Minimum positive amount
        let interval = 1u64; // Minimum interval

        env.mock_all_auths();

        client.initialize(&employer);
        client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

        let payroll_data = client.get_payroll(&employee).unwrap();
        assert_eq!(payroll_data.amount, amount);
        assert_eq!(payroll_data.interval, interval);
    }

    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #7)")]
    fn test_disburse_salary_insufficient_balance() {
        let (env, _, client) = create_test_contract();
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token = Address::generate(&env);
        let amount = 1000i128;
        let interval = 86400u64;

        env.mock_all_auths();

        // Initialize contract but don't deposit enough tokens
        client.initialize(&employer);
        client.deposit_tokens(&employer, &token, &500i128); // Less than needed

        client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

        let next_timestamp = env.ledger().timestamp() + interval + 1;
        env.ledger().with_mut(|li| {
            li.timestamp = next_timestamp;
        });

        // Should fail due to insufficient balance
        client.disburse_salary(&employer, &employee);
    }

    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #7)")]
    fn test_employee_withdraw_insufficient_balance() {
        let (env, _, client) = create_test_contract();
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token = Address::generate(&env);
        let amount = 1000i128;
        let interval = 86400u64;

        env.mock_all_auths();

        // Initialize contract but don't deposit enough tokens
        client.initialize(&employer);
        client.deposit_tokens(&employer, &token, &500i128); // Less than needed

        client.create_or_update_escrow(&employer, &employee, &token, &amount, &interval);

        let next_timestamp = env.ledger().timestamp() + interval + 1;
        env.ledger().with_mut(|li| {
            li.timestamp = next_timestamp;
        });

        // Should fail due to insufficient balance
        client.employee_withdraw(&employee);
    }
}