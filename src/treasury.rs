// src/treasury.rs
//! Treasury contract for managing contributions and funds.

use soroban_sdk::{contractimpl, contracterror, contracttype, Env, Vec, Symbol, String, Address, vec};
use soroban_sdk::storage::{Storage, StorageKey};

// Tipos de error
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum TreasuryError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InsufficientFunds = 4,
    InvalidAmount = 5,
}

// Estructura de contribución
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Contribution {
    pub contributor: Address,
    pub amount: i128,
    pub timestamp: u64,
}

// Estado del contrato
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct State {
    pub admin: Address,
    pub total_contributions: i128,
    pub contribution_count: u64,
}

// Claves de almacenamiento
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    State,
    Contributions(u64),
    ContributionCount,
}

// Contrato de tesorería
pub struct TreasuryContract;

// ============================================================================
// UTILIDADES DE TTL
// ============================================================================

/// Extiende el TTL del almacenamiento de instancia (para datos de contrato).
/// TTL en Soroban se mide en ledgers. Se extiende a 1000 ledgers (~5 horas).
fn bump_instance(env: &Env) {
    env.storage().instance().extend_ttl(100, 1000);
}

/// Extiende el TTL del almacenamiento persistente (para datos de usuario).
/// Se extiende a 1000 ledgers (~5 horas).
fn bump_persistent(env: &Env, key: &StorageKey) {
    env.storage().persistent().extend_ttl(key, 100, 1000);
}

// ============================================================================
// FUNCIONES DEL CONTRATO
// ============================================================================

#[contractimpl]
impl TreasuryContract {
    /// Inicializa el contrato con el administrador.
    pub fn initialize(env: Env, admin: Address) -> Result<(), TreasuryError> {
        // Bump TTL de instancia
        bump_instance(&env);

        // Verificar si ya está inicializado
        if env.storage().instance().has(&DataKey::State) {
            return Err(TreasuryError::AlreadyInitialized);
        }

        // Guardar estado inicial
        let state = State {
            admin,
            total_contributions: 0,
            contribution_count: 0,
        };
        env.storage().instance().set(&DataKey::State, &state);

        Ok(())
    }

    /// Realiza una contribución.
    pub fn contribute(env: Env, contributor: Address, amount: i128) -> Result<(), TreasuryError> {
        // Bump TTL al inicio
        bump_instance(&env);

        // Verificar que el contrato está inicializado
        let mut state: State = env.storage().instance().get(&DataKey::State)
            .ok_or(TreasuryError::NotInitialized)?;

        // Validar monto
        if amount <= 0 {
            return Err(TreasuryError::InvalidAmount);
        }

        // Actualizar estado
        state.total_contributions += amount;
        state.contribution_count += 1;
        env.storage().instance().set(&DataKey::State, &state);

        // Guardar contribución individual en almacenamiento persistente
        let contribution_key = DataKey::Contributions(state.contribution_count);
        let contribution = Contribution {
            contributor,
            amount,
            timestamp: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&contribution_key, &contribution);

        // Bump TTL del almacenamiento persistente
        let storage_key = env.storage().persistent().get_key(&contribution_key).unwrap();
        bump_persistent(&env, &storage_key);

        Ok(())
    }

    /// Obtiene el estado del contrato.
    pub fn get_state(env: Env) -> Result<State, TreasuryError> {
        bump_instance(&env);
        env.storage().instance().get(&DataKey::State)
            .ok_or(TreasuryError::NotInitialized)
    }

    /// Obtiene una contribución por su ID.
    pub fn get_contribution(env: Env, id: u64) -> Result<Contribution, TreasuryError> {
        bump_instance(&env);
        let key = DataKey::Contributions(id);
        env.storage().persistent().get(&key)
            .ok_or(TreasuryError::NotFound)
    }
}

// ============================================================================
// ERRORES ADICIONALES
// ============================================================================

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum TreasuryError {
    // ... (errores anteriores)
    NotFound = 6,
}