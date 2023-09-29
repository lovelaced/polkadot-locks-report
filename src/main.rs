use std::fs::File;
use std::io::{BufRead, BufReader};
use std::str::FromStr;
use subxt::utils;
use subxt::{Error, OnlineClient, PolkadotConfig};

#[subxt::subxt(runtime_metadata_path = "./artifacts/polkadot_metadata_small.scale")]
pub mod polkadot {}

fn get_conviction_multiplier(conviction: u8) -> u32 {
    match conviction {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 4,
        4 => 8,
        5 => 16,
        6 => 32,
        _ => panic!("Unknown conviction value: {}", conviction),
    }
}

const BASE_LOCK_PERIOD: u32 = 28; // 28 days

fn plancks_to_dots<T: Into<f64>>(plancks: T) -> f64 {
    const PLANCKS_PER_DOT: f64 = 1e10;
    plancks.into() / PLANCKS_PER_DOT
}

async fn gather_and_cross_reference(
    api: &OnlineClient<PolkadotConfig>,
    key: utils::AccountId32,
) -> Result<(), Box<dyn std::error::Error>> {
    let class_locks_data = fetch_class_locks(api, key.clone()).await?;
    let class_locks = class_locks_data.0.as_slice();

    for class_lock in class_locks {
        let votes_data = fetch_voting(api, key.clone(), class_lock.0).await?;

        if let polkadot::runtime_types::pallet_conviction_voting::vote::Voting::Casting(casting) =
            votes_data
        {
            let mut referendums_with_details = vec![];

            for (ref_num, vote_detail) in casting.votes.0.as_slice().iter() {
                let ref_data = fetch_referendum_info(api, key.clone(), *ref_num).await?;

                let tally = if let polkadot::runtime_types::pallet_referenda::types::ReferendumInfo::Ongoing(status) = &ref_data {
                    &status.tally
                } else {
                    // Handle the case when there's no Ongoing status or provide a default value
                    continue;
                };

                let ayes = tally.ayes as f64 / 1e10;
                let nays = tally.nays as f64 / 1e10;

                let detail = match vote_detail {
                    polkadot::runtime_types::pallet_conviction_voting::vote::AccountVote::Standard { vote, balance } => {
                        let conviction = vote.0 % 128;
                        let vote_type = if vote.0 >= 128 { "aye" } else { "nay" };
                        let amount_in_dot = *balance as f64 / 1e10;
                        format!("Referendum: {}, {}x conviction, Vote: {}, Amount: {:.10} DOT, Tally: Ayes: {:.10} DOT, Nays: {:.10} DOT", 
                                ref_num, conviction, vote_type, amount_in_dot, ayes, nays)
                    },
                    polkadot::runtime_types::pallet_conviction_voting::vote::AccountVote::Split { aye, nay } => {
                        let aye_amount_in_dot = *aye as f64 / 1e10;
                        let nay_amount_in_dot = *nay as f64 / 1e10;
                        format!("Referendum: {}, Split vote, Aye Amount: {:.10} DOT, Nay Amount: {:.10} DOT, Tally: Ayes: {:.10} DOT, Nays: {:.10} DOT", 
                                ref_num, aye_amount_in_dot, nay_amount_in_dot, ayes, nays)
                    },
                    polkadot::runtime_types::pallet_conviction_voting::vote::AccountVote::SplitAbstain { aye, nay, abstain } => {
                        let aye_amount_in_dot = *aye as f64 / 1e10;
                        let nay_amount_in_dot = *nay as f64 / 1e10;
                        let abstain_amount_in_dot = *abstain as f64 / 1e10;
                        format!("Referendum: {}, Split Abstain, Aye Amount: {:.10} DOT, Nay Amount: {:.10} DOT, Abstain Amount: {:.10} DOT, Tally: Ayes: {:.10} DOT, Nays: {:.10} DOT",
                               ref_num, aye_amount_in_dot, nay_amount_in_dot, abstain_amount_in_dot, ayes, nays)
                    },
                    _ => format!("Referendum: {}, unknown conviction, Tally: Ayes: {:.10} DOT, Nays: {:.10} DOT", ref_num, ayes, nays),
                };
                referendums_with_details.push(detail);
            }

            for info in &referendums_with_details {
                println!("{}", info);
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

async fn fetch_account_balance(
    api: &OnlineClient<PolkadotConfig>,
    key: utils::AccountId32,
) -> Result<polkadot::runtime_types::pallet_balances::types::AccountData<u128>, Box<subxt::Error>> {
    let storage_query = polkadot::storage().balances().account(key);

    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            println!("[balances.account] {:?}", value);
            Ok(value)
        }
        Ok(None) => Err(Box::new(subxt::Error::Other(
            "[balances] Not found for account".to_string(),
        ))),
        Err(e) => {
            eprintln!("[Error] Fetching failed for account balance: {}", e);
            Err(Box::new(e))
        }
    }
}

async fn fetch_account_locks(
    api: &OnlineClient<PolkadotConfig>,
    key: utils::AccountId32,
) -> Result<
    polkadot::runtime_types::bounded_collections::weak_bounded_vec::WeakBoundedVec<
        polkadot::runtime_types::pallet_balances::types::BalanceLock<u128>,
    >,
    Box<subxt::Error>,
> {
    let storage_query = polkadot::storage().balances().locks(key);

    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            println!("[balances.lock] {:?}", value);
            Ok(value)
        }
        Ok(None) => Err(Box::new(subxt::Error::Other(
            "[balances] Not found for address in conviction votes".to_string(),
        ))),
        Err(e) => {
            eprintln!("[Error] Fetching failed for conviction votes: {}", e);
            Err(Box::new(e))
        }
    }
}

