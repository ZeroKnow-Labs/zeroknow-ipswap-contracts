#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, Address, Bytes, BytesN, Env, Vec};

const PERSISTENT_TTL_LEDGERS: u32 = 6_312_000;

/// A single Merkle proof node: (sibling_hash, is_left)
#[contracttype]
#[derive(Clone)]
pub struct ProofNode {
    pub sibling: BytesN<32>,
    pub is_left: bool,
}

#[contracttype]
pub enum DataKey {
    MerkleRoot(u64),
    Owner(u64),
}

#[contract]
pub struct ZkVerifier;

#[contractimpl]
impl ZkVerifier {
    /// Store the Merkle root for a listing. Only the listing owner can set or overwrite it.
    ///
    /// # Arguments
    /// * `env` - The contract environment.
    /// * `owner` - The address of the caller/listing owner.
    /// * `listing_id` - The ID of the corresponding listing.
    /// * `root` - The root hash (`BytesN<32>`) of the Merkle tree representing the proof.
    ///
    /// # Returns
    /// This function does not return a value.
    ///
    /// # Panics
    /// * Panics if the caller is not the specified `owner`.
    /// * Panics if an `existing_owner` is already stored and does not match the caller `owner`.
    pub fn set_merkle_root(env: Env, owner: Address, listing_id: u64, root: BytesN<32>) {
        owner.require_auth();
        let owner_key = DataKey::Owner(listing_id);
        if let Some(existing_owner) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&owner_key)
        {
            assert!(
                existing_owner == owner,
                "unauthorized: caller is not the listing owner"
            );
        } else {
            env.storage().persistent().set(&owner_key, &owner);
            env.storage().persistent().extend_ttl(
                &owner_key,
                PERSISTENT_TTL_LEDGERS,
                PERSISTENT_TTL_LEDGERS,
            );
        }
        let key = DataKey::MerkleRoot(listing_id);
        env.storage().persistent().set(&key, &root);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
        env.storage()
            .instance()
            .extend_ttl(PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
    }

    /// Returns true if a Merkle root has been set for the given listing, false otherwise.
    ///
    /// # Arguments
    /// * `env` - The contract environment.
    /// * `listing_id` - The ID of the listing.
    ///
    /// # Returns
    /// `true` if a root exists, `false` otherwise. Never panics.
    pub fn proof_exists(env: Env, listing_id: u64) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::MerkleRoot(listing_id))
    }

    /// Retrieves the stored Merkle root for a given listing.
    ///
    /// # Arguments
    /// * `env` - The contract environment.
    /// * `listing_id` - The ID of the listing.
    ///
    /// # Returns
    /// Returns `Some(BytesN<32>)` if a root exists, otherwise `None`.
    pub fn get_merkle_root(env: Env, listing_id: u64) -> Option<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&DataKey::MerkleRoot(listing_id))
    }

    /// Verify a Merkle inclusion proof for a leaf against the stored root.
    pub fn verify_partial_proof(
        env: Env,
        listing_id: u64,
        leaf: Bytes,
        path: Vec<ProofNode>,
    ) -> bool {
        let root: BytesN<32> = env
            .storage()
            .persistent()
            .get(&DataKey::MerkleRoot(listing_id))
            .expect("root not found");

        let mut current: BytesN<32> = env.crypto().sha256(&leaf).into();
        for node in path.iter() {
            let mut combined = Bytes::new(&env);
            if node.is_left {
                combined.extend_from_array(&node.sibling.to_array());
                combined.extend_from_array(&current.to_array());
            } else {
                combined.extend_from_array(&current.to_array());
                combined.extend_from_array(&node.sibling.to_array());
            }
            current = env.crypto().sha256(&combined).into();
        }
        current == root
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger as _},
        Bytes, Env, Vec,
    };

    #[test]
    fn test_proof_exists_returns_false_when_no_root_set() {
        let env = Env::default();
        let contract_id = env.register(ZkVerifier, ());
        let client = ZkVerifierClient::new(&env, &contract_id);

        assert!(!client.proof_exists(&99u64));
    }

    #[test]
    fn test_proof_exists_returns_true_after_set_merkle_root() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(ZkVerifier, ());
        let client = ZkVerifierClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let leaf = Bytes::from_slice(&env, b"proof_data");
        let root: BytesN<32> = env.crypto().sha256(&leaf).into();

        assert!(!client.proof_exists(&1u64));
        client.set_merkle_root(&owner, &1u64, &root);
        assert!(client.proof_exists(&1u64));
    }

    #[test]
    fn test_proof_exists_is_isolated_per_listing() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(ZkVerifier, ());
        let client = ZkVerifierClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let leaf = Bytes::from_slice(&env, b"data");
        let root: BytesN<32> = env.crypto().sha256(&leaf).into();

        client.set_merkle_root(&owner, &1u64, &root);

        assert!(client.proof_exists(&1u64));
        assert!(!client.proof_exists(&2u64));
    }

    #[test]
    fn test_get_merkle_root_missing_returns_none() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(ZkVerifier, ());
        let client = ZkVerifierClient::new(&env, &contract_id);

        assert_eq!(client.get_merkle_root(&99u64), None);
    }

    #[test]
    fn test_single_leaf_proof() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(ZkVerifier, ());
        let client = ZkVerifierClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let leaf = Bytes::from_slice(&env, b"gear_ratio:3:1");
        let root: BytesN<32> = env.crypto().sha256(&leaf).into();

        client.set_merkle_root(&owner, &1u64, &root);

        let path: Vec<ProofNode> = Vec::new(&env);
        assert!(client.verify_partial_proof(&1u64, &leaf, &path));
    }

    #[test]
    fn test_merkle_root_survives_ttl_boundary() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(ZkVerifier, ());
        let client = ZkVerifierClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let leaf = Bytes::from_slice(&env, b"circuit_spec:v2");
        let root: BytesN<32> = env.crypto().sha256(&leaf).into();
        client.set_merkle_root(&owner, &42u64, &root);

        env.ledger().with_mut(|li| li.sequence_number += 5_000);

        assert_eq!(client.get_merkle_root(&42u64), Some(root));
    }

    #[test]
    #[should_panic(expected = "unauthorized: caller is not the listing owner")]
    fn test_unauthorized_overwrite_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(ZkVerifier, ());
        let client = ZkVerifierClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let attacker = Address::generate(&env);
        let leaf = Bytes::from_slice(&env, b"secret");
        let root: BytesN<32> = env.crypto().sha256(&leaf).into();

        client.set_merkle_root(&owner, &1u64, &root);

        let fake_root: BytesN<32> = env
            .crypto()
            .sha256(&Bytes::from_slice(&env, b"fake"))
            .into();
        client.set_merkle_root(&attacker, &1u64, &fake_root);
    }
}
