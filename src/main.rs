use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::str::FromStr;
use subxt::utils;
use subxt::{Error, OnlineClient, PolkadotConfig};

#[subxt::subxt(runtime_metadata_path = "./artifacts/polkadot_metadata_small.scale")]
pub mod polkadot {}

fn get_conviction_multiplier(conviction: u8) -> u32 {
    match conviction {
        0..=6 => 1 << conviction,
        _ => panic!("Unknown conviction value: {}", conviction),
    }
}

fn categorize_lock_period(end_date: DateTime<Utc>) -> &'static str {
    let duration_from_now_in_days = (end_date - Utc::now()).num_days();
    match duration_from_now_in_days {
        0 => "Locked 0 Days",
        1..=7 => "Locked 1-7 Days",
        8..=14 => "Locked 8-14 Days",
        15..=28 => "Locked 15-28 Days",
        29..=60 => "Locked 29-60 Days",
        _ => "Locked 60+ Days",
    }
}

const BASE_LOCK_PERIOD: u32 = 28; // 28 days
const PLANCKS_PER_DOT: f64 = 1e10;

fn plancks_to_dots<T: Into<f64>>(plancks: T) -> f64 {
    plancks.into() / PLANCKS_PER_DOT
}

fn calculate_end_datetime(
    base_block: u32,
    current_block: u32,
    conviction: u8,
) -> (DateTime<Utc>, DateTime<Utc>) {
    const SECONDS_PER_BLOCK: i64 = 6;
    const MINUTES_PER_HOUR: i64 = 60;
    const HOURS_PER_DAY: i64 = 24;

    let base_datetime = NaiveDate::from_ymd_opt(2023, 8, 25)
        .expect("Invalid date")
        .and_hms_opt(13, 1, 0)
        .expect("Invalid time")
        .and_utc();

    let minutes_diff = (current_block - base_block) as i64 * SECONDS_PER_BLOCK / MINUTES_PER_HOUR;
    let current_block_datetime = base_datetime + Duration::minutes(minutes_diff);

    let conviction_multiplier = get_conviction_multiplier(conviction) as i64;
    let lock_period_in_minutes =
        BASE_LOCK_PERIOD as i64 * conviction_multiplier * HOURS_PER_DAY * MINUTES_PER_HOUR;

    let end_datetime = current_block_datetime + Duration::minutes(lock_period_in_minutes);
    (current_block_datetime, end_datetime)
}

// Helper function to calculate and update lock dates
fn update_lock_dates(
    lock_dates: &mut HashMap<NaiveDateTime, f64>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    amount: f64,
) {
    let mut current_date = start.date();
    let end_date = end.date();

    while current_date <= end_date {
        // Convert Date<Utc> to NaiveDateTime
        let naive_datetime = current_date.and_hms(0, 0, 0).naive_utc();

        let existing_amount = lock_dates.entry(naive_datetime).or_insert(0.0);
        *existing_amount = f64::max(*existing_amount, amount); // Use 'amount' here

        current_date = current_date + chrono::Duration::days(1); // Move to next day, else it will become an infinite loop
    }
    //println!("lock dates: {:?}", lock_dates);
}

