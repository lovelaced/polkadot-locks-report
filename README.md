# locks-report

This program generates an HTML report of locks given a list of accounts, as well as having the capability of retrieving and printing detailed data on locks, referenda, voting, and vesting. It focuses on analyzing and processing voting data, referenda information, and account lock periods to generate a detailed report of active and expired locks.

![Screenshot 2023-11-03 at 14.58.31.png](https://github.com/lovelaced/polkadot-locks-report/blob/5934c404ae646de22fb8281d0e1688375b8ebc12/Screenshot%202023-11-03%20at%2014.58.31.png)

## Features

- Calculating lock periods based on vote conviction, staking, and vesting.
- Handling different vote types including standard, split, and abstain votes.
- Outputting detailed lock information associated with votes and referenda.

## Prerequisites

Before you begin, ensure you have met the following requirements:

- Rust programming environment.
- Cargo, Rust's package manager and build system.
- Access to a polkadot or kusama archive node (uses rpc.polkadot.io by default).

## Installation

Clone the repository to your local machine:

```bash
git clone https://github.com/lovelaced/locks-report.git
cd locks-report
```

Build the project using Cargo:

```bash
cargo build --release
```

## Usage

Only works on Mac at the moment.

## Configuration

Provide details on how to configure the environment, if necessary, including environment variables, configuration files, or command-line arguments.

## Output Interpretation

After running the program, you will receive an output consisting of detailed lock information. Here is how to interpret the key components:

- `balances.lock`: Shows the balance locks on accounts due to voting.
- `Class locks data`: Represents the locks on specific classes of assets.
- `conviction_voting.voting_for`: Provides information on the current votes and the conviction levels.
- `Vote Data`: Shows the individual votes that have been cast.
- `Referendum Data`: Details about ongoing referendums and their status.

## License

Distributed under the MIT License. See `LICENSE` for more information.
