use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};
use handlebars::Handlebars;
use handlebars::JsonValue;
use serde_json::json;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Cursor, Write};
use std::process::Command;
use std::str::FromStr;
use subxt::utils;
use subxt::{Error, OnlineClient, PolkadotConfig};
use chrono::prelude::*;

#[subxt::subxt(runtime_metadata_path = "./artifacts/polkadot_metadata_small.scale")]
pub mod polkadot {}

const BASE_LOCK_PERIOD: u32 = 28; // 28 days
const PLANCKS_PER_DOT: f64 = 1e10;
const MINUTES_PER_HOUR: i64 = 60;
const HOURS_PER_DAY: i64 = 24;
const SECONDS_PER_BLOCK: i64 = 6;
const BLOCKS_TO_MINUTES_FACTOR: i64 = SECONDS_PER_BLOCK / 60; // This combines the constants
const GENESIS_THRESHOLD: u32 = 9000000; // use a block number closer to genesis for early block time calculations

fn get_conviction_multiplier(conviction: u8) -> u32 {
    match conviction {
        0..=6 => 1 << conviction,
        _ => panic!("Unknown conviction value: {}", conviction),
    }
}

fn plancks_to_dots<T: Into<f64>>(plancks: T) -> f64 {
    plancks.into() / PLANCKS_PER_DOT
}

fn create_datetime_from_ymd(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> DateTime<Utc> {
    NaiveDate::from_ymd(year, month, day)
        .and_hms(hour, minute, second)
        .and_utc()
}

struct LockedInterval {
    start_date: DateTime<Utc>,
    end_date: DateTime<Utc>,
    amount: f64,
}

impl LockedInterval {
    fn overlaps_with(&self, start: &DateTime<Utc>, end: &DateTime<Utc>) -> bool {
        !(self.end_date < *start || self.start_date > *end)
    }
}

fn calculate_end_datetime(
    base_block: u32,
    current_block: u32,
    conviction: u8,
) -> (DateTime<Utc>, DateTime<Utc>) {
    // The current_block_datetime is the current UTC time
    let current_block_datetime = Utc::now();

    // Calculate the difference in blocks and convert it into a time difference
    let block_diff = (current_block - base_block) as i64;
    let time_diff = Duration::seconds(block_diff * SECONDS_PER_BLOCK);

    // Subtracting the time difference from the current time gives us the base_block_datetime
    let base_block_datetime = current_block_datetime - time_diff;

    let conviction_multiplier = get_conviction_multiplier(conviction) as i64;
    let lock_period_in_minutes =
        BASE_LOCK_PERIOD as i64 * conviction_multiplier * HOURS_PER_DAY * MINUTES_PER_HOUR;

    let end_datetime = base_block_datetime + Duration::minutes(lock_period_in_minutes);
    (current_block_datetime, end_datetime)
}

fn update_lock_dates(
    locked_intervals: &mut Vec<LockedInterval>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    amount: f64,
) {
    locked_intervals.push(LockedInterval {
        start_date: start,
        end_date: end,
        amount,
    });
}

async fn gather_and_cross_reference(
    api: &OnlineClient<PolkadotConfig>,
    key: &utils::AccountId32,
) -> Result<JsonValue, Box<dyn std::error::Error>> {
    // Initialize default values
    let mut liquidity_data = json!({});
    let mut locked_intervals = Vec::new();

    // Try fetching class locks and process them if available
    if let Some(class_locks_data) = fetch_class_locks(api, key).await? {
        let class_locks = class_locks_data.0.as_slice();

        let current_block_number = fetch_current_block_number(api).await?;
        locked_intervals = process_class_locks(api, key, class_locks, current_block_number).await?;
        liquidity_data = display_liquidity_ladder(&locked_intervals)?;
    }

    let lock_totals_data = display_lock_totals(api, key).await?;
    let vesting_data = display_vesting_info(api, key).await?;

    // Combine data and return
    Ok(json!({
        "liquidity": liquidity_data,
        "locks": lock_totals_data,
        "vesting": vesting_data,
    }))
}

async fn fetch_current_block_number(
    api: &OnlineClient<PolkadotConfig>,
) -> Result<u32, Box<dyn std::error::Error>> {
    let mut blocks_sub = api.blocks().subscribe_finalized().await?;
    if let Some(block) = blocks_sub.next().await {
        Ok(block?.header().number)
    } else {
        Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Failed to fetch block.",
        )))
    }
}

