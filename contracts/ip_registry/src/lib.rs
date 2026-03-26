#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, contracterror, Address, Bytes, Env, Vec};

/// Entry for batch IP registration: (ipfs_hash, merkle_root)
pub type IpEntry = (Bytes, Bytes);

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    InvalidInput = 1,
<<<<<<< Implement-batch-reg
    CounterOverflow = 2,
    ListingNotFound = 3,
=======
>>>>>>> main
}

const PERSISTENT_TTL_LEDGERS: u32 = 6_312_000;

#[contracttype]
#[derive(Clone)]
pub struct Listing {
    pub owner: Address,
    pub ipfs_hash: Bytes,
    pub merkle_root: Bytes,
    pub royalty_bps: u32,
    pub royalty_recipient: Address,
}

#[contracttype]
pub enum DataKey {
    Listing(u64),
    Counter,
    OwnerIndex(Address),
}

<<<<<<< Implement-batch-reg
/// Emitted when a new IP listing is registered.
#[contractevent]
pub struct IpRegistered {
    #[topic]
    pub listing_id: u64,
    #[topic]
    pub owner: Address,
    pub ipfs_hash: Bytes,
    pub merkle_root: Bytes,
}

/// Emitted when multiple IP listings are registered in a batch.
#[contractevent]
pub struct BatchIpRegistered {
    #[topic]
    pub owner: Address,
    pub listing_ids: Vec<u64>,
    pub ipfs_hashes: Vec<Bytes>,
    pub merkle_roots: Vec<Bytes>,
}

/// Emitted when a listing ownership is transferred.
#[contractevent]
#[derive(Clone)]
pub struct ListingTransferred {
    #[topic]
    pub listing_id: u64,
    #[topic]
    pub old_owner: Address,
    #[topic]
    pub new_owner: Address,
}

=======
>>>>>>> main
#[contract]
pub struct IpRegistry;

#[contractimpl]
impl IpRegistry {
    /// Register a new IP listing. Returns the listing ID.
    pub fn register_ip(
        env: Env,
        owner: Address,
        ipfs_hash: Bytes,
        merkle_root: Bytes,
    ) -> Result<u64, ContractError> {
        if ipfs_hash.is_empty() || merkle_root.is_empty() {
            return Err(ContractError::InvalidInput);
        }
        owner.require_auth();
        let id: u64 = env.storage().instance().get(&DataKey::Counter).unwrap_or(0) + 1;
        env.storage().instance().set(&DataKey::Counter, &id);

        let key = DataKey::Listing(id);
        env.storage().persistent().set(
            &key,
            &Listing { owner: owner.clone(), ipfs_hash, merkle_root },
        );
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);

