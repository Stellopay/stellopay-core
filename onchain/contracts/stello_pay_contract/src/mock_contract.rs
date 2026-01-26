use soroban_sdk::{
    contract, contractimpl, Address, Env, Symbol,
};

/// Mock contract for testing upgrade functionality
#[contract]
pub struct UpgradeableContract;

#[contractimpl]
impl UpgradeableContract {
    /// Initialize the contract with version tracking
    pub fn initialize(env: Env, admin: Address) -> u32 {
        admin.require_auth();
        
        // Store admin
        env.storage().instance().set(&Symbol::new(&env, "admin"), &admin);
        
        // Initialize version to 1
        let initial_version: u32 = 1;
        env.storage().instance().set(&Symbol::new(&env, "version"), &initial_version);
        
        initial_version
    }
    
    /// Get current contract version
    pub fn get_contract_version(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&Symbol::new(&env, "version"))
            .unwrap_or(0)
    }
    
    /// Authorize upgrade (admin only)
    pub fn authorize_upgrade(env: Env, caller: Address, new_wasm_hash: soroban_sdk::BytesN<32>) {
        caller.require_auth();
        
        // Verify caller is admin
        let admin: Address = env.storage()
            .instance()
            .get(&Symbol::new(&env, "admin"))
            .expect("Admin not set");
        
        if caller != admin {
            panic!("Unauthorized: Only admin can authorize upgrades");
        }
        
        // Store the authorized wasm hash
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "authorized_wasm"), &new_wasm_hash);
        
        // Emit upgrade authorized event
        env.events().publish(
            (Symbol::new(&env, "upgrade"), Symbol::new(&env, "authorized")),
            new_wasm_hash,
        );
    }
    
    pub fn upgrade(env: Env, _new_wasm_hash: soroban_sdk::BytesN<32>) {
        let current_version: u32 = Self::get_contract_version(env.clone());
        let new_version = current_version + 1;
        env.storage().instance().set(&Symbol::new(&env, "version"), &new_version);
    }
    
    /// Store test data for state preservation tests
    pub fn store_agreement(env: Env, agreement_id: u32, data: Symbol) {
        env.storage()
            .persistent()
            .set(&(Symbol::new(&env, "agreement"), agreement_id), &data);
    }
    
    /// Get stored agreement
    pub fn get_agreement(env: Env, agreement_id: u32) -> Option<Symbol> {
        env.storage()
            .persistent()
            .get(&(Symbol::new(&env, "agreement"), agreement_id))
    }
    
    /// Store employee data
    pub fn store_employee(env: Env, employee_id: u32, name: Symbol) {
        env.storage()
            .persistent()
            .set(&(Symbol::new(&env, "employee"), employee_id), &name);
    }
    
    /// Get employee data
    pub fn get_employee(env: Env, employee_id: u32) -> Option<Symbol> {
        env.storage()
            .persistent()
            .get(&(Symbol::new(&env, "employee"), employee_id))
    }
    
    /// Store balance
    pub fn store_balance(env: Env, account: Address, balance: i128) {
        env.storage()
            .persistent()
            .set(&(Symbol::new(&env, "balance"), account), &balance);
    }
    
    /// Get balance
    pub fn get_balance(env: Env, account: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&(Symbol::new(&env, "balance"), account))
            .unwrap_or(0)
    }
    
    /// Store settings
    pub fn store_setting(env: Env, key: Symbol, value: u32) {
        env.storage()
            .persistent()
            .set(&(Symbol::new(&env, "setting"), key), &value);
    }
    
    /// Get setting
    pub fn get_setting(env: Env, key: Symbol) -> Option<u32> {
        env.storage()
            .persistent()
            .get(&(Symbol::new(&env, "setting"), key))
    }
    
    /// Migration function - can be called multiple times safely
    pub fn migrate(env: Env) -> bool {
        // Check if migration already ran
        let migration_key = Symbol::new(&env, "migration_v1");
        let already_migrated: bool = env.storage()
            .instance()
            .get(&migration_key)
            .unwrap_or(false);
        
        if already_migrated {
            return false; // Already migrated, safe to call again
        }
        
        // Perform migration
        env.storage().instance().set(&migration_key, &true);
        
        true // Migration performed
    }
}
