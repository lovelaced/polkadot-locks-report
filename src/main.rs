use std::fs::File;
use std::io::{BufRead, BufReader};
use subxt::{OnlineClient, PolkadotConfig, Error};
use subxt::utils;
use std::str::FromStr;

#[subxt::subxt(runtime_metadata_path = "./artifacts/polkadot_metadata_small.scale")]
pub mod polkadot {}

fn plancks_to_dots<T: Into<f64>>(plancks: T) -> f64 {
    const PLANCKS_PER_DOT: f64 = 1e10;
    plancks.into() / PLANCKS_PER_DOT
}

async fn gather_and_cross_reference(api: &OnlineClient<PolkadotConfig>, key: utils::AccountId32) -> Result<(), Box<dyn std::error::Error>> {
    let class_locks_data = fetch_class_locks(api, key.clone()).await?;
    let class_locks = class_locks_data.0.as_slice();
    for class_lock in class_locks {
        println!("Class lock: {:?}", class_lock.0);
        let votes_data = fetch_voting(api, key.clone(), class_lock.0).await?;
        if let polkadot::runtime_types::pallet_conviction_voting::vote::Voting::Casting(casting) = votes_data {
            let referendums_with_convictions: Vec<_> = casting.votes.0.as_slice().iter()
                .map(|(ref_num, vote_detail)| {
                    let conviction = match vote_detail {
                        polkadot::runtime_types::pallet_conviction_voting::vote::AccountVote::Standard { vote, .. } => format!("{}x conviction", vote.0),
                        // You can extend this match for other vote detail variants, if they exist.
                        _ => "unknown conviction".to_string(),
                    };
                    (ref_num, conviction)
                })
                .collect();

            for (ref_num, conviction) in &referendums_with_convictions {
                println!("Referendum: {}, {}", ref_num, conviction);
            }
        }
    }
    let locks_data = fetch_account_locks(api, key.clone()).await?;
    let locks = locks_data.0.as_slice();
    // Access the locks inside the WeakBoundedVec and print them
    for lock in locks {
        if let Ok(id_str) = String::from_utf8(lock.id.to_vec()) {
            let amount_in_dot = lock.amount as f64 / 1e10;
            println!("Lock ID: {}, Amount: {:.10} DOT", id_str, amount_in_dot);
        } else {
            println!("Failed to convert lock id to string");
        }
    }
    Ok(())
}

async fn fetch_account_balance(api: &OnlineClient<PolkadotConfig>, key: utils::AccountId32) -> Result<polkadot::runtime_types::pallet_balances::types::AccountData<u128>, Box<subxt::Error>> {
    let storage_query = polkadot::storage().balances().account(key);
    
    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            println!("[balances.account] {:?}", value);
            Ok(value)
        }
        Ok(None) => Err(Box::new(subxt::Error::Other("[balances] Not found for account".to_string()))),
        Err(e) => {
            eprintln!("[Error] Fetching failed for account balance: {}", e);
            Err(Box::new(e))
        }
    }
}

async fn fetch_account_locks(api: &OnlineClient<PolkadotConfig>, key: utils::AccountId32) -> Result<polkadot::runtime_types::bounded_collections::weak_bounded_vec::WeakBoundedVec<
    polkadot::runtime_types::pallet_balances::types::BalanceLock<u128>>, Box<subxt::Error>> {
    let storage_query = polkadot::storage().balances().locks(key);
    
    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            println!("[balances.lock] {:?}", value);
            Ok(value)
        }
        Ok(None) => Err(Box::new(subxt::Error::Other("[balances] Not found for address in conviction votes".to_string()))),
        Err(e) => {
            eprintln!("[Error] Fetching failed for conviction votes: {}", e);
            Err(Box::new(e))
        }
    }
}

async fn fetch_voting(api: &OnlineClient<PolkadotConfig>, key: utils::AccountId32, lock_class: u16) -> Result<polkadot::runtime_types::pallet_conviction_voting::vote::Voting<u128, utils::AccountId32, u32, u32>, Box<subxt::Error>> {
    let storage_query = polkadot::storage().conviction_voting().voting_for(key, lock_class);
    
    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            //println!("[conviction_voting.voting_for] {:?}", value);
            Ok(value)
        }
        Ok(None) => Err(Box::new(subxt::Error::Other("[voting] Not found for address in conviction votes".to_string()))),
        Err(e) => {
            eprintln!("[Error] Fetching failed for conviction votes: {}", e);
            Err(Box::new(e))
        }
    }
}