async fn gather_and_cross_reference(
    api: &OnlineClient<PolkadotConfig>,
    key: &utils::AccountId32,
) -> Result<(), Box<dyn std::error::Error>> {
    let class_locks_data = fetch_class_locks(api, key).await?;
    let class_locks = class_locks_data.0.as_slice();

    let mut blocks_sub = api.blocks().subscribe_finalized().await?;
    let mut lock_dates: HashMap<NaiveDateTime, f64> = HashMap::new();

    // Fetch the current block
    if let Some(block) = blocks_sub.next().await {
        let block = block?;
        let current_block_number = block.header().number;

        for class_lock in class_locks {
            let votes_data = fetch_voting(api, key, class_lock.0).await?;

            if let polkadot::runtime_types::pallet_conviction_voting::vote::Voting::Casting(
                casting,
            ) = votes_data
            {
                let mut referendums_with_details = vec![];

                for (ref_num, vote_detail) in casting.votes.0.as_slice().iter() {
                    let ref_data = fetch_referendum_info(api, key, *ref_num).await?;

                    let (message, block_number) = match &ref_data {
                    polkadot::runtime_types::pallet_referenda::types::ReferendumInfo::Ongoing(status) => {
                        let ayes = status.tally.ayes as f64 / 1e10;
                        let nays = status.tally.nays as f64 / 1e10;

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
                            _ => format!("Referendum: {}, unknown conviction, Tally: Ayes: {:.10} DOT, Nays: {:.10} DOT", ref_num, ayes, nays)
                        };
                        (detail, status.submitted)
                    }, 
                    polkadot::runtime_types::pallet_referenda::types::ReferendumInfo::Approved(block_number, ..) => {
                        (format!("Referendum: {}, was accepted.", ref_num), *block_number)
                    },
                    polkadot::runtime_types::pallet_referenda::types::ReferendumInfo::Rejected(block_number, ..) => {
                        (format!("Referendum: {}, was rejected.", ref_num), *block_number)
                    },
                    polkadot::runtime_types::pallet_referenda::types::ReferendumInfo::Killed(block_number, ..) => {
                        (format!("Referendum: {}, was killed.", ref_num), *block_number)
                    },
                    polkadot::runtime_types::pallet_referenda::types::ReferendumInfo::Cancelled(block_number, ..) => {
                        (format!("Referendum: {}, was cancelled.", ref_num), *block_number)
                    },
                    polkadot::runtime_types::pallet_referenda::types::ReferendumInfo::TimedOut(block_number, ..) => {
                        (format!("Referendum: {}, timed out.", ref_num), *block_number)
                    }, 
                    _ => {
                        (format!("Referendum: {}, had unknown status.", ref_num), 0)
                    } 
                };

                    //println!("Block Number: {}", block_number); // Print block number here
                    referendums_with_details.push(message);
                    if let polkadot::runtime_types::pallet_referenda::types::ReferendumInfo::Ongoing(_) = &ref_data {
                if let polkadot::runtime_types::pallet_conviction_voting::vote::AccountVote::Standard { vote, balance } = vote_detail {
                    let conviction = vote.0 % 128;
                    let (current_block_date, end_datetime) = calculate_end_datetime(block_number, current_block_number, conviction);

                    let locked_amount_in_dot = *balance as f64 / PLANCKS_PER_DOT;
                    update_lock_dates(&mut lock_dates, current_block_date, end_datetime, locked_amount_in_dot);
                }

    }
                }

                    let mut categorized_amounts: HashMap<&'static str, f64> = HashMap::new();

                    // Step 2 & 3: Directly update categorized_amounts from lock_dates
                    for (&naive_date, &locked_amount) in &lock_dates {
                        // Convert NaiveDateTime to DateTime<Utc>
                        let date_in_utc = DateTime::<Utc>::from_utc(naive_date, Utc);
                        let category = categorize_lock_period(date_in_utc);

                        // Update the categorized_amounts if the new value is greater than the previous
                        let entry = categorized_amounts.entry(category).or_insert(0.0);
                        if locked_amount > *entry {
                            *entry = locked_amount;
                        }
                    }
                    for (category, &amount) in &categorized_amounts {
                        println!("{}: {:.10} DOT locked", category, amount);
                    }
                for info in &referendums_with_details {
                    println!("{}", info);
                }
            }
        }
    }

    let locks_data = fetch_account_locks(api, &key).await?;
    let locks = locks_data.0.as_slice();
    // Access the locks inside the WeakBoundedVec and print them
    for lock in locks {
        if let Ok(id_str) = String::from_utf8(lock.id.to_vec()) {
            let amount_in_dot = lock.amount as f64 / 1e10;
        //    println!("Lock ID: {}, Amount: {:.10} DOT", id_str, amount_in_dot);
        } else {
            println!("Failed to convert lock id to string");
        }
    }
    Ok(())
}

async fn fetch_account_balance(
    api: &OnlineClient<PolkadotConfig>,
    key: &utils::AccountId32,
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
    key: &utils::AccountId32,
) -> Result<
    polkadot::runtime_types::bounded_collections::weak_bounded_vec::WeakBoundedVec<
        polkadot::runtime_types::pallet_balances::types::BalanceLock<u128>,
    >,
    Box<subxt::Error>,
> {
    let storage_query = polkadot::storage().balances().locks(key);

    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            //    println!("[balances.lock] {:?}", value);
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
    key: &utils::AccountId32,
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
    key: &utils::AccountId32,
) -> Result<
    polkadot::runtime_types::bounded_collections::bounded_vec::BoundedVec<(u16, u128)>,
    Box<subxt::Error>,
> {
    let storage_query = polkadot::storage().conviction_voting().class_locks_for(key);

    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            //    println!("[Class locks data] {:?}", value);
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
    key: &utils::AccountId32,
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
            //    println!("[Referendum Data] {:?}", value);
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
    key: &utils::AccountId32,
) -> Result<
    polkadot::runtime_types::bounded_collections::bounded_vec::BoundedVec<
        polkadot::runtime_types::pallet_vesting::vesting_info::VestingInfo<u128, u32>,
    >,
    Box<subxt::Error>,
> {
    let storage_query = polkadot::storage().vesting().vesting(key);

    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            //    println!("[Vesting Data] {:?}", value);
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
    if let Err(e) = fetch_account_balance(&api, &public_key_bytes).await {
        eprintln!("[Error] Failed to fetch balance: {}", e);
    }
    if let Err(e) = fetch_account_locks(&api, &public_key_bytes).await {
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
    if let Err(e) = gather_and_cross_reference(&api, &public_key_bytes).await {
        eprintln!("[Error] Failed to xr: {}", e);
    }
    Ok(())
}