async fn fetch_voting(
    api: &OnlineClient<PolkadotConfig>,
    key: utils::AccountId32,
    lock_class: u16,
) -> Result<
    polkadot::runtime_types::pallet_conviction_voting::vote::Voting<
        u128,
        utils::AccountId32,
        u32,
        u32,
    >,
    Box<subxt::Error>,
> {
    let storage_query = polkadot::storage()
        .conviction_voting()
        .voting_for(key, lock_class);

    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            //println!("[conviction_voting.voting_for] {:?}", value);
            Ok(value)
        }
        Ok(None) => Err(Box::new(subxt::Error::Other(
            "[voting] Not found for address in conviction votes".to_string(),
        ))),
        Err(e) => {
            eprintln!("[Error] Fetching failed for conviction votes: {}", e);
            Err(Box::new(e))
        }
    }
}

async fn fetch_class_locks(
    api: &OnlineClient<PolkadotConfig>,
    key: utils::AccountId32,
) -> Result<
    polkadot::runtime_types::bounded_collections::bounded_vec::BoundedVec<(u16, u128)>,
    Box<subxt::Error>,
> {
    let storage_query = polkadot::storage().conviction_voting().class_locks_for(key);

    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            println!("[Class locks data] {:?}", value);
            Ok(value)
        }
        Ok(None) => Err(Box::new(subxt::Error::Other(
            "[voting] Not found for address in class locks".to_string(),
        ))),
        Err(e) => {
            eprintln!("[Error] Fetching failed for class locks: {}", e);
            Err(Box::new(e))
        }
    }
}

async fn fetch_referendum_info(
    api: &OnlineClient<PolkadotConfig>,
    key: utils::AccountId32,
    ref_num: u32,
) -> Result<
    polkadot::runtime_types::pallet_referenda::types::ReferendumInfo<
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
    >,
    Box<subxt::Error>,
> {
    let storage_query = polkadot::storage().referenda().referendum_info_for(ref_num);

    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            println!("[Referendum Data] {:?}", value);
            Ok(value)
        }
        Ok(None) => Err(Box::new(subxt::Error::Other(
            "[referenda] Not found in referenda".to_string(),
        ))),
        Err(e) => {
            eprintln!("[Error] Fetching failed for referenda: {}", e);
            Err(Box::new(e))
        }
    }
}

async fn fetch_vesting(
    api: &OnlineClient<PolkadotConfig>,
    key: utils::AccountId32,
) -> Result<
    polkadot::runtime_types::bounded_collections::bounded_vec::BoundedVec<
        polkadot::runtime_types::pallet_vesting::vesting_info::VestingInfo<u128, u32>,
    >,
    Box<subxt::Error>,
> {
    let storage_query = polkadot::storage().vesting().vesting(key);

    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            println!("[Vesting Data] {:?}", value);
            Ok(value)
        }
        Ok(None) => Err(Box::new(subxt::Error::Other(
            "[vesting] Not found in vesting".to_string(),
        ))),
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

async fn connect_to_polkadot_node(
) -> Result<OnlineClient<PolkadotConfig>, Box<dyn std::error::Error>> {
    println!("[Connection] Attempting to connect to 'wss://rpc.polkadot.io:443'...");
    OnlineClient::<PolkadotConfig>::from_url("wss://rpc.polkadot.io:443")
        .await
        .map_err(Into::into)
}

fn read_addresses_from_file(path: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    BufReader::new(File::open(path)?)
        .lines()
        .collect::<Result<_, _>>()
        .map_err(Into::into)
}

async fn process_address(
    api: &OnlineClient<PolkadotConfig>,
    address: &str,
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
