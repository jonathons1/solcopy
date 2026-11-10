# solcopy

A copy-trading system for pump.fun on Solana, written in Rust. It follows a small set of public
trader wallets and buys a coin directly **behind** a trader's own buy, never ahead of it.

This repository is the public face of a private codebase. It holds a description of the system
and the source of its one on-chain program. The trading engine itself is not published.

## What it does

When a followed trader buys a newly launched coin, other buyers follow within the same block.
The system's job is to be the first of them: its buy must execute right behind the trader's.
It then sells on a fixed rule. It does nothing else: no sandwiching, no front-running, no
trading against the trader.

That last point is enforced on-chain, not by policy. Every buy the system sends carries a check
that fails unless a followed trader **already holds** the coin. A copy that reaches a leader
before the trader's buy has executed fails at that check, before any swap. The system cannot
buy ahead of the transaction it follows.

## How it is built

- **Language and size.** Rust, about 42,000 lines in four crates, about 200 tests. Private
  repository, 340 commits since 21 October 2026.
- **Seeing a trade.** Three streams are raced and the first to show a transaction is acted on:
  decoded shreds, leader preconfirmations (BAM leaders), and Harmonic's builder
  preconfirmations (through Triton). The Harmonic stream is read by a separate process, so the
  program that holds the wallet key carries no gRPC stack.
- **The guard.** A buy transaction with two conditions in front of it: the holding check above,
  and a slot ceiling after which it can no longer execute. A guarded copy can wait at the
  leader and can only ever execute behind the trader's buy, inside a bounded window.
- **`any-of`, the on-chain program** (in `programs/any-of`, deployed on mainnet at
  `G4Hv7XWncKeR8XwAExbm3ch4sJGwQtsGUCjujZkWx1sC`). It succeeds when at least one of the token
  accounts passed to it holds a minimum amount, and fails otherwise. One guarded copy can
  therefore cover several followed traders at once. It holds nothing, owns nothing, signs
  nothing and writes nothing; it has no dependency, and its source (about 200 lines with its
  tests) reads the loader's input directly.
- **Limits that fail closed.** The system starts paused and trades only when a person sets it
  running. Caps on trade size, fees a day, trades a day and loss a day; a floor on the wallet's
  balance; a pause that drops every open arming within 20 ms. Decoders refuse a layout they do
  not know. A feed frame of an unknown schema halts the system.
- **Everything is recorded.** Every feed message goes to disk with the time it arrived, and
  every decision is journaled with its timings, so any trade can be replayed and explained.

## Where it stands

Live on mainnet since 29 October 2026 at test size (0.05 SOL a trade), run by hand in short
sessions and measured after each. It is an engineering and measurement project, not yet a
profitable one, and nothing here claims otherwise.

What has been measured so far:

- From a trader's buy reaching the system to its copy being handed to the sender: a median of
  2.5 ms over the live reactions of 6 to 9 November 2026.
- A one-month read of the chain (4 October to 3 November 2026): every block in which one
  followed trader made a first buy, 2,214 blocks, read in full. Two findings matter here. Block
  order is close to arrival order on every builder. And on slots led by Harmonic validators,
  the place directly behind the trader's buy is most often taken by a transaction placed there
  with it, not by one that arrived fastest.

## Why the Harmonic Searcher Network

The second finding is the reason for the application. The strategy is exactly the one the
network names: copy trading, a single buy directly behind a user's transaction. On Harmonic's
slots that place appears to be reached through the network's auction rather than by speed, and
the system already carries what such a trade needs: it never executes ahead of the transaction it
follows, its copies are bounded in time, and its limits fail closed.

What would run on the network: one backrun buy behind a followed wallet's pump.fun buy, bid
from the measured value of that place, at small size during the pilot.

## Repository layout

    programs/any-of/    the on-chain check: source, lock file, tests

Build and test the program:

    cargo test --manifest-path programs/any-of/Cargo.toml

It is built for the chain with `cargo build-sbf`.