async fn process_class_locks(
    api: &OnlineClient<PolkadotConfig>,
    key: &utils::AccountId32,
    class_locks: &[(u16, u128)],
    current_block_number: u32,
) -> Result<Vec<LockedInterval>, Box<dyn std::error::Error>> {
    let mut locked_intervals: Vec<LockedInterval> = Vec::new();

    for class_lock in class_locks {
        let votes_data = fetch_voting(api, key, class_lock.0).await?;

        if let Some(polkadot::runtime_types::pallet_conviction_voting::vote::Voting::Casting(
            casting,
        )) = votes_data
        {
            process_casting_votes(
                api,
                key,
                &casting,
                current_block_number,
                &mut locked_intervals,
            )
            .await?;
        }
    }

    Ok(locked_intervals)
}

async fn process_casting_votes(
    api: &OnlineClient<PolkadotConfig>,
    key: &utils::AccountId32,
    casting: &polkadot::runtime_types::pallet_conviction_voting::vote::Casting<u128, u32, u32>,
    current_block_number: u32,
    locked_intervals: &mut Vec<LockedInterval>,
) -> Result<(), Box<dyn std::error::Error>> {
    for (ref_num, vote_detail) in casting.votes.0.as_slice().iter() {
        let ref_data = fetch_referendum_info(api, key, *ref_num).await?;

        let block_number = match ref_data {
            Some(data) => match data {
                polkadot::runtime_types::pallet_referenda::types::ReferendumInfo::Ongoing(
                    status,
                ) => status.submitted,
                polkadot::runtime_types::pallet_referenda::types::ReferendumInfo::Approved(
                    block_number,
                    ..,
                ) => block_number,
                polkadot::runtime_types::pallet_referenda::types::ReferendumInfo::Rejected(
                    block_number,
                    ..,
                ) => block_number,
                polkadot::runtime_types::pallet_referenda::types::ReferendumInfo::Killed(
                    block_number,
                    ..,
                ) => block_number,
                polkadot::runtime_types::pallet_referenda::types::ReferendumInfo::Cancelled(
                    block_number,
                    ..,
                ) => block_number,
                polkadot::runtime_types::pallet_referenda::types::ReferendumInfo::TimedOut(
                    block_number,
                    ..,
                ) => block_number,
                _ => 0, // For other `ReferendumInfo` variants, if there are any
            },
            None => 0, // Handle the case where ref_data is None
        };

        if block_number != 0 {
            // Make sure we have a valid block_number
            if let polkadot::runtime_types::pallet_conviction_voting::vote::AccountVote::Standard { vote, balance } = vote_detail {
                let conviction = vote.0 % 128;
                let (base_block_date, end_datetime) = calculate_end_datetime(block_number, current_block_number, conviction);
                let locked_amount_in_dot = *balance as f64 / 1e10;
                update_lock_dates(locked_intervals, base_block_date, end_datetime, locked_amount_in_dot);
            }
        }
    }

    Ok(())
}

fn categorize_lock_period(end_date: DateTime<Utc>) -> &'static str {
    let duration_from_now_in_days = (end_date - Utc::now()).num_days();
    match duration_from_now_in_days {
        d if d <= 0 => "Locked 0 Days",
        1..=7 => "Locked 1-7 Days",
        8..=14 => "Locked 8-14 Days",
        15..=28 => "Locked 15-28 Days",
        29..=60 => "Locked 29-60 Days",
        _ => "Locked 60+ Days",
    }
}