async fn fetch_class_locks(api: &OnlineClient<PolkadotConfig>, key: utils::AccountId32) -> Result<polkadot::runtime_types::bounded_collections::bounded_vec::BoundedVec<(u16, u128)>, Box<subxt::Error>> {
    let storage_query = polkadot::storage().conviction_voting().class_locks_for(key);
    
    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            println!("[Class locks data] {:?}", value);
            Ok(value)
        }
        Ok(None) => Err(Box::new(subxt::Error::Other("[voting] Not found for address in class locks".to_string()))),
        Err(e) => {
            eprintln!("[Error] Fetching failed for class locks: {}", e);
            Err(Box::new(e))
        }
    }
}

async fn fetch_referendum_info(api: &OnlineClient<PolkadotConfig>, key: utils::AccountId32, ref_num: u32) -> Result<polkadot::runtime_types::pallet_referenda::types::ReferendumInfo<
    u16,
    polkadot::runtime_types::polkadot_runtime::OriginCaller,
    u32,
    polkadot::runtime_types::frame_support::traits::preimages::Bounded<
        polkadot::runtime_types::polkadot_runtime::RuntimeCall,
    >,
    u128,
    polkadot::runtime_types::pallet_conviction_voting::types::Tally<u128>,
    utils::AccountId32,
    (u32, u32),
>, Box<subxt::Error>> {
    let storage_query = polkadot::storage().referenda().referendum_info_for(ref_num);
    
    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            println!("[Referendum Data] {:?}", value);
            Ok(value)
        },
        Ok(None) => Err(Box::new(subxt::Error::Other("[referenda] Not found in referenda".to_string()))),
        Err(e) => {
            eprintln!("[Error] Fetching failed for referenda: {}", e);
            Err(Box::new(e))
        }
    }
}

async fn fetch_vesting(api: &OnlineClient<PolkadotConfig>, key: utils::AccountId32) -> Result<polkadot::runtime_types::bounded_collections::bounded_vec::BoundedVec<
    polkadot::runtime_types::pallet_vesting::vesting_info::VestingInfo<u128, u32>,
>, Box<subxt::Error>> {
    let storage_query = polkadot::storage().vesting().vesting(key);
    
    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            println!("[Vesting Data] {:?}", value);
            Ok(value)
        },
        Ok(None) => Err(Box::new(subxt::Error::Other("[vesting] Not found in vesting".to_string()))),
        Err(e) => {
            eprintln!("[Error] Fetching failed for vesting: {}", e);
            Err(Box::new(e))
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api = connect_to_polkadot_node().await?;
    let addresses = read_addresses_from_file("addresses.txt")?;

    for address in &addresses {
        process_address(&api, &address).await?;
    }

    println!("\n[Completion] Finished processing all addresses.");
    Ok(())
}

async fn connect_to_polkadot_node() -> Result<OnlineClient<PolkadotConfig>, Box<dyn std::error::Error>> {
    println!("[Connection] Attempting to connect to 'wss://rpc.polkadot.io:443'...");
    OnlineClient::<PolkadotConfig>::from_url("wss://rpc.polkadot.io:443").await.map_err(Into::into)
}

fn read_addresses_from_file(path: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    BufReader::new(File::open(path)?).lines().collect::<Result<_, _>>().map_err(Into::into)
}

async fn process_address(
    api: &OnlineClient<PolkadotConfig>,
    address: &str
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n[Processing] Address: {}", address);
    let public_key_bytes = utils::AccountId32::from_str(address)?;
    if let Err(e) = fetch_account_balance(&api, public_key_bytes.clone()).await {
        eprintln!("[Error] Failed to fetch balance: {}", e);
    }
    if let Err(e) = fetch_account_locks(&api, public_key_bytes.clone()).await {
        eprintln!("[Error] Failed to fetch locked balance: {}", e);
    }
//
//    println!("[Vesting Balance] Fetching...");
//    if let Err(e) = fetch_and_print_vesting(&api, public_key_bytes.clone()).await {
//        eprintln!("[Error] Failed to fetch vesting balance: {}", e);
//    }
//    if let Err(e) = fetch_class_locks(&api, public_key_bytes.clone()).await {
//        eprintln!("[Error] Failed to fetch class locks: {}", e);
//    }
    if let Err(e) = gather_and_cross_reference(&api, public_key_bytes.clone()).await {
        eprintln!("[Error] Failed to xr: {}", e);
    }
  Ok(())
}

