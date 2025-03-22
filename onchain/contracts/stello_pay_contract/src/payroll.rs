// use soroban_sdk::token::{TokenClient, TokenIdentifier};
use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env};
// use soroban_sdk::token::{TokenClient, TokenIdentifier};
//-----------------------------------------------------------------------------
// Errors
//-----------------------------------------------------------------------------

/// Possible errors for the payroll contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum PayrollError {
    /// Raised when a non-employer attempts to call a restricted function.
    Unauthorized = 1,
    /// Raised when the current time has not reached the required interval.
    IntervalNotReached = 2,
    /// Raised when attempting to initialize or disburse with invalid data.
    InvalidData = 3,
    // Payroll Not Found
    PayrollNotFound = 4,
    // Tranfer Fail
    TransferFailed = 5,
}

//-----------------------------------------------------------------------------
// Data Structures
//-----------------------------------------------------------------------------

/// Key used to store payroll info in contract storage.
#[contracttype]
pub struct PayrollKey(pub Address);

/// Stores basic payroll information.
#[contracttype]
#[derive(Clone)]
pub struct Payroll {
    // Address of the employer (who pays).
    pub employer: Address,
    // Address of the employee (who receives salary).
    pub employee: Address,
    // Amount to be paid (this is an example stub, real logic to do token
    // transfers would reference a token contract).
    pub amount: i64,
    // Payment interval in seconds (e.g., weekly, monthly).
    pub interval: u64,
    // Last payment timestamp.
    pub last_payment_time: u64,
}

//-----------------------------------------------------------------------------
// Contract Struct
//-----------------------------------------------------------------------------
#[contract]
pub struct PayrollContract;

//-----------------------------------------------------------------------------
// Contract Implementation
//-----------------------------------------------------------------------------

#[contractimpl]
impl PayrollContract {
    /// Creates or updates a payroll escrow for production scenarios.
    ///
    /// Requirements:
    /// - Only the employer can call this method (if updating an existing record).
    /// - Must provide valid interval (> 0).
    /// - Sets `last_payment_time` to current block timestamp when created.
    pub fn create_or_update_escrow(
        env: Env,
        employer: Address,
        employee: Address,
        amount: i64,
        interval: u64,
    ) -> Result<Payroll, PayrollError> {
        employer.require_auth();
        if interval == 0 {
            return Err(PayrollError::InvalidData);
        }

        let key = PayrollKey(employee.clone());

        let storage = env.storage().persistent();

        if let Some(existing_payroll) = storage.get::<PayrollKey, Payroll>(&key) {
            if existing_payroll.employer != employer {
                return Err(PayrollError::Unauthorized);
            }

            let updated_payroll = Payroll {
                employer: existing_payroll.employer,
                employee: existing_payroll.employee,
                amount,
                interval,
                last_payment_time: existing_payroll.last_payment_time,
            };

            storage.set(&key, &updated_payroll);
            return Ok(updated_payroll);
        }

        let current_time = env.ledger().timestamp();

        let new_payroll = Payroll {
            employer,
            employee,
            amount,
            interval,
            last_payment_time: current_time,
        };

        storage.set(&key, &new_payroll);
        Ok(new_payroll)
    }

    /// Disburses salary if enough time has elapsed since the last payment.
    /// In a production-level contract, this would involve:
    /// - Checking the token contract for transferring funds.
    /// - Verifying that the caller is the employer or an automated process with the right credentials.
    /// - Updating the `last_payment_time` on success.
    pub fn disburse_salary(
        env: Env,
        caller: Address,
        employee: Address,
    ) -> Result<(), PayrollError> {
        let storage = env.storage().persistent();
        let key = PayrollKey(employee.clone());
        let payroll = storage.get::<PayrollKey, Payroll>(&key);
        let payroll_data = payroll.unwrap();

        if caller != payroll_data.employer {
            return Err(PayrollError::Unauthorized);
        }
        let current_time = env.ledger().timestamp();

        if current_time < payroll_data.last_payment_time + payroll_data.interval {
            return Err(PayrollError::IntervalNotReached);
        }

        // dispatch transfer
        // let xlm_token_id = TokenIdentifier::native();
        // let xlm_client = TokenClient::new(&e, &xlm_token_id);
        //  xlm_client.transfer(&payroll_data.employer, &employee, &payroll_data.amount)

        // Update the last payment time
        let updated_payroll = Payroll {
            employer: payroll_data.employer,
            employee: payroll_data.employee,
            amount: payroll_data.amount,
            interval: payroll_data.interval,
            last_payment_time: current_time,
        };

        let storage = env.storage().persistent();
        storage.set(&key, &updated_payroll);
        return Ok(());
    }

    /// Gets the payroll details for a given employee.
    /// This can be used by UIs or dashboards to display status.
    pub fn get_payroll(env: Env, employee: Address) {

        // return -> Option<Payroll>;
    }

    /// Example function to allow employees to pull payment themselves.
    /// In some real-world setups, employees might want to call the contract
    /// to initiate the disbursement if they are due for a payout.
    ///
    /// For security, you'd still require that the employer's account
    /// or an automated bot is used to actually sign transactions
    /// or have an on-chain logic that automatically triggers disburse.
    pub fn employee_withdraw(env: Env, employee: Address) -> Result<(), PayrollError> {
        employee.require_auth();
        let key = PayrollKey(employee.clone());
        let storage = env.storage().persistent();

        if let Some(existing_payroll) = storage.get::<PayrollKey, Payroll>(&key) {
            // Check if the interval has passed since the last payment time
            let current_time = env.ledger().timestamp();
            let last_payment_time = existing_payroll.last_payment_time;
            if current_time - last_payment_time >= existing_payroll.interval {
                // Process the disbursement of payment
                PayrollContract::disburse_salary(env, existing_payroll.employer.clone(), employee);

                // Update last_payment_time of the payroll
                let updated_payroll = Payroll {
                    last_payment_time: current_time,
                    ..existing_payroll
                };
                storage.set(&key, &updated_payroll);

                Ok(())
            } else {
                // Interval has not completed since the last payment time
                return Err(PayrollError::IntervalNotReached);
            }
        } else {
            // No existing payroll record found for the employee
            return Err(PayrollError::InvalidData);
        }
    }
}
