#![no_std]
use ip_registry::IpRegistryClient;
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Bytes, Env};

const PERSISTENT_TTL_LEDGERS: u32 = 6_312_000;

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ContractError {
    EmptyDecryptionKey,
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
    BuyerIndex(Address),
}

#[contract]
pub struct AtomicSwap;

#[contractimpl]
impl AtomicSwap {
    /// Buyer initiates swap by locking USDC into the contract.
    /// Cross-calls ip_registry to verify seller owns the listing.
    pub fn initiate_swap(
        env: Env,
        listing_id: u64,
        buyer: Address,
        seller: Address,
        usdc_token: Address,
        usdc_amount: i128,
        zk_verifier: Address,
        ip_registry: Address,
    ) -> u64 {
        buyer.require_auth();

        // Verify seller owns the listing in ip_registry
        let listing = IpRegistryClient::new(&env, &ip_registry).get_listing(&listing_id);
        assert!(listing.owner == seller, "seller does not own this listing");

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
            &Swap { listing_id, buyer: buyer.clone(), seller, usdc_amount, usdc_token, zk_verifier, status: SwapStatus::Pending, decryption_key: None },
        );
        env.storage().persistent().extend_ttl(&key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);
        env.storage().instance().extend_ttl(PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);

        // Maintain buyer index
        let buyer_key = DataKey::BuyerIndex(buyer.clone());
        let mut ids: soroban_sdk::Vec<u64> = env
            .storage()
            .persistent()
            .get(&buyer_key)
            .unwrap_or_else(|| soroban_sdk::Vec::new(&env));
        ids.push_back(id);
        env.storage().persistent().set(&buyer_key, &ids);
        env.storage().persistent().extend_ttl(&buyer_key, PERSISTENT_TTL_LEDGERS, PERSISTENT_TTL_LEDGERS);