fn display_liquidity_ladder(
    locked_intervals: &[LockedInterval],
) -> Result<JsonValue, Box<dyn std::error::Error>> {
    let mut categorized_amounts: HashMap<&'static str, (f64, DateTime<Utc>)> = HashMap::new();

    for interval in locked_intervals {
        let category = categorize_lock_period(interval.end_date);
        let entry = categorized_amounts.entry(category).or_default();
        println!(
            "Interval amount: {:.10}, End date: {}, Category: {}",
            interval.amount, interval.end_date, category
        );

        if interval.amount > entry.0
            || (f64::abs(interval.amount - entry.0) < f64::EPSILON && interval.end_date > entry.1)
        {
            *entry = (interval.amount, interval.end_date);
        }
    }

    let lock_order = [
        "Locked 0 Days",
        "Locked 1-7 Days",
        "Locked 8-14 Days",
        "Locked 15-28 Days",
        "Locked 29-60 Days",
        "Locked 60+ Days",
    ];

    let mut max_lock_amount = 0.0;
    let mut account_data = vec![];

    // Gather data to be passed to the template
    for &lock_category in lock_order.iter().rev() {
        if let Some(&(amount, _)) = categorized_amounts.get(lock_category) {
            if amount > max_lock_amount {
                max_lock_amount = amount;
            }

            let class = match lock_category {
                "Locked 0 Days" => "locked-0-days",
                "Locked 1-7 Days" => "locked-1-7-days",
                "Locked 8-14 Days" => "locked-8-14-days",
                "Locked 15-28 Days" => "locked-15-28-days",
                "Locked 29-60 Days" => "locked-29-60-days",
                _ => "locked-60-plus-days",
            };
            println!(
                "Lock Category: {}, Amount: {:.10}, Class: {}",
                lock_category, amount, class
            );
            account_data.push(json!({
                "lock_category": lock_category,
                "amount": format!("{:.10}", amount),
                "class": class.to_string(),
            }));
        } else {
            account_data.push(json!({
                "lock_category": lock_category,
                "amount": "none",
                "class": "none",
            }));
        }
    }
    let account_data_for_address = json!({
        "locks": account_data,
    });

    Ok(account_data_for_address)
}

fn generate_html_for_all_addresses(
    all_addresses_data: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let reg = Handlebars::new();
    let template_string = include_str!("../templates/liquidity_matrix.html");

    let mut cursor = Cursor::new(Vec::new());
    reg.render_template_to_write(&template_string, &all_addresses_data, &mut cursor)?;

    let rendered_html = String::from_utf8(cursor.into_inner())?;

    // Generate current date and time string
    let local: DateTime<Local> = Local::now();
    let timestamp_str = local.format("%Y-%m-%d_%H-%M-%S").to_string();

    // Create a filename with the current date and time
    let filename = format!("liquidity_matrix_all_addresses_{}.html", timestamp_str);

    let mut file = File::create(&filename)?;
    file.write_all(rendered_html.as_bytes())?;

    println!("Generated heatmap at {}", filename);
    Command::new("open")
        .arg(&filename)
        .status()?;

    Ok(())
}

async fn display_lock_totals(
    api: &OnlineClient<PolkadotConfig>,
    key: &utils::AccountId32,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(locks_data) = fetch_account_locks(api, key).await? {
        let locks = locks_data.0.as_slice();

        println!("Lock totals:");
        for lock in locks {
            if let Ok(id_str) = String::from_utf8(lock.id.to_vec()) {
                let amount_in_dot = lock.amount as f64 / 1e10;
                println!("Lock ID: {}, Amount: {:.10} DOT", id_str, amount_in_dot);
            } else {
                println!("Failed to convert lock id to string");
            }
        }
    }

    Ok(())
}
/*
async fn gather_detailed_vote_info(
    api: &OnlineClient<PolkadotConfig>,
    key: &utils::AccountId32,
) -> Result<(), Box<dyn std::error::Error>> {

let class_locks_opt = fetch_class_locks(api, key).await?;
if let Some(class_locks_data) = class_locks_opt {
    let class_locks = class_locks_data.0.as_slice();

    for class_lock in class_locks {
        let votes_data = fetch_voting(api, key, class_lock.0).await?;

        if let polkadot::runtime_types::pallet_conviction_voting::vote::Voting::Casting(casting) =
            votes_data
        {
            let mut referendums_with_details = vec![];

            for (ref_num, vote_detail) in casting.votes.0.as_slice().iter() {
                let ref_data = fetch_referendum_info(api, key, *ref_num).await?;

                let (message, block_number) = match &ref_data {
                    polkadot::runtime_types::pallet_referenda::types::ReferendumInfo::Ongoing(
                        status,
                    ) => {
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
                    }
                    polkadot::runtime_types::pallet_referenda::types::ReferendumInfo::Approved(
                        block_number,
                        ..,
                    ) => (
                        format!("Referendum: {}, was accepted.", ref_num),
                        *block_number,
                    ),
                    polkadot::runtime_types::pallet_referenda::types::ReferendumInfo::Rejected(
                        block_number,
                        ..,
                    ) => (
                        format!("Referendum: {}, was rejected.", ref_num),
                        *block_number,
                    ),
                    polkadot::runtime_types::pallet_referenda::types::ReferendumInfo::Killed(
                        block_number,
                        ..,
                    ) => (
                        format!("Referendum: {}, was killed.", ref_num),
                        *block_number,
                    ),
                    polkadot::runtime_types::pallet_referenda::types::ReferendumInfo::Cancelled(
                        block_number,
                        ..,
                    ) => (
                        format!("Referendum: {}, was cancelled.", ref_num),
                        *block_number,
                    ),
                    polkadot::runtime_types::pallet_referenda::types::ReferendumInfo::TimedOut(
                        block_number,
                        ..,
                    ) => (
                        format!("Referendum: {}, timed out.", ref_num),
                        *block_number,
                    ),
                    _ => (format!("Referendum: {}, had unknown status.", ref_num), 0),
                };

                //println!("Block Number: {}", block_number); // Print block number here
                referendums_with_details.push(message);
            }
            for info in &referendums_with_details {
                println!("{}", info);
            }
        }
}
    }

    Ok(())
}
*/
async fn fetch_account_balance(
    api: &OnlineClient<PolkadotConfig>,
    key: &utils::AccountId32,
) -> Result<
    Option<polkadot::runtime_types::pallet_balances::types::AccountData<u128>>,
    Box<subxt::Error>,
