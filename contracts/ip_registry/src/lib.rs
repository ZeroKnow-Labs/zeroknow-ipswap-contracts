#![no_std]
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error, Address,
    Bytes, Env, Vec,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    InvalidInput = 1,
    CounterOverflow = 2,
    Unauthorized = 3,
    ListingNotFound = 4,
}

const PERSISTENT_TTL_LEDGERS: u32 = 6_312_000;

#[contracttype]
#[derive(Clone)]
pub struct Listing {
    pub owner: Address,
    pub ipfs_hash: Bytes,
    pub merkle_root: Bytes,
}

#[contracttype]
pub enum DataKey {
    Listing(u64),
    Counter,
    OwnerIndex(Address),
}

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

/// Emitted when a listing is updated.
#[contractevent]
pub struct ListingUpdated {
    #[topic]
    pub listing_id: u64,
    #[topic]
    pub owner: Address,
}

/// Emitted when a listing is deregistered.
#[contractevent]
pub struct ListingDeregistered {
    #[topic]
    pub listing_id: u64,
    #[topic]
    pub owner: Address,
}

/// Emitted when listing ownership is transferred.
#[contractevent]
pub struct OwnershipTransferred {
    #[topic]
    pub listing_id: u64,
    pub old_owner: Address,
    pub new_owner: Address,
}

#[contract]
pub struct IpRegistry;

