#![allow(dead_code)]

#[contractimpl]
impl ExampleContract {
    pub fn missing_contract_docs(env: Env, owner: Address) {
        let _ = (env, owner);
    }

    /// Has a summary, but intentionally omits caller permissions.
    ///
    /// # Arguments
    /// * `env` - Contract environment.
    pub fn missing_contract_sections(env: Env) {
        let _ = env;
    }

    /// Fully documented public contract function.
    ///
    /// # Arguments
    /// * `env` - Contract environment.
    ///
    /// # Access Control
    /// This read-only helper does not require authorization.
    pub fn documented_contract_fn(env: Env) {
        let _ = env;
    }

    pub(crate) fn private_missing_docs(env: Env) {
        let _ = env;
    }
}

#[contracterror]
pub enum ExampleError {
    /// A documented error variant.
    Documented = 1,
    MissingDocs = 2,
}

#[contracterror]
enum PrivateError {
    MissingDocs = 1,
}
