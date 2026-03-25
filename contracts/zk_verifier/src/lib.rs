#![no_std]
use soroban_poseidon::poseidon_hash;
use soroban_sdk::{
    contract, contractimpl, contracttype, crypto::BnScalar, Address, Bytes, BytesN, Env, U256, Vec,
};

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

/// Convert a `BytesN<32>` to a `U256` (big-endian).
fn bytesn_to_u256(env: &Env, b: &BytesN<32>) -> U256 {
    U256::from_be_bytes(env, &b.into())
}

/// Convert a `U256` to a `BytesN<32>` (big-endian, zero-padded).
fn u256_to_bytesn(env: &Env, u: &U256) -> BytesN<32> {
    let be: Bytes = u.to_be_bytes();
    // to_be_bytes may return fewer than 32 bytes for small values; left-pad with zeros.
    let len = be.len();
    if len == 32 {
        be.try_into().unwrap()
    } else {
        let mut padded = Bytes::new(env);
        for _ in 0..(32 - len) {
            padded.push_back(0u8);
        }
        padded.append(&be);
        padded.try_into().unwrap()
    }
}

/// Hash a single field element using Poseidon (t=2, 1 input) over BN254.
fn poseidon1(env: &Env, a: U256) -> U256 {
    let inputs: Vec<U256> = soroban_sdk::vec![env, a];
    poseidon_hash::<2, BnScalar>(env, &inputs)
}

/// Hash two field elements using Poseidon (t=3, 2 inputs) over BN254.
fn poseidon2(env: &Env, a: U256, b: U256) -> U256 {
    let inputs: Vec<U256> = soroban_sdk::vec![env, a, b];
    poseidon_hash::<3, BnScalar>(env, &inputs)
}

/// Interpret raw bytes as a field element by zero-padding to 32 bytes (big-endian U256).
/// Callers must ensure the value is < BN254 field modulus.
fn bytes_to_field(env: &Env, b: &Bytes) -> U256 {
    let len = b.len();
    if len == 32 {
        U256::from_be_bytes(env, b)
    } else {
        let mut padded = Bytes::new(env);
        for _ in 0..(32 - len) {
            padded.push_back(0u8);
        }
        padded.append(b);
        U256::from_be_bytes(env, &padded)
    }
}

#[contractimpl]
impl ZkVerifier {
    /// Store the Merkle root for a listing. Only the listing owner can set or overwrite it.
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

    /// Retrieves the stored Merkle root for a given listing.
    pub fn get_merkle_root(env: Env, listing_id: u64) -> Option<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&DataKey::MerkleRoot(listing_id))
    }

    /// Verify a Merkle inclusion proof for a leaf against the stored root using Poseidon hashing.
    ///
    /// Compatible with off-chain Poseidon (circom/iden3) proof generators over BN254.
    /// The leaf bytes are interpreted as a big-endian field element and hashed with
    /// Poseidon(t=2). Each path step combines two field elements with Poseidon(t=3).
    ///
    /// # Arguments
    /// * `env` - The contract environment.
    /// * `listing_id` - The ID of the listing representing the Merkle tree.
    /// * `leaf` - The raw leaf data (interpreted as a big-endian field element).
    /// * `path` - A `Vec<ProofNode>` representing the inclusion path.
    ///
    /// # Returns
    /// `true` if the computed root matches the stored root; otherwise `false`.
    ///
    /// # Panics
    /// * Panics if the stored Merkle root for `listing_id` is missing.
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

        // Hash the leaf as a single field element via Poseidon(t=2).
        let leaf_fe = bytes_to_field(&env, &leaf);
        let mut current: U256 = poseidon1(&env, leaf_fe);

        // Traverse the proof path, combining with each sibling via Poseidon(t=3).
        for node in path.iter() {
            let sibling = bytesn_to_u256(&env, &node.sibling);
            current = if node.is_left {
                // sibling is left, current is right
                poseidon2(&env, sibling, current)
            } else {
                // current is left, sibling is right
                poseidon2(&env, current, sibling)
            };
        }

        let computed_root = u256_to_bytesn(&env, &current);
        computed_root == root
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger as _},
        Bytes, Env, Vec,
    };

    /// Compute Poseidon(leaf_bytes) as a BytesN<32> — mirrors verify_partial_proof leaf hashing.
    fn poseidon_leaf(env: &Env, leaf: &Bytes) -> BytesN<32> {
        let fe = bytes_to_field(env, leaf);
        let h = poseidon1(env, fe);
        u256_to_bytesn(env, &h)
    }

    /// Compute Poseidon(left, right) as a BytesN<32> — mirrors path node hashing.
    fn poseidon_pair(env: &Env, left: &BytesN<32>, right: &BytesN<32>) -> BytesN<32> {
        let l = bytesn_to_u256(env, left);
        let r = bytesn_to_u256(env, right);
        let h = poseidon2(env, l, r);
        u256_to_bytesn(env, &h)
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
        // Root for a single-leaf tree is Poseidon(leaf).
        let root = poseidon_leaf(&env, &leaf);

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
        let root = poseidon_leaf(&env, &leaf);
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
        let root = poseidon_leaf(&env, &leaf);

        client.set_merkle_root(&owner, &1u64, &root);

        let fake_leaf = Bytes::from_slice(&env, b"fake");
        let fake_root = poseidon_leaf(&env, &fake_leaf);
        client.set_merkle_root(&attacker, &1u64, &fake_root);
    }

    #[test]
    fn test_two_leaf_proof() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(ZkVerifier, ());
        let client = ZkVerifierClient::new(&env, &contract_id);

        let owner = Address::generate(&env);

        // Build a 2-leaf Poseidon Merkle tree:
        //       root = Poseidon(h0, h1)
        //      /                      \
        //  h0 = Poseidon(leaf0)    h1 = Poseidon(leaf1)
        let leaf0 = Bytes::from_slice(&env, b"leaf_zero");
        let leaf1 = Bytes::from_slice(&env, b"leaf_one");
        let h0 = poseidon_leaf(&env, &leaf0);
        let h1 = poseidon_leaf(&env, &leaf1);
        let root = poseidon_pair(&env, &h0, &h1);

        client.set_merkle_root(&owner, &2u64, &root);

        // Prove leaf0 (index 0, sibling h1 is on the right → is_left = false)
        let path0: Vec<ProofNode> = soroban_sdk::vec![
            &env,
            ProofNode {
                sibling: h1.clone(),
                is_left: false,
            }
        ];
        assert!(client.verify_partial_proof(&2u64, &leaf0, &path0));

        // Prove leaf1 (index 1, sibling h0 is on the left → is_left = true)
        let path1: Vec<ProofNode> = soroban_sdk::vec![
            &env,
            ProofNode {
                sibling: h0.clone(),
                is_left: true,
            }
        ];
        assert!(client.verify_partial_proof(&2u64, &leaf1, &path1));
    }
}