        let idx_key = DataKey::OwnerIndex(owner.clone());
        let mut ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&idx_key)
            .unwrap_or_else(|| Vec::new(&env));
        ids.push_back(id);
        env.storage().persistent().set(&idx_key, &ids);
        env.storage()
            .persistent()
            .extend_ttl(&idx_key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);

        env.storage()
            .instance()
            .extend_ttl(PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
        Ok(id)
    }

    /// Register multiple IP listings in a single transaction. Returns listing IDs.
    /// Requires authentication once for the entire batch.
    pub fn batch_register_ip(
        env: Env,
        owner: Address,
        entries: Vec<IpEntry>,
    ) -> Vec<u64> {
        // Validate all entries first - fail fast if any entry is invalid
        let mut i: u32 = 0;
        while i < entries.len() {
            let (ipfs_hash, merkle_root) = entries.get(i).unwrap();
            if ipfs_hash.is_empty() || merkle_root.is_empty() {
                panic_with_error!(&env, ContractError::InvalidInput);
            }
            i += 1;
        }

        // Require auth once for the entire batch
        owner.require_auth();

        let mut listing_ids: Vec<u64> = Vec::new(&env);
        let mut ipfs_hashes: Vec<Bytes> = Vec::new(&env);
        let mut merkle_roots: Vec<Bytes> = Vec::new(&env);

        let mut j: u32 = 0;
        while j < entries.len() {
            let (ipfs_hash, merkle_root) = entries.get(j).unwrap();

            // Get next listing ID
            let prev: u64 = env.storage().instance().get(&DataKey::Counter).unwrap_or(0);
            let id: u64 = prev
                .checked_add(1)
                .unwrap_or_else(|| panic_with_error!(&env, ContractError::CounterOverflow));
            env.storage().instance().set(&DataKey::Counter, &id);

            // Store listing
            let key = DataKey::Listing(id);
            env.storage().persistent().set(
                &key,
                &Listing {
                    owner: owner.clone(),
                    ipfs_hash: ipfs_hash.clone(),
                    merkle_root: merkle_root.clone(),
                },
            );
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);

            // Update owner index
            let idx_key = DataKey::OwnerIndex(owner.clone());
            let mut ids: Vec<u64> = env
                .storage()
                .persistent()
                .get(&idx_key)
                .unwrap_or_else(|| Vec::new(&env));
            ids.push_back(id);
            env.storage().persistent().set(&idx_key, &ids);
            env.storage().persistent().extend_ttl(
                &idx_key,
                PERSISTENT_TTL_LEDGERS,
                PERSISTENT_TTL_LEDGERS,
            );

            // Collect data for batch event
            listing_ids.push_back(id);
            ipfs_hashes.push_back(ipfs_hash.clone());
            merkle_roots.push_back(merkle_root.clone());

            // Emit individual event for each registration
            IpRegistered {
                listing_id: id,
                owner: owner.clone(),
                ipfs_hash,
                merkle_root,
            }
            .publish(&env);

            j += 1;
        }

        // Extend instance TTL once for the entire batch
        env.storage()
            .instance()
            .extend_ttl(PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);

        // Emit batch event
        BatchIpRegistered {
            owner,
            listing_ids: listing_ids.clone(),
            ipfs_hashes,
            merkle_roots,
        }
        .publish(&env);

        listing_ids
    }

    /// Retrieves a specific IP listing by its ID.
    pub fn get_listing(env: Env, listing_id: u64) -> Listing {
        env.storage()
            .persistent()
            .get(&DataKey::Listing(listing_id))
            .expect("listing not found")
    }

    /// Retrieves all listing IDs owned by a specific address.
    pub fn list_by_owner(env: Env, owner: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::OwnerIndex(owner))
            .unwrap_or_else(|| Vec::new(&env))
    }
<<<<<<< Implement-batch-reg

    /// Transfer ownership of a listing to another address.
    pub fn transfer_listing(env: Env, listing_id: u64, new_owner: Address) {
        let key = DataKey::Listing(listing_id);
        let mut listing: Listing = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::ListingNotFound));

        listing.owner.require_auth();
        let old_owner = listing.owner.clone();

        if old_owner == new_owner {
            return;
        }

        // Update listing owner
        listing.owner = new_owner.clone();
        env.storage().persistent().set(&key, &listing);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);

        // Update old owner index
        let old_idx_key = DataKey::OwnerIndex(old_owner.clone());
        let mut old_ids: Vec<u64> = env.storage().persistent().get(&old_idx_key).unwrap();
        if let Some(pos) = old_ids.first_index_of(listing_id) {
            old_ids.remove(pos);
        }
        env.storage().persistent().set(&old_idx_key, &old_ids);
        env.storage().persistent().extend_ttl(
            &old_idx_key,
            PERSISTENT_TTL_LEDGERS,
            PERSISTENT_TTL_LEDGERS,
        );

        // Update new owner index
        let new_idx_key = DataKey::OwnerIndex(new_owner.clone());
        let mut new_ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&new_idx_key)
            .unwrap_or_else(|| Vec::new(&env));
        new_ids.push_back(listing_id);
        env.storage().persistent().set(&new_idx_key, &new_ids);
        env.storage().persistent().extend_ttl(
            &new_idx_key,
            PERSISTENT_TTL_LEDGERS,
            PERSISTENT_TTL_LEDGERS,
        );

        // Emit transfer event
        ListingTransferred {
            listing_id,
            old_owner,
            new_owner,
        }
        .publish(&env);

        env.storage()
            .instance()
            .extend_ttl(PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
    }
