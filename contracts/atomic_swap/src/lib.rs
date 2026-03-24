#![no_std]
use soroban_sdk::{contract, contractclient, contractimpl, contracttype, token, Address, Bytes, Env, Vec};

const PERSISTENT_TTL_LEDGERS: u32 = 6_312_000;

/// Minimal cross-contract interface for ZkVerifier — mirrors zk_verifier::ProofNode.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ContractError {
    EmptyDecryptionKey,
}

#[contracttype]
#[derive(Clone, PartialEq)]
pub enum SwapStatus {
    Pending,
    Completed,
    Cancelled,
}

#[contracttype]
#[derive(Clone)]
pub struct Swap {
    pub listing_id: u64,
    pub buyer: Address,
    pub seller: Address,
    pub usdc_amount: i128,
    pub usdc_token: Address,
    pub zk_verifier: Address,
    pub status: SwapStatus,
    pub decryption_key: Option<Bytes>,
}

#[contracttype]
pub enum DataKey {
    Swap(u64),
    Counter,
}

#[contract]
pub struct AtomicSwap;

#[contractimpl]
impl AtomicSwap {
    /// Buyer initiates swap by locking USDC into the contract.
    pub fn initiate_swap(
        env: Env,
        listing_id: u64,
        buyer: Address,
        seller: Address,
        usdc_token: Address,
        usdc_amount: i128,
        zk_verifier: Address,
    ) -> u64 {
        buyer.require_auth();
        token::Client::new(&env, &usdc_token).transfer(
            &buyer,
            &env.current_contract_address(),
            &usdc_amount,
        );
        let id: u64 = env.storage().instance().get(&DataKey::Counter).unwrap_or(0) + 1;
        env.storage().instance().set(&DataKey::Counter, &id);
        env.storage().instance().set(
            &DataKey::Swap(id),
            &Swap { listing_id, buyer, seller, usdc_amount, usdc_token, status: SwapStatus::Pending, decryption_key: None },
        );
        env.storage().persistent().extend_ttl(&key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
        env.storage().instance().extend_ttl(PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
        id
    }

    /// Seller confirms swap by submitting the decryption key; USDC released atomically.
    pub fn confirm_swap(env: Env, swap_id: u64, decryption_key: Bytes) {
        assert!(!decryption_key.is_empty(), "{:?}", ContractError::EmptyDecryptionKey);
        let mut swap: Swap = env
            .storage()
            .instance()
            .get(&DataKey::Swap(swap_id))
            .expect("swap not found");
        assert!(swap.status == SwapStatus::Pending, "swap not pending");
        swap.seller.require_auth();

        // Verify ZK proof before releasing funds
        let verified = ZkVerifierClient::new(&env, &swap.zk_verifier)
            .verify_partial_proof(&swap.listing_id, &proof_leaf, &proof_path);
        assert!(verified, "{:?}", ContractError::InvalidProof);

        token::Client::new(&env, &swap.usdc_token).transfer(
            &env.current_contract_address(),
            &swap.seller,
            &swap.usdc_amount,
        );
        swap.status = SwapStatus::Completed;
        swap.decryption_key = Some(decryption_key);
        env.storage().instance().set(&DataKey::Swap(swap_id), &swap);
    }

    /// Buyer cancels and reclaims USDC if seller never confirms.
    pub fn cancel_swap(env: Env, swap_id: u64) {
        let key = DataKey::Swap(swap_id);
        let mut swap: Swap = env.storage().persistent().get(&key).expect("swap not found");
        assert!(swap.status == SwapStatus::Pending, "swap not pending");
        swap.buyer.require_auth();
        token::Client::new(&env, &swap.usdc_token).transfer(
            &env.current_contract_address(),
            &swap.buyer,
            &swap.usdc_amount,
        );
        swap.status = SwapStatus::Cancelled;
        env.storage().persistent().set(&key, &swap);
        env.storage().persistent().extend_ttl(&key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
        env.storage().instance().extend_ttl(PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
    }

    pub fn get_swap_status(env: Env, swap_id: u64) -> SwapStatus {
        let swap: Swap = env.storage().persistent().get(&DataKey::Swap(swap_id)).expect("swap not found");
        swap.status
    }

    /// Returns the decryption key once the swap is completed.
    pub fn get_decryption_key(env: Env, swap_id: u64) -> Option<Bytes> {
        let swap: Swap = env
            .storage()
            .instance()
            .get(&DataKey::Swap(swap_id))
            .expect("swap not found");
        swap.decryption_key
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        testutils::Address as _,
        token, Env,
    };

    #[test]
    fn test_swap_status_pending_on_initiate() {
        let _ = SwapStatus::Pending;
        let _ = SwapStatus::Completed;
        let _ = SwapStatus::Cancelled;
    }

    #[test]
    #[should_panic(expected = "EmptyDecryptionKey")]
    fn test_confirm_swap_rejects_empty_key() {
        let env = Env::default();
        let contract_id = env.register(AtomicSwap, ());
        let client = AtomicSwapClient::new(&env, &contract_id);
        client.confirm_swap(&0, &Bytes::new(&env));
    }

    #[test]
    fn test_decryption_key_accessible_after_confirmation() {
        let env = Env::default();
        env.mock_all_auths();

        // Deploy a Stellar asset (USDC stand-in) and mint to buyer
        let usdc_admin = Address::generate(&env);
        let usdc_id = env.register_stellar_asset_contract_v2(usdc_admin.clone()).address();
        let usdc_admin_client = token::StellarAssetClient::new(&env, &usdc_id);
        let usdc_client = token::Client::new(&env, &usdc_id);

        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);
        usdc_admin_client.mint(&buyer, &1000);

        // Deploy swap contract
        let contract_id = env.register(AtomicSwap, ());
        let client = AtomicSwapClient::new(&env, &contract_id);

        // Buyer initiates swap
        let swap_id = client.initiate_swap(&1, &buyer, &seller, &usdc_id, &500);

        // Seller confirms with a decryption key
        let key = Bytes::from_slice(&env, b"super-secret-key");
        client.confirm_swap(&swap_id, &key);

        // Key must be retrievable on-chain
        let stored = client.get_decryption_key(&swap_id);
        assert_eq!(stored, Some(key));

        // USDC must have moved to seller
        assert_eq!(usdc_client.balance(&seller), 500);
    }
}
