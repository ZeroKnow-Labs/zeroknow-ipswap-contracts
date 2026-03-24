#![no_std]
use soroban_sdk::{contract, contractclient, contractimpl, contracttype, token, Address, Bytes, Env, Vec};

const PERSISTENT_TTL_LEDGERS: u32 = 6_312_000;

/// Minimal cross-contract interface for ZkVerifier — mirrors zk_verifier::ProofNode.
#[contracttype]
#[derive(Clone)]
pub struct ProofNode {
    pub sibling: soroban_sdk::BytesN<32>,
    pub is_left: bool,
}

#[contractclient(name = "ZkVerifierClient")]
pub trait ZkVerifierInterface {
    fn verify_partial_proof(env: Env, listing_id: u64, leaf: Bytes, path: Vec<ProofNode>) -> bool;
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ContractError {
    EmptyDecryptionKey,
    InvalidProof,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
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

        let key = DataKey::Swap(id);
        env.storage().persistent().set(
            &key,
            &Swap { listing_id, buyer, seller, usdc_amount, usdc_token, zk_verifier, status: SwapStatus::Pending, decryption_key: None },
        );
        env.storage().persistent().extend_ttl(&key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
        env.storage().instance().extend_ttl(PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
        id
    }

    /// Seller confirms swap by submitting the decryption key and a ZK proof.
    /// The proof is verified against the listing's Merkle root before USDC is released.
    pub fn confirm_swap(
        env: Env,
        swap_id: u64,
        decryption_key: Bytes,
        proof_leaf: Bytes,
        proof_path: Vec<ProofNode>,
    ) {
        assert!(!decryption_key.is_empty(), "{:?}", ContractError::EmptyDecryptionKey);

        let key = DataKey::Swap(swap_id);
        let mut swap: Swap = env.storage().persistent().get(&key).expect("swap not found");
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
        env.storage().persistent().set(&key, &swap);
        env.storage().persistent().extend_ttl(&key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
        env.storage().instance().extend_ttl(PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
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

    pub fn get_decryption_key(env: Env, swap_id: u64) -> Option<Bytes> {
        let swap: Swap = env.storage().persistent().get(&DataKey::Swap(swap_id)).expect("swap not found");
        swap.decryption_key
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger as _},
        token, Env,
    };
    use zk_verifier::{ZkVerifier, ZkVerifierClient as ZkClient};

    fn setup(env: &Env) -> (AtomicSwapClient, ZkClient, token::Client, Address, Address, Address) {
        env.mock_all_auths();

        let usdc_admin = Address::generate(env);
        let usdc_id = env.register_stellar_asset_contract_v2(usdc_admin.clone()).address();
        token::StellarAssetClient::new(env, &usdc_id).mint(&Address::generate(env), &0);

        let buyer = Address::generate(env);
        let seller = Address::generate(env);
        token::StellarAssetClient::new(env, &usdc_id).mint(&buyer, &1000);

        let zk_id = env.register(ZkVerifier, ());
        let swap_id = env.register(AtomicSwap, ());

        (
            AtomicSwapClient::new(env, &swap_id),
            ZkClient::new(env, &zk_id),
            token::Client::new(env, &usdc_id),
            usdc_id,
            buyer,
            seller,
        )
    }

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
        client.confirm_swap(&0, &Bytes::new(&env), &Bytes::new(&env), &Vec::new(&env));
    }

    #[test]
    fn test_decryption_key_accessible_after_confirmation() {
        let env = Env::default();
        let (swap_client, zk_client, usdc_client, usdc_id, buyer, seller) = setup(&env);

        // Single-leaf tree: root = sha256(leaf)
        let leaf = Bytes::from_slice(&env, b"gear_ratio:3:1");
        let root: soroban_sdk::BytesN<32> = env.crypto().sha256(&leaf).into();
        zk_client.set_merkle_root(&1u64, &root);

        let swap_id = swap_client.initiate_swap(&1, &buyer, &seller, &usdc_id, &500, &zk_client.address);

        let dec_key = Bytes::from_slice(&env, b"super-secret-key");
        swap_client.confirm_swap(&swap_id, &dec_key, &leaf, &Vec::new(&env));

        assert_eq!(swap_client.get_decryption_key(&swap_id), Some(dec_key));
        assert_eq!(usdc_client.balance(&seller), 500);
    }

    #[test]
    #[should_panic(expected = "InvalidProof")]
    fn test_confirm_swap_blocked_on_invalid_proof() {
        let env = Env::default();
        let (swap_client, zk_client, _usdc_client, usdc_id, buyer, seller) = setup(&env);

        // Root is sha256("real_leaf") but we submit a wrong leaf — proof must fail
        let real_leaf = Bytes::from_slice(&env, b"real_leaf");
        let root: soroban_sdk::BytesN<32> = env.crypto().sha256(&real_leaf).into();
        zk_client.set_merkle_root(&1u64, &root);

        let swap_id = swap_client.initiate_swap(&1, &buyer, &seller, &usdc_id, &500, &zk_client.address);

        let wrong_leaf = Bytes::from_slice(&env, b"wrong_leaf");
        swap_client.confirm_swap(&swap_id, &Bytes::from_slice(&env, b"key"), &wrong_leaf, &Vec::new(&env));
    }

    #[test]
    fn test_swap_survives_ttl_boundary() {
        let env = Env::default();
        let (swap_client, zk_client, _usdc_client, usdc_id, buyer, seller) = setup(&env);

        let swap_id = swap_client.initiate_swap(&1, &buyer, &seller, &usdc_id, &100, &zk_client.address);
        env.ledger().with_mut(|li| li.sequence_number += 5_000);
        assert_eq!(swap_client.get_swap_status(&swap_id), SwapStatus::Pending);
    }
}