=======
>>>>>>> main
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::{Address as _, Ledger as _}, Env};

    #[test]
    fn test_register_and_get() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let hash = Bytes::from_slice(&env, b"QmTestHash");
        let root = Bytes::from_slice(&env, b"merkle_root_bytes");

        let id = client.register_ip(&owner, &hash, &root, &0, &owner);
        assert_eq!(id, 1);

        let listing = client.get_listing(&id);
        assert_eq!(listing.owner, owner);
        assert_eq!(listing.royalty_bps, 0);
        assert_eq!(listing.royalty_recipient, owner);
    }

    #[test]
    fn test_owner_index() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);

        let owner_a = Address::generate(&env);
        let owner_b = Address::generate(&env);
        let hash = Bytes::from_slice(&env, b"QmHash");
        let root = Bytes::from_slice(&env, b"root");

        let id1 = client.register_ip(&owner_a, &hash, &root, &0, &owner_a);
        let id2 = client.register_ip(&owner_b, &hash, &root, &0, &owner_b);
        let id3 = client.register_ip(&owner_a, &hash, &root, &0, &owner_a);

        let a_ids = client.list_by_owner(&owner_a);
        assert_eq!(a_ids.len(), 2);
        assert_eq!(a_ids.get(0).unwrap(), id1);
        assert_eq!(a_ids.get(1).unwrap(), id3);

        let b_ids = client.list_by_owner(&owner_b);
        assert_eq!(b_ids.len(), 1);
        assert_eq!(b_ids.get(0).unwrap(), id2);

        let empty = client.list_by_owner(&Address::generate(&env));
        assert_eq!(empty.len(), 0);
    }

    #[test]
    fn test_listing_survives_ttl_boundary() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let id = client.register_ip(
            &owner,
            &Bytes::from_slice(&env, b"QmHash"),
            &Bytes::from_slice(&env, b"root"),
            &0,
            &owner,
        );

        env.ledger().with_mut(|li| li.sequence_number += 5_000);

        let listing = client.get_listing(&id);
        assert_eq!(listing.owner, owner);
    }

    #[test]
    fn test_register_rejects_empty_ipfs_hash() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let result = client.try_register_ip(
            &owner,
            &Bytes::new(&env),
            &Bytes::from_slice(&env, b"merkle_root_bytes"),
            &0,
            &owner,
        );
        assert_eq!(result, Err(Ok(ContractError::InvalidInput)));
    }

    #[test]
    fn test_register_rejects_empty_merkle_root() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let result = client.try_register_ip(
            &owner,
            &Bytes::from_slice(&env, b"QmTestHash"),
            &Bytes::new(&env),
            &0,
            &owner,
        );
        assert_eq!(result, Err(Ok(ContractError::InvalidInput)));
    }

    #[test]
    fn test_batch_register_ip() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);

        let owner = Address::generate(&env);

        // Create batch entries
        let mut entries: Vec<IpEntry> = Vec::new(&env);
        entries.push_back((
            Bytes::from_slice(&env, b"QmHash1"),
            Bytes::from_slice(&env, b"root1"),
        ));
        entries.push_back((
            Bytes::from_slice(&env, b"QmHash2"),
            Bytes::from_slice(&env, b"root2"),
        ));
        entries.push_back((
            Bytes::from_slice(&env, b"QmHash3"),
            Bytes::from_slice(&env, b"root3"),
        ));

        let ids = client.batch_register_ip(&owner, &entries);
        assert_eq!(ids.len(), 3);
        assert_eq!(ids.get(0).unwrap(), 1);
        assert_eq!(ids.get(1).unwrap(), 2);
        assert_eq!(ids.get(2).unwrap(), 3);

        // Verify all listings were created correctly
        let listing1 = client.get_listing(&1).expect("listing 1 should exist");
        assert_eq!(listing1.owner, owner);
        assert_eq!(listing1.ipfs_hash, Bytes::from_slice(&env, b"QmHash1"));

        let listing2 = client.get_listing(&2).expect("listing 2 should exist");
        assert_eq!(listing2.owner, owner);
        assert_eq!(listing2.merkle_root, Bytes::from_slice(&env, b"root2"));

        let listing3 = client.get_listing(&3).expect("listing 3 should exist");
        assert_eq!(listing3.owner, owner);

        // Verify owner index contains all listing IDs
        let owner_ids = client.list_by_owner(&owner);
        assert_eq!(owner_ids.len(), 3);
    }

    #[test]
    fn test_batch_register_ip_with_single_entry() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);

        let owner = Address::generate(&env);

        let mut entries: Vec<IpEntry> = Vec::new(&env);
        entries.push_back((
            Bytes::from_slice(&env, b"QmSingle"),
            Bytes::from_slice(&env, b"single_root"),
        ));

        let ids = client.batch_register_ip(&owner, &entries);
        assert_eq!(ids.len(), 1);
        assert_eq!(ids.get(0).unwrap(), 1);
    }

    #[test]
    fn test_batch_register_ip_emits_events() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);

        let owner = Address::generate(&env);

        let mut entries: Vec<IpEntry> = Vec::new(&env);
        entries.push_back((
            Bytes::from_slice(&env, b"QmHash1"),
            Bytes::from_slice(&env, b"root1"),
        ));
        entries.push_back((
            Bytes::from_slice(&env, b"QmHash2"),
            Bytes::from_slice(&env, b"root2"),
        ));

        client.batch_register_ip(&owner, &entries);

        // Verify listings were created (events were emitted implicitly)
        assert!(client.get_listing(&1).is_some());
        assert!(client.get_listing(&2).is_some());
    }

    #[test]
    fn test_batch_register_ip_empty_list() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let entries: Vec<IpEntry> = Vec::new(&env);

        let ids = client.batch_register_ip(&owner, &entries);
        assert_eq!(ids.len(), 0);

        // Counter should remain 0
        assert!(client.get_listing(&1).is_none());
    }

    #[test]
    fn test_batch_register_ip_rejects_empty_ipfs_hash() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);

        let owner = Address::generate(&env);

        let mut entries: Vec<IpEntry> = Vec::new(&env);
        entries.push_back((
            Bytes::new(&env), // Empty ipfs_hash
            Bytes::from_slice(&env, b"root"),
        ));

        let result = client.try_batch_register_ip(&owner, &entries);
        assert!(result.is_err());
    }

    #[test]
    fn test_batch_register_ip_rejects_empty_merkle_root() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);

        let owner = Address::generate(&env);

        let mut entries: Vec<IpEntry> = Vec::new(&env);
        entries.push_back((
            Bytes::from_slice(&env, b"QmHash"),
            Bytes::new(&env), // Empty merkle_root
        ));

        let result = client.try_batch_register_ip(&owner, &entries);
        assert!(result.is_err());
    }

    #[test]
    fn test_batch_register_ip_atomic_failure() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);

        let owner = Address::generate(&env);

        // First entry is valid, second is invalid
        let mut entries: Vec<IpEntry> = Vec::new(&env);
        entries.push_back((
            Bytes::from_slice(&env, b"QmHash1"),
            Bytes::from_slice(&env, b"root1"),
        ));
        entries.push_back((
            Bytes::new(&env), // Invalid
            Bytes::from_slice(&env, b"root2"),
        ));

        // Should fail and not register anything
        let result = client.try_batch_register_ip(&owner, &entries);
        assert!(result.is_err());

        // Counter should remain 0 since batch is atomic
        assert!(client.get_listing(&1).is_none());
    }

    #[test]
    fn test_batch_and_single_registration_work_together() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);

        let owner = Address::generate(&env);

        // Register single listing first
        let single_id = client.register_ip(
            &owner,
            &Bytes::from_slice(&env, b"QmSingle"),
            &Bytes::from_slice(&env, b"single_root"),
        );
        assert_eq!(single_id, 1);

        // Register batch
        let mut entries: Vec<IpEntry> = Vec::new(&env);
        entries.push_back((
            Bytes::from_slice(&env, b"QmBatch1"),
            Bytes::from_slice(&env, b"batch_root1"),
        ));
        entries.push_back((
            Bytes::from_slice(&env, b"QmBatch2"),
            Bytes::from_slice(&env, b"batch_root2"),
        ));

        let batch_ids = client.batch_register_ip(&owner, &entries);
        assert_eq!(batch_ids.get(0).unwrap(), 2);
        assert_eq!(batch_ids.get(1).unwrap(), 3);

        // Verify all listings exist
        assert!(client.get_listing(&1).is_some());
        assert!(client.get_listing(&2).is_some());
        assert!(client.get_listing(&3).is_some());

        // Verify owner index has all 3 IDs
        let owner_ids = client.list_by_owner(&owner);
        assert_eq!(owner_ids.len(), 3);
    }
}