> {
    let storage_query = polkadot::storage().balances().account(key);

    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            println!("[balances.account] {:?}", value);
            Ok(Some(value))
        }
        Ok(None) => Ok(None),
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
    Option<
        polkadot::runtime_types::bounded_collections::weak_bounded_vec::WeakBoundedVec<
            polkadot::runtime_types::pallet_balances::types::BalanceLock<u128>,
        >,
    >,
    Box<subxt::Error>,
> {
    let storage_query = polkadot::storage().balances().locks(key);

    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            //    println!("[balances.lock] {:?}", value);
            Ok(Some(value))
        }
        Ok(None) => Ok(None),
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
    Option<
        polkadot::runtime_types::pallet_conviction_voting::vote::Voting<
            u128,
            utils::AccountId32,
            u32,
            u32,
        >,
    >,
    Box<subxt::Error>,
> {
    let storage_query = polkadot::storage()
        .conviction_voting()
        .voting_for(key, lock_class);

    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            //println!("[conviction_voting.voting_for] {:?}", value);
            Ok(Some(value))
        }
        Ok(None) => Ok(None),
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
    Option<polkadot::runtime_types::bounded_collections::bounded_vec::BoundedVec<(u16, u128)>>,
    Box<subxt::Error>,
> {
    let storage_query = polkadot::storage().conviction_voting().class_locks_for(key);

    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            //println!("[Class locks data] {:?}", value);
            Ok(Some(value))
        }
        Ok(None) => Ok(None),
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
    Option<
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
    >,
    Box<subxt::Error>,
> {
    let storage_query = polkadot::storage().referenda().referendum_info_for(ref_num);

    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            //    println!("[Referendum Data] {:?}", value);
            Ok(Some(value))
        }
        Ok(None) => Ok(None),
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
    Option<
        polkadot::runtime_types::bounded_collections::bounded_vec::BoundedVec<
            polkadot::runtime_types::pallet_vesting::vesting_info::VestingInfo<u128, u32>,
        >,
    >,
    Box<subxt::Error>,
> {
    let storage_query = polkadot::storage().vesting().vesting(key);

    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            //println!("[Vesting Data] {:?}", value);
            Ok(Some(value))
        }
        Ok(None) => {
            // If no vesting data is found, simply return None instead of an error
            Ok(None)
        }
        Err(e) => {
            eprintln!("[Error] Fetching failed for vesting: {}", e);
            Err(Box::new(e))
        }
    }
}