        id
    }

    /// Seller confirms swap by submitting the decryption key; USDC released atomically.
    pub fn confirm_swap(env: Env, swap_id: u64, decryption_key: Bytes) {
        assert!(!decryption_key.is_empty(), "{:?}", ContractError::EmptyDecryptionKey);
        let key = DataKey::Swap(swap_id);
        let mut swap: Swap = env.storage().persistent().get(&key).expect("swap not found");
        assert!(swap.status == SwapStatus::Pending, "swap not pending");
        swap.seller.require_auth();
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

    pub fn get_swap_status(env: Env, swap_id: u64) -> Option<SwapStatus> {
        env.storage()
            .persistent()
            .get::<DataKey, Swap>(&DataKey::Swap(swap_id))
            .map(|swap| swap.status)
    }

    /// Returns the decryption key once the swap is completed.
    pub fn get_decryption_key(env: Env, swap_id: u64) -> Option<Bytes> {
        env.storage()
            .persistent()
            .get::<DataKey, Swap>(&DataKey::Swap(swap_id))
            .and_then(|swap| swap.decryption_key)
    }

    /// Returns all swap IDs initiated by the given buyer, in insertion order.
    pub fn get_swaps_by_buyer(env: Env, buyer: Address) -> soroban_sdk::Vec<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::BuyerIndex(buyer))
            .unwrap_or_else(|| soroban_sdk::Vec::new(&env))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use ip_registry::{IpRegistry, IpRegistryClient};
    use soroban_sdk::{testutils::Address as _, token, Bytes, Env};

    #[test]
    fn test_get_swap_status_returns_none_for_missing_swap() {
        let env = Env::default();
        let contract_id = env.register(AtomicSwap, ());
        let client = AtomicSwapClient::new(&env, &contract_id);
        assert_eq!(client.get_swap_status(&999), None);
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

        let usdc_admin = Address::generate(&env);
        let usdc_id = env.register_stellar_asset_contract_v2(usdc_admin.clone()).address();
        let usdc_admin_client = token::StellarAssetClient::new(&env, &usdc_id);
        let usdc_client = token::Client::new(&env, &usdc_id);

        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);
        let zk_verifier = Address::generate(&env);
        usdc_admin_client.mint(&buyer, &1000);

        // Register listing with seller as owner
        let registry_id = env.register(IpRegistry, ());
        let registry = IpRegistryClient::new(&env, &registry_id);
        let listing_id = registry.register_ip(
            &seller,
            &Bytes::from_slice(&env, b"QmHash"),
            &Bytes::from_slice(&env, b"root"),
        );

        let contract_id = env.register(AtomicSwap, ());
        let client = AtomicSwapClient::new(&env, &contract_id);

        let swap_id = client.initiate_swap(&listing_id, &buyer, &seller, &usdc_id, &500, &zk_verifier, &registry_id);

        let key = Bytes::from_slice(&env, b"super-secret-key");
        client.confirm_swap(&swap_id, &key);

        assert_eq!(client.get_decryption_key(&swap_id), Some(key));
        assert_eq!(usdc_client.balance(&seller), 500);
    }

    #[test]
    #[should_panic(expected = "seller does not own this listing")]
    fn test_seller_impersonation_rejected() {
        let env = Env::default();
        env.mock_all_auths();

        let usdc_admin = Address::generate(&env);
        let usdc_id = env.register_stellar_asset_contract_v2(usdc_admin.clone()).address();
        token::StellarAssetClient::new(&env, &usdc_id).mint(&Address::generate(&env), &1000);

        let buyer = Address::generate(&env);
        let real_seller = Address::generate(&env);
        let impersonator = Address::generate(&env);
        let zk_verifier = Address::generate(&env);

        // Register listing with real_seller as owner
        let registry_id = env.register(IpRegistry, ());
        let registry = IpRegistryClient::new(&env, &registry_id);
        let listing_id = registry.register_ip(
            &real_seller,
            &Bytes::from_slice(&env, b"QmHash"),
            &Bytes::from_slice(&env, b"root"),
        );

        token::StellarAssetClient::new(&env, &usdc_id).mint(&buyer, &1000);

        let contract_id = env.register(AtomicSwap, ());
        let client = AtomicSwapClient::new(&env, &contract_id);

        // impersonator tries to pose as seller
        client.initiate_swap(&listing_id, &buyer, &impersonator, &usdc_id, &500, &zk_verifier, &registry_id);
    }

    // ── helper ────────────────────────────────────────────────────────────────
    /// Sets up a full swap environment: USDC token, mints `usdc_amount` to `buyer`,
    /// registers one IP listing owned by `seller`, and registers the AtomicSwap contract.
    /// Returns `(usdc_id, listing_id, registry_id, contract_id, client)`.
    fn setup_swap_env<'a>(
        env: &'a Env,
        buyer: &Address,
        seller: &Address,
        usdc_amount: i128,
    ) -> (Address, u64, Address, Address, AtomicSwapClient<'a>) {
        let usdc_admin = Address::generate(env);
        let usdc_id = env
            .register_stellar_asset_contract_v2(usdc_admin.clone())
            .address();
        token::StellarAssetClient::new(env, &usdc_id).mint(buyer, &usdc_amount);

        let registry_id = env.register(IpRegistry, ());
        let registry = IpRegistryClient::new(env, &registry_id);
        let listing_id = registry.register_ip(
            seller,
            &Bytes::from_slice(env, b"QmHash"),
            &Bytes::from_slice(env, b"root"),
        );

        let contract_id = env.register(AtomicSwap, ());
        let client = AtomicSwapClient::new(env, &contract_id);
        (usdc_id, listing_id, registry_id, contract_id, client)
    }

    // ── 5.1 ───────────────────────────────────────────────────────────────────
    #[test]
    fn test_get_swaps_by_buyer_empty() {
        let env = Env::default();
        let contract_id = env.register(AtomicSwap, ());
        let client = AtomicSwapClient::new(&env, &contract_id);

        let stranger = Address::generate(&env);
        let result = client.get_swaps_by_buyer(&stranger);
        assert_eq!(result.len(), 0);
    }

    // ── 5.2 ───────────────────────────────────────────────────────────────────
    #[test]
    fn test_get_swaps_by_buyer_single() {
        let env = Env::default();
        env.mock_all_auths();

        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);
        let zk_verifier = Address::generate(&env);

        let (usdc_id, listing_id, registry_id, _contract_id, client) =
            setup_swap_env(&env, &buyer, &seller, 500);

        let swap_id = client.initiate_swap(
            &listing_id, &buyer, &seller, &usdc_id, &500, &zk_verifier, &registry_id,
        );

        let ids = client.get_swaps_by_buyer(&buyer);
        assert_eq!(ids.len(), 1);
        assert_eq!(ids.get(0).unwrap(), swap_id);
    }

    // ── 5.3 ───────────────────────────────────────────────────────────────────
    #[test]
    fn test_get_swaps_by_buyer_multiple() {
        let env = Env::default();
        env.mock_all_auths();

        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);
        let zk_verifier = Address::generate(&env);

        // Mint enough for 3 swaps of 500 each
        let (usdc_id, listing_id, registry_id, _contract_id, client) =
            setup_swap_env(&env, &buyer, &seller, 1500);

        let id1 = client.initiate_swap(
            &listing_id, &buyer, &seller, &usdc_id, &500, &zk_verifier, &registry_id,
        );
        let id2 = client.initiate_swap(
            &listing_id, &buyer, &seller, &usdc_id, &500, &zk_verifier, &registry_id,
        );
        let id3 = client.initiate_swap(
            &listing_id, &buyer, &seller, &usdc_id, &500, &zk_verifier, &registry_id,
        );

        let ids = client.get_swaps_by_buyer(&buyer);
        assert_eq!(ids.len(), 3);
        assert_eq!(ids.get(0).unwrap(), id1);
        assert_eq!(ids.get(1).unwrap(), id2);
        assert_eq!(ids.get(2).unwrap(), id3);
    }

    // ── 5.4 ───────────────────────────────────────────────────────────────────
    #[test]
    fn test_buyer_index_isolation() {
        let env = Env::default();
        env.mock_all_auths();

        let buyer_a = Address::generate(&env);
        let buyer_b = Address::generate(&env);
        let seller = Address::generate(&env);
        let zk_verifier = Address::generate(&env);

        // Set up for buyer_a
        let (usdc_id, listing_id_a, registry_id, _contract_id, client) =
            setup_swap_env(&env, &buyer_a, &seller, 500);

        // Mint USDC for buyer_b using the same token
        token::StellarAssetClient::new(&env, &usdc_id).mint(&buyer_b, &500);

        // Register a second listing for buyer_b's swap
        let registry = IpRegistryClient::new(&env, &registry_id);
        let listing_id_b = registry.register_ip(
            &seller,
            &Bytes::from_slice(&env, b"QmHash2"),
            &Bytes::from_slice(&env, b"root2"),
        );

        let id_a = client.initiate_swap(
            &listing_id_a, &buyer_a, &seller, &usdc_id, &500, &zk_verifier, &registry_id,
        );
        let id_b = client.initiate_swap(
            &listing_id_b, &buyer_b, &seller, &usdc_id, &500, &zk_verifier, &registry_id,
        );

        let ids_a = client.get_swaps_by_buyer(&buyer_a);
        assert_eq!(ids_a.len(), 1);
        assert_eq!(ids_a.get(0).unwrap(), id_a);

        let ids_b = client.get_swaps_by_buyer(&buyer_b);
        assert_eq!(ids_b.len(), 1);
        assert_eq!(ids_b.get(0).unwrap(), id_b);
    }

    // ── 5.5 ───────────────────────────────────────────────────────────────────
    #[test]
    fn test_buyer_index_consistency_roundtrip() {
        let env = Env::default();
        env.mock_all_auths();

        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);
        let zk_verifier = Address::generate(&env);

        let (usdc_id, listing_id, registry_id, _contract_id, client) =
            setup_swap_env(&env, &buyer, &seller, 1000);

        client.initiate_swap(
            &listing_id, &buyer, &seller, &usdc_id, &500, &zk_verifier, &registry_id,
        );
        client.initiate_swap(
            &listing_id, &buyer, &seller, &usdc_id, &500, &zk_verifier, &registry_id,
        );

        let ids = client.get_swaps_by_buyer(&buyer);
        assert_eq!(ids.len(), 2);
        for i in 0..ids.len() {
            let id = ids.get(i).unwrap();
            assert!(
                client.get_swap_status(&id).is_some(),
                "swap_id {} has no corresponding swap record",
                id
            );
        }
    }
}
