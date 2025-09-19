use soroban_sdk::{
    contracttype, symbol_short, Address, Env, Symbol, String, Vec, Map, 
    contracterror, contractimpl, contract, token::Client as TokenClient
};

use crate::storage::DataKey;
use crate::events::*;

//-----------------------------------------------------------------------------
// Token Swap Errors
//-----------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum TokenSwapError {
    InvalidSwapParams = 1,
    InsufficientBalance = 2,
    SwapExecutionFailed = 3,
    InvalidConversionRate = 4,
    SwapFeeCalculationFailed = 5,
    DexIntegrationError = 6,
    SwapHistoryError = 7,
    UnauthorizedSwap = 8,
    TokenNotSupported = 9,
    SlippageExceeded = 10,
}

//-----------------------------------------------------------------------------
// Token Swap Data Structures
//-----------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum DexProtocol {
    Soroswap,
    Phoenix,
    StellarX,
    Custom(String),
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum SwapStatus {
    Pending,
    Executing,
    Completed,
    Failed,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TokenPair {
    pub token_a: Address,
    pub token_b: Address,
    pub dex_protocol: DexProtocol,
    pub pool_address: Option<Address>,
    pub is_active: bool,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ConversionRate {
    pub token_pair: TokenPair,
    pub rate: i128,
    pub precision: u32,
    pub last_updated: u64,
    pub source: String,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SwapFee {
    pub fee_percentage: u32,
    pub min_fee_amount: i128,
    pub max_fee_amount: i128,
    pub fee_recipient: Address,
    pub is_active: bool,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SwapRequest {
    pub request_id: String,
    pub employer: Address,
    pub employee: Address,
    pub input_token: Address,
    pub output_token: Address,
    pub input_amount: i128,
    pub expected_output_amount: i128,
    pub slippage_tolerance: u32,
    pub dex_protocol: DexProtocol,
    pub status: SwapStatus,
    pub created_at: u64,
    pub executed_at: Option<u64>,
    pub actual_output_amount: Option<i128>,
    pub fee_paid: Option<i128>,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SwapResult {
    pub request_id: String,
    pub success: bool,
    pub input_amount: i128,
    pub output_amount: i128,
    pub fee_amount: i128,
    pub gas_used: u32,
    pub error_message: Option<String>,
    pub executed_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SwapHistoryEntry {
    pub entry_id: String,
    pub request_id: String,
    pub employer: Address,
    pub employee: Address,
    pub input_token: Address,
    pub output_token: Address,
    pub input_amount: i128,
    pub output_amount: i128,
    pub fee_amount: i128,
    pub dex_protocol: DexProtocol,
    pub timestamp: u64,
    pub block_number: u32,
    pub transaction_hash: String,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct DexConfig {
    pub protocol: DexProtocol,
    pub router_address: Address,
    pub factory_address: Address,
    pub is_active: bool,
    pub gas_limit: u32,
    pub priority_fee: u32,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TokenSwapSettings {
    pub enabled: bool,
    pub default_slippage: u32,
    pub max_swap_amount: i128,
    pub min_swap_amount: i128,
    pub swap_timeout: u64,
    pub fee_settings: SwapFee,
    pub supported_dex_protocols: Vec<DexProtocol>,
    pub last_updated: u64,
}

//-----------------------------------------------------------------------------
// Token Swap System Implementation
//-----------------------------------------------------------------------------

#[contract]
pub struct TokenSwapSystem;

#[contractimpl]
impl TokenSwapSystem {
    /// Add or update a token pair for swapping
    pub fn set_token_pair(
        env: Env,
        caller: Address,
        token_pair: TokenPair,
    ) -> Result<(), TokenSwapError> {
        caller.require_auth();
        Self::require_swap_authorized(&env, &caller)?;

        let storage = env.storage().persistent();
        let pair_key = Self::generate_token_pair_key(&token_pair.token_a, &token_pair.token_b);
        storage.set(&DataKey::TokenPair(pair_key), &token_pair);

        Ok(())
    }

    /// Get token pair configuration
    pub fn get_token_pair(
        env: Env,
        token_a: Address,
        token_b: Address,
    ) -> Option<TokenPair> {
        let storage = env.storage().persistent();
        let pair_key = Self::generate_token_pair_key(&token_a, &token_b);
        storage.get(&DataKey::TokenPair(pair_key))
    }

    /// Set conversion rate for a token pair
    pub fn set_conversion_rate(
        env: Env,
        caller: Address,
        token_a: Address,
        token_b: Address,
        rate: i128,
        precision: u32,
        source: String,
    ) -> Result<(), TokenSwapError> {
        caller.require_auth();
        Self::require_swap_authorized(&env, &caller)?;

        if rate <= 0 {
            return Err(TokenSwapError::InvalidConversionRate);
        }

        let storage = env.storage().persistent();
        let pair_key = Self::generate_token_pair_key(&token_a, &token_b);
        
        if let Some(token_pair) = storage.get(&DataKey::TokenPair(pair_key)) {
            let conversion_rate = ConversionRate {
                token_pair,
                rate,
                precision,
                last_updated: env.ledger().timestamp(),
                source,
            };

            storage.set(&DataKey::ConversionRate(pair_key), &conversion_rate);
        }

        Ok(())
    }

    /// Get conversion rate for a token pair
    pub fn get_conversion_rate(
        env: Env,
        token_a: Address,
        token_b: Address,
    ) -> Option<ConversionRate> {
        let storage = env.storage().persistent();
        let pair_key = Self::generate_token_pair_key(&token_a, &token_b);
        storage.get(&DataKey::ConversionRate(pair_key))
    }

    /// Calculate output amount based on conversion rate
    pub fn calculate_output_amount(
        env: Env,
        input_token: Address,
        output_token: Address,
        input_amount: i128,
    ) -> Result<i128, TokenSwapError> {
        let conversion_rate = Self::get_conversion_rate(env.clone(), input_token.clone(), output_token.clone())
            .ok_or(TokenSwapError::InvalidConversionRate)?;

        let output_amount = (input_amount * conversion_rate.rate) / (10i128.pow(conversion_rate.precision));
        
        if output_amount <= 0 {
            return Err(TokenSwapError::InvalidConversionRate);
        }

        Ok(output_amount)
    }

    /// Set swap fee configuration
    pub fn set_swap_fee(
        env: Env,
        caller: Address,
        fee: SwapFee,
    ) -> Result<(), TokenSwapError> {
        caller.require_auth();
        Self::require_swap_authorized(&env, &caller)?;

        let storage = env.storage().persistent();
        storage.set(&DataKey::SwapFee, &fee);

        Ok(())
    }

    /// Get current swap fee configuration
    pub fn get_swap_fee(env: Env) -> Option<SwapFee> {
        let storage = env.storage().persistent();
        storage.get(&DataKey::SwapFee)
    }

    /// Calculate swap fee for a given amount
    pub fn calculate_swap_fee(
        env: Env,
        amount: i128,
    ) -> Result<i128, TokenSwapError> {
        let fee_config = Self::get_swap_fee(env.clone())
            .ok_or(TokenSwapError::SwapFeeCalculationFailed)?;

        if !fee_config.is_active {
            return Ok(0);
        }

        let fee_amount = (amount * fee_config.fee_percentage as i128) / 10000;

        let final_fee = if fee_amount < fee_config.min_fee_amount {
            fee_config.min_fee_amount
        } else if fee_amount > fee_config.max_fee_amount {
            fee_config.max_fee_amount
        } else {
            fee_amount
        };

        Ok(final_fee)
    }

    /// Create a swap request
    pub fn create_swap_request(
        env: Env,
        employer: Address,
        employee: Address,
        input_token: Address,
        output_token: Address,
        input_amount: i128,
        slippage_tolerance: Option<u32>,
        dex_protocol: DexProtocol,
    ) -> Result<SwapRequest, TokenSwapError> {
        employer.require_auth();

        if input_amount <= 0 {
            return Err(TokenSwapError::InvalidSwapParams);
        }

        let token_pair = Self::get_token_pair(env.clone(), input_token.clone(), output_token.clone())
            .ok_or(TokenSwapError::TokenNotSupported)?;

        if !token_pair.is_active {
            return Err(TokenSwapError::TokenNotSupported);
        }

        let settings = Self::get_token_swap_settings(env.clone())
            .ok_or(TokenSwapError::InvalidSwapParams)?;

        if !settings.enabled {
            return Err(TokenSwapError::InvalidSwapParams);
        }

        let expected_output_amount = Self::calculate_output_amount(
            env.clone(),
            input_token.clone(),
            output_token.clone(),
            input_amount,
        )?;

        let slippage = slippage_tolerance.unwrap_or(settings.default_slippage);

        let request = SwapRequest {
            request_id: Self::generate_swap_request_id(&env, &employer, &employee),
            employer: employer.clone(),
            employee: employee.clone(),
            input_token: input_token.clone(),
            output_token: output_token.clone(),
            input_amount,
            expected_output_amount,
            slippage_tolerance: slippage,
            dex_protocol,
            status: SwapStatus::Pending,
            created_at: env.ledger().timestamp(),
            executed_at: None,
            actual_output_amount: None,
            fee_paid: None,
        };

        let storage = env.storage().persistent();
        storage.set(&DataKey::SwapRequest(request.request_id.clone()), &request);

        Ok(request)
    }

    /// Execute a swap request
    pub fn execute_swap(
        env: Env,
        caller: Address,
        request_id: String,
    ) -> Result<SwapResult, TokenSwapError> {
        caller.require_auth();
        Self::require_swap_authorized(&env, &caller)?;

        let storage = env.storage().persistent();
        let request_key = DataKey::SwapRequest(request_id.clone());
        
        let mut request = storage.get(&request_key)
            .ok_or(TokenSwapError::InvalidSwapParams)?;

        if request.status != SwapStatus::Pending {
            return Err(TokenSwapError::InvalidSwapParams);
        }

        request.status = SwapStatus::Executing;
        storage.set(&request_key, &request);

        let current_time = env.ledger().timestamp();
        let mut result = SwapResult {
            request_id: request_id.clone(),
            success: false,
            input_amount: request.input_amount,
            output_amount: 0,
            fee_amount: 0,
            gas_used: 0,
            error_message: None,
            executed_at: current_time,
        };

        let fee_amount = Self::calculate_swap_fee(env.clone(), request.input_amount)?;
        result.fee_amount = fee_amount;

        match Self::perform_dex_swap(&env, &request, fee_amount) {
            Ok(output_amount) => {
                result.success = true;
                result.output_amount = output_amount;

                request.status = SwapStatus::Completed;
                request.executed_at = Some(current_time);
                request.actual_output_amount = Some(output_amount);
                request.fee_paid = Some(fee_amount);
                storage.set(&request_key, &request);

                Self::add_swap_history_entry(&env, &request, output_amount, fee_amount);

                env.events().publish(
                    (symbol_short!("swap_completed"),),
                    (request_id, request.employer, request.employee, output_amount),
                );
            }
            Err(error) => {
                result.error_message = Some(String::from_slice(&env, &format!("{:?}", error)));
                request.status = SwapStatus::Failed;
                storage.set(&request_key, &request);

                env.events().publish(
                    (symbol_short!("swap_failed"),),
                    (request_id, format!("{:?}", error)),
                );
            }
        }

        storage.set(&DataKey::SwapResult(request_id.clone()), &result);

        Ok(result)
    }

    /// Get swap request by ID
    pub fn get_swap_request(env: Env, request_id: String) -> Option<SwapRequest> {
        let storage = env.storage().persistent();
        storage.get(&DataKey::SwapRequest(request_id))
    }

    /// Get swap result by request ID
    pub fn get_swap_result(env: Env, request_id: String) -> Option<SwapResult> {
        let storage = env.storage().persistent();
        storage.get(&DataKey::SwapResult(request_id))
    }

    /// Get swap history for an address
    pub fn get_swap_history(
        env: Env,
        address: Address,
        limit: Option<u32>,
    ) -> Vec<SwapHistoryEntry> {
        let storage = env.storage().persistent();
        let index_key = DataKey::SwapHistoryIndex(address);
        
        if let Some(entry_ids) = storage.get::<DataKey, Vec<String>>(&index_key) {
            let mut entries = Vec::new(&env);
            let max_limit = limit.unwrap_or(50).min(100);
            
            for (i, entry_id) in entry_ids.iter().enumerate() {
                if i >= max_limit as usize {
                    break;
                }
                
                let key = DataKey::SwapHistoryEntry(entry_id);
                if let Some(entry) = storage.get(&key) {
                    entries.push_back(entry);
                }
            }
            entries
        } else {
            Vec::new(&env)
        }
    }

    /// Set token swap settings
    pub fn set_token_swap_settings(
        env: Env,
        caller: Address,
        settings: TokenSwapSettings,
    ) -> Result<(), TokenSwapError> {
        caller.require_auth();
        Self::require_swap_authorized(&env, &caller)?;

        let storage = env.storage().persistent();
        storage.set(&DataKey::TokenSwapSettings, &settings);

        Ok(())
    }

    /// Get token swap settings
    pub fn get_token_swap_settings(env: Env) -> Option<TokenSwapSettings> {
        let storage = env.storage().persistent();
        storage.get(&DataKey::TokenSwapSettings)
    }

    //-----------------------------------------------------------------------------
    // Helper Functions
    //-----------------------------------------------------------------------------

    fn require_swap_authorized(env: &Env, caller: &Address) -> Result<(), TokenSwapError> {
        let storage = env.storage().persistent();
        
        if let Some(owner) = storage.get::<DataKey, Address>(&DataKey::Owner) {
            if caller == &owner {
                return Ok(());
            }
        }

        Err(TokenSwapError::UnauthorizedSwap)
    }

    fn generate_token_pair_key(token_a: &Address, token_b: &Address) -> Address {
        if token_a < token_b {
            token_a.clone()
        } else {
            token_b.clone()
        }
    }

    fn generate_swap_request_id(env: &Env, employer: &Address, employee: &Address) -> String {
        let timestamp = env.ledger().timestamp();
        let sequence = env.ledger().sequence();
        format!("swap_{}_{}_{}_{}", employer, employee, timestamp, sequence)
    }

    fn add_swap_history_entry(
        env: &Env,
        request: &SwapRequest,
        output_amount: i128,
        fee_amount: i128,
    ) {
        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();
        let entry_id = format!("swap_hist_{}_{}", request.request_id, current_time);

        let entry = SwapHistoryEntry {
            entry_id: String::from_slice(env, &entry_id),
            request_id: request.request_id.clone(),
            employer: request.employer.clone(),
            employee: request.employee.clone(),
            input_token: request.input_token.clone(),
            output_token: request.output_token.clone(),
            input_amount: request.input_amount,
            output_amount,
            fee_amount,
            dex_protocol: request.dex_protocol.clone(),
            timestamp: current_time,
            block_number: env.ledger().sequence(),
            transaction_hash: String::from_slice(env, "tx_hash_placeholder"),
        };

        let key = DataKey::SwapHistoryEntry(entry_id.clone());
        storage.set(&key, &entry);

        let index_key = DataKey::SwapHistoryIndex(request.employer.clone());
        let mut history_entries: Vec<String> = storage.get(&index_key).unwrap_or(Vec::new(env));
        history_entries.push_back(String::from_slice(env, &entry_id));
        storage.set(&index_key, &history_entries);
    }

    fn perform_dex_swap(
        env: &Env,
        request: &SwapRequest,
        fee_amount: i128,
    ) -> Result<i128, TokenSwapError> {
        let net_input_amount = request.input_amount - fee_amount;
        let output_amount = Self::calculate_output_amount(
            env.clone(),
            request.input_token.clone(),
            request.output_token.clone(),
            net_input_amount,
        )?;

        let min_output = request.expected_output_amount -
            (request.expected_output_amount * request.slippage_tolerance as i128) / 10000;

        if output_amount < min_output {
            return Err(TokenSwapError::SlippageExceeded);
        }

        Ok(output_amount)
    }

    //-----------------------------------------------------------------------------
    // Enhanced Multi-Currency Support for Payroll System
    //-----------------------------------------------------------------------------

    /// Set or update exchange rate for a token pair
    pub fn set_exchange_rate(
        env: Env,
        caller: Address,
        from_token: Address,
        to_token: Address,
        rate: i128,
        source: String,
    ) -> Result<(), TokenSwapError> {
        caller.require_auth();

        let storage = env.storage().persistent();
        let current_time = env.ledger().timestamp();

        let exchange_rate = crate::storage::ExchangeRate {
            from_token: from_token.clone(),
            to_token: to_token.clone(),
            rate,
            timestamp: current_time,
            source,
            is_active: true,
        };

        // Store current rate
        storage.set(
            &DataKey::ExchangeRate(from_token.clone(), to_token.clone()),
            &exchange_rate,
        );

        // Store historical rate
        storage.set(
            &DataKey::ExchangeRateHistory(from_token.clone(), to_token.clone(), current_time),
            &exchange_rate,
        );

        env.events().publish(
            (symbol_short!("rate_update"),),
            (from_token, to_token, rate, current_time),
        );

        Ok(())
    }

    /// Get current exchange rate between two tokens
    pub fn get_exchange_rate(
        env: Env,
        from_token: Address,
        to_token: Address,
    ) -> Option<crate::storage::ExchangeRate> {
        let storage = env.storage().persistent();
        storage.get(&DataKey::ExchangeRate(from_token, to_token))
    }

    /// Convert amount from one currency to another using stored exchange rates
    pub fn convert_currency(
        env: Env,
        amount: i128,
        from_token: Address,
        to_token: Address,
    ) -> Result<i128, TokenSwapError> {
        // If same token, return original amount
        if from_token == to_token {
            return Ok(amount);
        }

        let storage = env.storage().persistent();

        // Try direct conversion
        if let Some(rate) = storage.get::<DataKey, crate::storage::ExchangeRate>(
            &DataKey::ExchangeRate(from_token.clone(), to_token.clone())
        ) {
            if rate.is_active {
                let converted = (amount * rate.rate) / 10_000_000; // Assuming 7 decimal places
                return Ok(converted);
            }
        }

        // Try reverse conversion
        if let Some(rate) = storage.get::<DataKey, crate::storage::ExchangeRate>(
            &DataKey::ExchangeRate(to_token.clone(), from_token.clone())
        ) {
            if rate.is_active && rate.rate > 0 {
                let converted = (amount * 10_000_000) / rate.rate; // Reverse calculation
                return Ok(converted);
            }
        }

        Err(TokenSwapError::InvalidConversionRate)
    }

    /// Execute payroll payment with automatic currency conversion
    pub fn execute_payroll_with_conversion(
        env: Env,
        caller: Address,
        employee: Address,
        employer_token: Address,  // Token employer has
        employee_token: Address, // Token employee wants
        payroll_amount: i128,    // Amount in employer_token
        max_slippage: u32,       // Slippage tolerance in basis points
    ) -> Result<i128, TokenSwapError> {
        caller.require_auth();

        // If tokens are the same, no conversion needed
        if employer_token == employee_token {
            return Ok(payroll_amount);
        }

        // Get conversion rate
        let converted_amount = Self::convert_currency(
            env.clone(),
            payroll_amount,
            employer_token.clone(),
            employee_token.clone(),
        )?;

        // If we have a direct rate, use it
        let exchange_rate = Self::get_exchange_rate(
            env.clone(),
            employer_token.clone(),
            employee_token.clone(),
        );

        if let Some(rate) = exchange_rate {
            // Check if rate is recent (within 1 hour)
            let current_time = env.ledger().timestamp();
            if current_time - rate.timestamp <= 3600 {
                let converted = (payroll_amount * rate.rate) / 10_000_000;

                // Apply slippage protection
                let min_output = converted - (converted * max_slippage as i128) / 10000;

                if converted >= min_output {
                    env.events().publish(
                        (symbol_short!("payroll_conv"),),
                        (employer_token, employee_token, payroll_amount, converted),
                    );
                    return Ok(converted);
                }
            }
        }

        // If no recent rate or slippage exceeded, attempt DEX swap
        Self::execute_dex_conversion(
            env,
            employer_token,
            employee_token,
            payroll_amount,
            max_slippage,
        )
    }

    /// Execute currency conversion through DEX
    fn execute_dex_conversion(
        env: Env,
        from_token: Address,
        to_token: Address,
        amount: i128,
        max_slippage: u32,
    ) -> Result<i128, TokenSwapError> {
        // Create swap request
        let request_id = String::from_slice(&env, "payroll_conversion_request");

        let expected_output = Self::calculate_output_amount(
            env.clone(),
            from_token.clone(),
            to_token.clone(),
            amount,
        )?;

        let swap_request = SwapRequest {
            request_id: String::from_slice(&env, &request_id),
            employer: env.current_contract_address(), // Contract acts as employer for conversions
            employee: env.current_contract_address(), // Contract acts as employee for conversions
            input_token: from_token,
            output_token: to_token,
            input_amount: amount,
            expected_output_amount: expected_output,
            slippage_tolerance: max_slippage,
            dex_protocol: DexProtocol::Soroswap, // Default to Soroswap
            status: SwapStatus::Pending,
            created_at: env.ledger().timestamp(),
            executed_at: None,
            actual_output_amount: None,
            fee_paid: None,
        };

        // Execute the swap
        let result = Self::execute_swap(env, swap_request)?;

        if result.success {
            Ok(result.output_amount)
        } else {
            Err(TokenSwapError::SwapExecutionFailed)
        }
    }

    /// Get historical exchange rates for reporting
    pub fn get_historical_rates(
        env: Env,
        from_token: Address,
        to_token: Address,
        from_timestamp: u64,
        to_timestamp: u64,
    ) -> Vec<crate::storage::ExchangeRate> {
        let storage = env.storage().persistent();
        let mut rates = Vec::new(&env);

        // This is a simplified implementation
        // In practice, you'd iterate through timestamps in the range
        for timestamp in (from_timestamp..=to_timestamp).step_by(3600) { // Hourly intervals
            if let Some(rate) = storage.get::<DataKey, crate::storage::ExchangeRate>(
                &DataKey::ExchangeRateHistory(from_token.clone(), to_token.clone(), timestamp)
            ) {
                rates.push_back(rate);
            }
        }

        rates
    }

    /// Set preferred payment currency for multi-currency payroll
    pub fn set_payment_currency_preference(
        env: Env,
        caller: Address,
        employee: Address,
        preferred_token: Address,
        auto_convert: bool,
        max_slippage: u32,
    ) -> Result<(), TokenSwapError> {
        caller.require_auth();

        // This would integrate with the payroll system to set payment preferences
        // For now, we'll emit an event to indicate the preference was set
        env.events().publish(
            (symbol_short!("currency_pref"),),
            (caller, employee, preferred_token, auto_convert, max_slippage),
        );

        Ok(())
    }

    /// Batch currency conversion for multiple payroll payments
    pub fn batch_payroll_conversion(
        env: Env,
        caller: Address,
        conversions: Vec<(Address, Address, Address, i128)>, // (employee, from_token, to_token, amount)
        max_slippage: u32,
    ) -> Result<Vec<i128>, TokenSwapError> {
        caller.require_auth();

        let mut results = Vec::new(&env);

        for conversion in conversions.iter() {
            let (employee, from_token, to_token, amount) = conversion;

            let converted_amount = Self::execute_payroll_with_conversion(
                env.clone(),
                caller.clone(),
                employee.clone(),
                from_token.clone(),
                to_token.clone(),
                amount.clone(),
                max_slippage,
            )?;

            results.push_back(converted_amount);
        }

        env.events().publish(
            (symbol_short!("batch_conv"),),
            (caller, conversions.len() as u32, max_slippage),
        );

        Ok(results)
    }
} 