fn calculate_vesting_datetimes(
    starting_block: u32,
    total_blocks_until_vested: u32,
    current_block: u32,
) -> (DateTime<Utc>, DateTime<Utc>) {
    let base_datetime = if starting_block < GENESIS_THRESHOLD {
        // Use the datetime for block 1 if within the threshold
        NaiveDate::from_ymd(2020, 5, 26)
            .and_hms(15, 36, 18)
            .and_utc()
    } else {
        // Use the original hardcoded datetime for later blocks
        NaiveDate::from_ymd(2023, 8, 25).and_hms(13, 1, 0).and_utc()
    };

    // Calculate difference in minutes between base_datetime and starting_block
    let minutes_diff_start = (starting_block as i64) * SECONDS_PER_BLOCK / MINUTES_PER_HOUR;
    let start_datetime = base_datetime + Duration::minutes(minutes_diff_start);

    // Calculate end datetime
    let minutes_diff_end =
        (total_blocks_until_vested) as i64 * SECONDS_PER_BLOCK / MINUTES_PER_HOUR;
    let end_datetime = start_datetime + Duration::minutes(minutes_diff_end);

    (start_datetime, end_datetime)
}
async fn display_vesting_info(
    api: &OnlineClient<PolkadotConfig>,
    key: &utils::AccountId32,
) -> Result<(), Box<dyn std::error::Error>> {
    let vesting_data_opt = fetch_vesting(api, key).await?;

    // If there's no vesting data, exit early
    if let Some(vesting_data) = vesting_data_opt {
        let mut blocks_sub = api.blocks().subscribe_finalized().await?;

        match blocks_sub.next().await {
            Some(block) => {
                let block = block?;
                let current_block_number = block.header().number;

                println!("Detailed Vesting Schedule:");

                for vesting_info in vesting_data.0.iter() {
                    let total_blocks_until_vested =
                        vesting_info.locked / vesting_info.per_block as u128;
                    let (start_date, end_date) = calculate_vesting_datetimes(
                        vesting_info.starting_block,
                        total_blocks_until_vested as u32,
                        current_block_number,
                    );

                    let locked_in_dot = vesting_info.locked as f64 / PLANCKS_PER_DOT;
                    let per_block_in_dot = vesting_info.per_block as f64 / PLANCKS_PER_DOT;

                    println!(
                        "Start Date: {}, Locked: {:.10} DOT, Per Block: {:.10} DOT, End Date: {}",
                        start_date.format("%Y-%m-%d %H:%M:%S"),
                        locked_in_dot,
                        per_block_in_dot,
                        end_date.format("%Y-%m-%d %H:%M:%S")
                    );
                }
            }
            None => {
                println!("No block data available.");
            }
        }
    } else {
        println!("No vesting data available for the account.");
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api = connect_to_polkadot_node().await?;
    let addresses = match read_addresses_from_input() {
        Ok(addrs) => {
            for address in &addrs {
                println!("{}", address);
            }
            addrs
        }
        Err(e) => {
            println!("Error: {}", e);
            return Err(e.into());
        }
    };
    let mut all_data = json!({
        "date": Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        "accounts": []
    });

    for address in &addresses {
        let data = process_address(&api, address).await?;
        all_data["accounts"].as_array_mut().unwrap().push(data);
    }
    generate_html_for_all_addresses(&all_data)?;

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

fn read_addresses_from_input() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let apple_script = r#"
    set defaultText to "








    "
    set userInput to text returned of (display dialog "Please input addresses, separated by newlines:" default answer defaultText)
    return userInput
    "#;

    let output = Command::new("osascript")
        .arg("-e")
        .arg(apple_script)
        .output()?;

    let user_input = String::from_utf8(output.stdout)?;

    // Split the input by newline, filter out any empty lines, and collect into a Vec<String>
    let addresses = user_input.lines().filter(|s| !s.trim().is_empty()).map(|s| s.to_string()).collect();
    Ok(addresses)
}
async fn process_address(
    api: &OnlineClient<PolkadotConfig>,
    address: &str,
) -> Result<JsonValue, Box<dyn std::error::Error>> {
    println!("\n[Processing] Address: {}", address);
    let public_key_bytes = utils::AccountId32::from_str(address)?;

    if let Err(e) = fetch_account_balance(&api, &public_key_bytes).await {
        eprintln!("[Error] Failed to fetch balance: {}", e);
    }
    if let Err(e) = fetch_account_locks(&api, &public_key_bytes).await {
        eprintln!("[Error] Failed to fetch locked balance: {}", e);
    }
    let xr_data = gather_and_cross_reference(&api, &public_key_bytes).await?;

    Ok(json!({
        "address": address,
        "data": xr_data,
    }))
}
