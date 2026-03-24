#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, contracterror, Address, Bytes, Env, Vec};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    InvalidInput = 1,
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
}

#[contract]
pub struct IpRegistry;

#[contractimpl]
impl IpRegistry {
    /// Register a new IP listing. Returns the listing ID.
    pub fn register_ip(env: Env, owner: Address, ipfs_hash: Bytes, merkle_root: Bytes) -> Result<u64, ContractError> {
        if ipfs_hash.is_empty() || merkle_root.is_empty() {
            return Err(ContractError::InvalidInput);
        }
        owner.require_auth();
        let id: u64 = env.storage().instance().get(&DataKey::Counter).unwrap_or(0) + 1;
        env.storage().instance().set(&DataKey::Counter, &id);

        let key = DataKey::Listing(id);
        env.storage().persistent().set(&key, &Listing { owner, ipfs_hash, merkle_root });
        env.storage().persistent().extend_ttl(&key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
        env.storage().instance().extend_ttl(PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
        Ok(id)
    }

    pub fn get_listing(env: Env, listing_id: u64) -> Listing {
        env.storage()
            .persistent()
            .get(&DataKey::Listing(listing_id))
            .expect("listing not found")
    }

    pub fn list_by_owner(env: Env, owner: Address) -> Vec<u64> {
        let count: u64 = env.storage().instance().get(&DataKey::Counter).unwrap_or(0);
        let mut result = Vec::new(&env);
        for id in 1..=count {
            let listing: Listing = env.storage().persistent().get(&DataKey::Listing(id)).unwrap();
            if listing.owner == owner {
                result.push_back(id);
            }
        }
        result
    }
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

        let id = client.register_ip(&owner, &hash, &root);
        assert_eq!(id, 1);

        let listing = client.get_listing(&id);
        assert_eq!(listing.owner, owner);
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
        );
        assert_eq!(result, Err(Ok(ContractError::InvalidInput)));
    }
}