#[contractimpl]
impl IpRegistry {
    /// Register a new IP listing. Returns the listing ID.
    pub fn register_ip(env: Env, owner: Address, ipfs_hash: Bytes, merkle_root: Bytes) -> u64 {
        if ipfs_hash.is_empty() || merkle_root.is_empty() {
            panic_with_error!(&env, ContractError::InvalidInput);
        }
        owner.require_auth();
        let prev: u64 = env.storage().persistent().get(&DataKey::Counter).unwrap_or(0);
        let id: u64 = prev
            .checked_add(1)
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::CounterOverflow));
        env.storage().persistent().set(&DataKey::Counter, &id);
        env.storage().persistent().extend_ttl(
            &DataKey::Counter,
            PERSISTENT_TTL_LEDGERS,
            PERSISTENT_TTL_LEDGERS,
        );

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

        env.storage()
            .instance()
            .extend_ttl(PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);

        IpRegistered {
            listing_id: id,
            owner,
            ipfs_hash,
            merkle_root,
        }
        .publish(&env);

        id
    }

    /// Retrieves a specific IP listing by its ID. Extends TTL on read.
    pub fn get_listing(env: Env, listing_id: u64) -> Option<Listing> {
        let key = DataKey::Listing(listing_id);
        if env.storage().persistent().has(&key) {
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
        }
        env.storage().persistent().get(&key)
    }

    /// Retrieves all listing IDs owned by a specific address.
    pub fn list_by_owner(env: Env, owner: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::OwnerIndex(owner))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Returns a page of listing IDs for an owner. offset is 0-indexed.
    pub fn list_by_owner_page(env: Env, owner: Address, offset: u32, limit: u32) -> Vec<u64> {
        let all: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::OwnerIndex(owner))
            .unwrap_or_else(|| Vec::new(&env));
        let len = all.len();
        let start = offset.min(len);
        let end = (offset.saturating_add(limit)).min(len);
        let mut page = Vec::new(&env);
        for i in start..end {
            page.push_back(all.get(i).unwrap());
        }
        page
    }

    /// Update ipfs_hash and merkle_root for an existing listing. Owner only.
    pub fn update_listing(
        env: Env,
        owner: Address,
        listing_id: u64,
        ipfs_hash: Bytes,
        merkle_root: Bytes,
    ) {
        if ipfs_hash.is_empty() || merkle_root.is_empty() {
            panic_with_error!(&env, ContractError::InvalidInput);
        }
        owner.require_auth();
        let key = DataKey::Listing(listing_id);
        let mut listing: Listing = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::ListingNotFound));
        if listing.owner != owner {
            panic_with_error!(&env, ContractError::Unauthorized);
        }
        listing.ipfs_hash = ipfs_hash;
        listing.merkle_root = merkle_root;
        env.storage().persistent().set(&key, &listing);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
        ListingUpdated { listing_id, owner }.publish(&env);
    }

    /// Remove a listing from the registry. Owner only.
    pub fn deregister_listing(env: Env, owner: Address, listing_id: u64) {
        owner.require_auth();
        let key = DataKey::Listing(listing_id);
        let listing: Listing = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::ListingNotFound));
        if listing.owner != owner {
            panic_with_error!(&env, ContractError::Unauthorized);
        }
        env.storage().persistent().remove(&key);
        // Remove from owner index
        let idx_key = DataKey::OwnerIndex(owner.clone());
        let mut ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&idx_key)
            .unwrap_or_else(|| Vec::new(&env));
        let mut new_ids = Vec::new(&env);
        for i in 0..ids.len() {
            let id = ids.get(i).unwrap();
            if id != listing_id {
                new_ids.push_back(id);
            }
        }
        env.storage().persistent().set(&idx_key, &new_ids);
        env.storage()
            .persistent()
            .extend_ttl(&idx_key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
        ListingDeregistered { listing_id, owner }.publish(&env);
    }

    /// Transfer ownership of a listing to a new address. Current owner only.
    pub fn transfer_ownership(
        env: Env,
        current_owner: Address,
        listing_id: u64,
        new_owner: Address,
    ) {
        current_owner.require_auth();
        let key = DataKey::Listing(listing_id);
        let mut listing: Listing = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::ListingNotFound));
        if listing.owner != current_owner {
            panic_with_error!(&env, ContractError::Unauthorized);
        }
        listing.owner = new_owner.clone();
        env.storage().persistent().set(&key, &listing);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
        // Update owner indexes
        let old_idx = DataKey::OwnerIndex(current_owner.clone());
        let mut old_ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&old_idx)
            .unwrap_or_else(|| Vec::new(&env));
        let mut filtered = Vec::new(&env);
        for i in 0..old_ids.len() {
            let id = old_ids.get(i).unwrap();
            if id != listing_id {
                filtered.push_back(id);
            }
        }
        env.storage().persistent().set(&old_idx, &filtered);
        env.storage()
            .persistent()
            .extend_ttl(&old_idx, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
        let new_idx = DataKey::OwnerIndex(new_owner.clone());
        let mut new_ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&new_idx)
            .unwrap_or_else(|| Vec::new(&env));
        new_ids.push_back(listing_id);
        env.storage().persistent().set(&new_idx, &new_ids);
        env.storage()
            .persistent()
            .extend_ttl(&new_idx, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
        OwnershipTransferred {
            listing_id,
            old_owner: current_owner,
            new_owner,
        }
        .publish(&env);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    extern crate std;
    use soroban_sdk::{
        testutils::{Address as _, Events as _, Ledger as _},
        Env, Event,
    };

    #[test]
    fn test_register_and_get() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let hash = Bytes::from_slice(&env, b"QmTestHash");
        let root = Bytes::from_slice(&env, b"merkle_root_bytes");

        let id = client.register_ip(&owner, &hash, &root);
        assert_eq!(id, 1);

        let listing = client.get_listing(&id).expect("listing should exist");
        assert_eq!(listing.owner, owner);
    }

    #[test]
    fn test_register_ip_emits_event() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let hash = Bytes::from_slice(&env, b"QmTestHash");
        let root = Bytes::from_slice(&env, b"merkle_root_bytes");

        let id = client.register_ip(&owner, &hash, &root);

        let expected = IpRegistered {
            listing_id: id,
            owner: owner.clone(),
            ipfs_hash: hash,
            merkle_root: root,
        };
        assert_eq!(
            env.events().all(),
            std::vec![expected.to_xdr(&env, &contract_id)]
        );
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

        let id1 = client.register_ip(&owner_a, &hash, &root);
        let id2 = client.register_ip(&owner_b, &hash, &root);
        let id3 = client.register_ip(&owner_a, &hash, &root);

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
        );

        env.ledger().with_mut(|li| li.sequence_number += 5_000);

        let listing = client.get_listing(&id).expect("listing should exist");
        assert_eq!(listing.owner, owner);
    }

    #[test]
    fn test_get_listing_missing_returns_none() {
        let env = Env::default();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);

        assert!(client.get_listing(&999).is_none());
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
        );
        assert!(result.is_err());
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
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_counter_overflow_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);

        env.as_contract(&contract_id, || {
            env.storage()
                .persistent()
                .set(&DataKey::Counter, &u64::MAX);
        });

        let owner = Address::generate(&env);
        let result = client.try_register_ip(
            &owner,
            &Bytes::from_slice(&env, b"QmHash"),
            &Bytes::from_slice(&env, b"root"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_update_listing_success() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let id = client.register_ip(
            &owner,
            &Bytes::from_slice(&env, b"QmOld"),
            &Bytes::from_slice(&env, b"old_root"),
        );
        client.update_listing(
            &owner,
            &id,
            &Bytes::from_slice(&env, b"QmNew"),
            &Bytes::from_slice(&env, b"new_root"),
        );
        let listing = client.get_listing(&id).unwrap();
        assert_eq!(listing.ipfs_hash, Bytes::from_slice(&env, b"QmNew"));
        assert_eq!(listing.merkle_root, Bytes::from_slice(&env, b"new_root"));
    }

    #[test]
    fn test_update_listing_unauthorized() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let attacker = Address::generate(&env);
        let id = client.register_ip(
            &owner,
            &Bytes::from_slice(&env, b"QmHash"),
            &Bytes::from_slice(&env, b"root"),
        );
        let result = client.try_update_listing(
            &attacker,
            &id,
            &Bytes::from_slice(&env, b"QmNew"),
            &Bytes::from_slice(&env, b"new_root"),
        );
        assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
    }

    #[test]
    fn test_update_listing_not_found() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let result = client.try_update_listing(
            &owner,
            &999u64,
            &Bytes::from_slice(&env, b"QmNew"),
            &Bytes::from_slice(&env, b"new_root"),
        );
        assert_eq!(result, Err(Ok(ContractError::ListingNotFound)));
    }

    #[test]
    fn test_deregister_listing_success() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let id = client.register_ip(
            &owner,
            &Bytes::from_slice(&env, b"QmHash"),
            &Bytes::from_slice(&env, b"root"),
        );
        client.deregister_listing(&owner, &id);
        assert!(client.get_listing(&id).is_none());
        assert_eq!(client.list_by_owner(&owner).len(), 0);
    }

    #[test]
    fn test_deregister_listing_unauthorized() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let attacker = Address::generate(&env);
        let id = client.register_ip(
            &owner,
            &Bytes::from_slice(&env, b"QmHash"),
            &Bytes::from_slice(&env, b"root"),
        );
        let result = client.try_deregister_listing(&attacker, &id);
        assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
    }

    #[test]
    fn test_transfer_ownership_success() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let new_owner = Address::generate(&env);
        let id = client.register_ip(
            &owner,
            &Bytes::from_slice(&env, b"QmHash"),
            &Bytes::from_slice(&env, b"root"),
        );
        client.transfer_ownership(&owner, &id, &new_owner);
        let listing = client.get_listing(&id).unwrap();
        assert_eq!(listing.owner, new_owner);
        assert_eq!(client.list_by_owner(&owner).len(), 0);
        assert_eq!(client.list_by_owner(&new_owner).len(), 1);
    }

    #[test]
    fn test_transfer_ownership_unauthorized() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let attacker = Address::generate(&env);
        let new_owner = Address::generate(&env);
        let id = client.register_ip(
            &owner,
            &Bytes::from_slice(&env, b"QmHash"),
            &Bytes::from_slice(&env, b"root"),
        );
        let result = client.try_transfer_ownership(&attacker, &id, &new_owner);
        assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
    }

    #[test]
    fn test_list_by_owner_page() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(IpRegistry, ());
        let client = IpRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let h = Bytes::from_slice(&env, b"h");
        let r = Bytes::from_slice(&env, b"r");
        let id1 = client.register_ip(&owner, &h, &r);
        let id2 = client.register_ip(&owner, &h, &r);
        let id3 = client.register_ip(&owner, &h, &r);
        let page = client.list_by_owner_page(&owner, &0u32, &2u32);
        assert_eq!(page.len(), 2);
        assert_eq!(page.get(0).unwrap(), id1);
        assert_eq!(page.get(1).unwrap(), id2);
        let page2 = client.list_by_owner_page(&owner, &2u32, &2u32);
        assert_eq!(page2.len(), 1);
        assert_eq!(page2.get(0).unwrap(), id3);
    }
}
