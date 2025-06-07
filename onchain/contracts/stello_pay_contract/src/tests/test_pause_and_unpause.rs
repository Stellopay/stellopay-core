#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation}, Env, Address};

    fn create_test_contract() -> (Env, Address, PayrollContract) {
        let env = Env::default();
        let contract_id = env.register_contract(None, PayrollContract);
        let contract = PayrollContract::new(&env, &contract_id);
        (env, contract_id, contract)
    }

    fn create_test_addresses(env: &Env) -> (Address, Address, Address) {
        let owner = Address::generate(env);
        let employer = Address::generate(env);
        let employee = Address::generate(env);
        (owner, employer, employee)
    }

    #[test]
    fn test_initialize_contract() {
        let (env, contract_id, _) = create_test_contract();
        let (owner, _, _) = create_test_addresses(&env);

        env.mock_all_auths();

        // Initialize the contract
        PayrollContract::initialize(&env, &contract_id, &owner);

        // Verify owner is set
        let stored_owner = PayrollContract::get_owner(&env, &contract_id);
        assert_eq!(stored_owner, Some(owner));

        // Verify contract starts unpaused
        let is_paused = PayrollContract::is_paused(&env, &contract_id);
        assert_eq!(is_paused, false);
    }

    #[test]
    #[should_panic(expected = "Contract already initialized")]
    fn test_initialize_twice_should_panic() {
        let (env, contract_id, _) = create_test_contract();
        let (owner, _, _) = create_test_addresses(&env);

        env.mock_all_auths();

        // Initialize the contract
        PayrollContract::initialize(&env, &contract_id, &owner);

        // Try to initialize again - should panic
        PayrollContract::initialize(&env, &contract_id, &owner);
    }

    #[test]
    fn test_pause_contract_by_owner() {
        let (env, contract_id, _) = create_test_contract();
        let (owner, _, _) = create_test_addresses(&env);

        env.mock_all_auths();

        // Initialize and pause
        PayrollContract::initialize(&env, &contract_id, &owner);
        let result = PayrollContract::pause(&env, &contract_id, &owner);

        assert!(result.is_ok());
        assert_eq!(PayrollContract::is_paused(&env, &contract_id), true);

        // Check that pause event was emitted
        let events = env.events().all();
        assert!(!events.is_empty());
        // Note: In a real test, you'd verify the specific event content
    }

    #[test]
    fn test_pause_contract_by_non_owner_fails() {
        let (env, contract_id, _) = create_test_contract();
        let (owner, _, _) = create_test_addresses(&env);
        let non_owner = Address::generate(&env);

        env.mock_all_auths();

        // Initialize contract
        PayrollContract::initialize(&env, &contract_id, &owner);

        // Try to pause as non-owner
        let result = PayrollContract::pause(&env, &contract_id, &non_owner);

        assert_eq!(result, Err(PayrollError::Unauthorized));
        assert_eq!(PayrollContract::is_paused(&env, &contract_id), false);
    }

    #[test]
    fn test_unpause_contract_by_owner() {
        let (env, contract_id, _) = create_test_contract();
        let (owner, _, _) = create_test_addresses(&env);

        env.mock_all_auths();

        // Initialize, pause, then unpause
        PayrollContract::initialize(&env, &contract_id, &owner);
        PayrollContract::pause(&env, &contract_id, &owner).unwrap();
        
        assert_eq!(PayrollContract::is_paused(&env, &contract_id), true);

        let result = PayrollContract::unpause(&env, &contract_id, &owner);
        
        assert!(result.is_ok());
        assert_eq!(PayrollContract::is_paused(&env, &contract_id), false);
    }

    #[test]
    fn test_unpause_contract_by_non_owner_fails() {
        let (env, contract_id, _) = create_test_contract();
        let (owner, _, _) = create_test_addresses(&env);
        let non_owner = Address::generate(&env);

        env.mock_all_auths();

        // Initialize and pause
        PayrollContract::initialize(&env, &contract_id, &owner);
        PayrollContract::pause(&env, &contract_id, &owner).unwrap();

        // Try to unpause as non-owner
        let result = PayrollContract::unpause(&env, &contract_id, &non_owner);

        assert_eq!(result, Err(PayrollError::Unauthorized));
        assert_eq!(PayrollContract::is_paused(&env, &contract_id), true);
    }

    #[test]
    fn test_create_escrow_when_paused_fails() {
        let (env, contract_id, _) = create_test_contract();
        let (owner, employer, employee) = create_test_addresses(&env);

        env.mock_all_auths();

        // Initialize and pause contract
        PayrollContract::initialize(&env, &contract_id, &owner);
        PayrollContract::pause(&env, &contract_id, &owner).unwrap();

        // Try to create escrow when paused
        let result = PayrollContract::create_or_update_escrow(
            &env,
            &contract_id,
            &employer,
            &employee,
            1000,
            86400, // 1 day interval
        );

        assert_eq!(result, Err(PayrollError::ContractPaused));
    }

    #[test]
    fn test_create_escrow_when_unpaused_succeeds() {
        let (env, contract_id, _) = create_test_contract();
        let (owner, employer, employee) = create_test_addresses(&env);

        env.mock_all_auths();

        // Initialize contract (starts unpaused)
        PayrollContract::initialize(&env, &contract_id, &owner);

        // Create escrow when unpaused
        let result = PayrollContract::create_or_update_escrow(
            &env,
            &contract_id,
            &employer,
            &employee,
            1000,
            86400, // 1 day interval
        );

        assert!(result.is_ok());
        let payroll = result.unwrap();
        assert_eq!(payroll.employer, employer);
        assert_eq!(payroll.employee, employee);
        assert_eq!(payroll.amount, 1000);
    }

    #[test]
    fn test_disburse_salary_when_paused_fails() {
        let (env, contract_id, _) = create_test_contract();
        let (owner, employer, employee) = create_test_addresses(&env);

        env.mock_all_auths();

        // Initialize contract and create escrow
        PayrollContract::initialize(&env, &contract_id, &owner);
        PayrollContract::create_or_update_escrow(
            &env,
            &contract_id,
            &employer,
            &employee,
            1000,
            86400,
        ).unwrap();

        // Pause contract
        PayrollContract::pause(&env, &contract_id, &owner).unwrap();

        // Advance time past interval
        env.ledger().with_mut(|li| {
            li.timestamp = li.timestamp + 86401; // Just over 1 day
        });

        // Try to disburse when paused
        let result = PayrollContract::disburse_salary(&env, &contract_id, &employer, &employee);

        assert_eq!(result, Err(PayrollError::ContractPaused));
    }

    #[test]
    fn test_employee_withdraw_when_paused_fails() {
        let (env, contract_id, _) = create_test_contract();
        let (owner, employer, employee) = create_test_addresses(&env);

        env.mock_all_auths();

        // Initialize contract and create escrow
        PayrollContract::initialize(&env, &contract_id, &owner);
        PayrollContract::create_or_update_escrow(
            &env,
            &contract_id,
            &employer,
            &employee,
            1000,
            86400,
        ).unwrap();

        // Pause contract
        PayrollContract::pause(&env, &contract_id, &owner).unwrap();

        // Advance time past interval
        env.ledger().with_mut(|li| {
            li.timestamp = li.timestamp + 86401;
        });

        // Try employee withdraw when paused
        let result = PayrollContract::employee_withdraw(&env, &contract_id, &employee);

        assert_eq!(result, Err(PayrollError::ContractPaused));
    }

    #[test]
    fn test_get_payroll_works_when_paused() {
        let (env, contract_id, _) = create_test_contract();
        let (owner, employer, employee) = create_test_addresses(&env);

        env.mock_all_auths();

        // Initialize contract and create escrow
        PayrollContract::initialize(&env, &contract_id, &owner);
        PayrollContract::create_or_update_escrow(
            &env,
            &contract_id,
            &employer,
            &employee,
            1000,
            86400,
        ).unwrap();

        // Pause contract
        PayrollContract::pause(&env, &contract_id, &owner).unwrap();

        // Get payroll should still work (read-only operation)
        let result = PayrollContract::get_payroll(&env, &contract_id, &employee);

        assert!(result.is_some());
        let payroll = result.unwrap();
        assert_eq!(payroll.amount, 1000);
    }

    #[test]
    fn test_transfer_ownership() {
        let (env, contract_id, _) = create_test_contract();
        let (owner, _, _) = create_test_addresses(&env);
        let new_owner = Address::generate(&env);

        env.mock_all_auths();

        // Initialize contract
        PayrollContract::initialize(&env, &contract_id, &owner);

        // Transfer ownership
        let result = PayrollContract::transfer_ownership(&env, &contract_id, &owner, &new_owner);

        assert!(result.is_ok());
        assert_eq!(PayrollContract::get_owner(&env, &contract_id), Some(new_owner.clone()));

        // Old owner should no longer be able to pause
        let pause_result = PayrollContract::pause(&env, &contract_id, &owner);
        assert_eq!(pause_result, Err(PayrollError::Unauthorized));

        // New owner should be able to pause
        let new_pause_result = PayrollContract::pause(&env, &contract_id, &new_owner);
        assert!(new_pause_result.is_ok());
    }

    #[test]
    fn test_transfer_ownership_by_non_owner_fails() {
        let (env, contract_id, _) = create_test_contract();
        let (owner, _, _) = create_test_addresses(&env);
        let non_owner = Address::generate(&env);
        let new_owner = Address::generate(&env);

        env.mock_all_auths();

        // Initialize contract
        PayrollContract::initialize(&env, &contract_id, &owner);

        // Try to transfer ownership as non-owner
        let result = PayrollContract::transfer_ownership(&env, &contract_id, &non_owner, &new_owner);

        assert_eq!(result, Err(PayrollError::Unauthorized));
        assert_eq!(PayrollContract::get_owner(&env, &contract_id), Some(owner));
    }

    #[test]
    fn test_pause_unpause_flow_with_operations() {
        let (env, contract_id, _) = create_test_contract();
        let (owner, employer, employee) = create_test_addresses(&env);

        env.mock_all_auths();

        // Initialize and create escrow
        PayrollContract::initialize(&env, &contract_id, &owner);
        PayrollContract::create_or_update_escrow(
            &env,
            &contract_id,
            &employer,
            &employee,
            1000,
            86400,
        ).unwrap();

        // Advance time
        env.ledger().with_mut(|li| {
            li.timestamp = li.timestamp + 86401;
        });

        // Normal operation should work
        let result = PayrollContract::disburse_salary(&env, &contract_id, &employer, &employee);
        assert!(result.is_ok());

        // Pause contract
        PayrollContract::pause(&env, &contract_id, &owner).unwrap();

        // Operations should fail
        let result = PayrollContract::employee_withdraw(&env, &contract_id, &employee);
        assert_eq!(result, Err(PayrollError::ContractPaused));

        // Unpause contract
        PayrollContract::unpause(&env, &contract_id, &owner).unwrap();

        // Advance time again
        env.ledger().with_mut(|li| {
            li.timestamp = li.timestamp + 86401;
        });

        // Operations should work again
        let result = PayrollContract::employee_withdraw(&env, &contract_id, &employee);
        assert!(result.is_ok());
    }

    #[test]
    fn test_is_paused_returns_false_for_uninitialized_contract() {
        let (env, contract_id, _) = create_test_contract();
        
        // Contract not initialized, should return false (default)
        let is_paused = PayrollContract::is_paused(&env, &contract_id);
        assert_eq!(is_paused, false);
    }

    #[test]
    fn test_pause_uninitialized_contract_fails() {
        let (env, contract_id, _) = create_test_contract();
        let owner = Address::generate(&env);

        env.mock_all_auths();

        // Try to pause uninitialized contract
        let result = PayrollContract::pause(&env, &contract_id, &owner);
        assert_eq!(result, Err(PayrollError::Unauthorized));
    }

    #[test]
    fn test_employee_withdraw_nonexistent_payroll() {
        let (env, contract_id, _) = create_test_contract();
        let (owner, _, _) = create_test_addresses(&env);
        let employee = Address::generate(&env);

        env.mock_all_auths();

        // Initialize contract
        PayrollContract::initialize(&env, &contract_id, &owner);

        // Try to withdraw with non-existent payroll
        let result = PayrollContract::employee_withdraw(&env, &contract_id, &employee);
        
        // Should return PayrollNotFound error (error code #4) to match actual contract behavior
        assert_eq!(result, Err(PayrollError::PayrollNotFound));
    }

    #[test]
    fn test_disburse_salary_nonexistent_payroll() {
        let (env, contract_id, _) = create_test_contract();
        let (owner, employer, _) = create_test_addresses(&env);
        let employee = Address::generate(&env);

        env.mock_all_auths();

        // Initialize contract
        PayrollContract::initialize(&env, &contract_id, &owner);

        // Try to disburse salary for non-existent payroll
        let result = PayrollContract::disburse_salary(&env, &contract_id, &employer, &employee);
        
        // Should return PayrollNotFound error (error code #4) to match actual contract behavior
        assert_eq!(result, Err(PayrollError::PayrollNotFound));
    }
}