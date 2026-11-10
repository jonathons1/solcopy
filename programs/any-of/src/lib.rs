//! any-of (5 Nov 2026): an on-chain check for the guard (see the README). It succeeds
//! when at least one of the token accounts passed to it holds `min` or more of its token, and
//! fails otherwise. The guard puts it in front of its buy with the token account of every
//! tracked trader for the coin, so one standing copy fires on the first of them to buy; a
//! Lighthouse assert can only look at one account.
//!
//! It holds nothing, owns nothing, signs nothing and writes nothing: it reads the accounts it
//! is given and answers yes or no.
//!
//! Instruction data: one tag byte (1), then `min` as a u64, little-endian; `min` is at least 1.
//! Accounts: one or more token accounts, read-only. The token amount is read at offset 64 of the
//! account's data, where both token programs keep it; an account that does not exist yet (no
//! data) or is shorter than 72 bytes counts as not holding. The caller passes token accounts.
//! Errors (custom codes): 1 none holds, 2 malformed instruction data, 3 no accounts.
//!
//! No dependency: the entrypoint reads the loader's own serialization of the accounts (the
//! aligned layout of the upgradeable BPF loader), so nothing but this file is compiled into it.

#![cfg_attr(target_os = "solana", no_std)]

pub const OK: u64 = 0;
pub const ERR_NONE_HOLDS: u64 = 1;
pub const ERR_DATA: u64 = 2;
pub const ERR_NO_ACCOUNTS: u64 = 3;

/// The instruction's tag byte.
pub const TAG: u8 = 1;

/// The marker of an account that is not a repeat of an earlier one.
const NOT_A_REPEAT: u8 = 0xFF;
/// Before an account's key: the repeat marker, is_signer, is_writable, executable, and four
/// bytes the loader keeps for itself.
const HEAD: usize = 8;
/// Key, owner, lamports, data length.
const BEFORE_DATA: usize = HEAD + 32 + 32 + 8 + 8;
/// The room the loader leaves after an account's data for it to grow.
const GROWTH: usize = 10_240;
/// Where a token account keeps its amount.
const AMOUNT_AT: usize = 64;

/// A u64, little-endian, at an offset of the input.
///
/// # Safety
/// `input.add(at)` must be readable for eight bytes.
unsafe fn u64_at(input: *const u8, at: usize) -> u64 {
    u64::from_le(core::ptr::read_unaligned(input.add(at) as *const u64))
}

/// The check, over the loader's serialized input: the count of accounts; each account (a repeat
/// is one marker byte and seven of padding); the instruction data's length and bytes.
///
/// # Safety
/// `input` must point to the loader's serialization of the instruction's accounts and data.
pub unsafe fn process(input: *const u8) -> u64 {
    let accounts = u64_at(input, 0) as usize;
    let mut at = 8;
    let mut most: u64 = 0;
    for _ in 0..accounts {
        if *input.add(at) != NOT_A_REPEAT {
            // The same account again: already looked at.
            at += 8;
            continue;
        }
        let data_len = u64_at(input, at + BEFORE_DATA - 8) as usize;
        let data = at + BEFORE_DATA;
        if data_len >= AMOUNT_AT + 8 {
            let amount = u64_at(input, data + AMOUNT_AT);
            if amount > most {
                most = amount;
            }
        }
        // Past the data and its growth room, up to the next multiple of eight, then the rent
        // epoch.
        at = ((data + data_len + GROWTH + 7) & !7) + 8;
    }
    let data_len = u64_at(input, at) as usize;
    let data = at + 8;
    if data_len != 9 || *input.add(data) != TAG {
        return ERR_DATA;
    }
    let min = u64_at(input, data + 1);
    if min == 0 {
        return ERR_DATA;
    }
    if accounts == 0 {
        return ERR_NO_ACCOUNTS;
    }
    if most >= min {
        OK
    } else {
        ERR_NONE_HOLDS
    }
}

/// The program's entrypoint.
///
/// # Safety
/// Called by the loader with its serialized input.
#[cfg(target_os = "solana")]
#[no_mangle]
pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u64 {
    process(input)
}

#[cfg(target_os = "solana")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One account as the loader serializes it.
    fn account(out: &mut Vec<u8>, data: &[u8]) {
        out.push(NOT_A_REPEAT);
        out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0]); // signer, writable, executable, padding
        out.extend_from_slice(&[7u8; 32]); // key
        out.extend_from_slice(&[9u8; 32]); // owner
        out.extend_from_slice(&1_000u64.to_le_bytes()); // lamports
        out.extend_from_slice(&(data.len() as u64).to_le_bytes());
        out.extend_from_slice(data);
        out.extend_from_slice(&vec![0u8; GROWTH]);
        while !out.len().is_multiple_of(8) {
            out.push(0);
        }
        out.extend_from_slice(&0u64.to_le_bytes()); // rent epoch
    }

    fn repeat(out: &mut Vec<u8>, of: u8) {
        out.push(of);
        out.extend_from_slice(&[0; 7]);
    }

    /// A token account's data with this amount (165 bytes, the amount at 64).
    fn token(amount: u64) -> Vec<u8> {
        let mut d = vec![3u8; 165];
        d[AMOUNT_AT..AMOUNT_AT + 8].copy_from_slice(&amount.to_le_bytes());
        d
    }

    fn input(accounts: &[Option<Vec<u8>>], repeats: &[u8], ix: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&((accounts.len() + repeats.len()) as u64).to_le_bytes());
        for a in accounts {
            account(&mut out, a.as_deref().unwrap_or(&[]));
        }
        for r in repeats {
            repeat(&mut out, *r);
        }
        out.extend_from_slice(&(ix.len() as u64).to_le_bytes());
        out.extend_from_slice(ix);
        out.extend_from_slice(&[5u8; 32]); // program id
        out
    }

    fn ix(min: u64) -> Vec<u8> {
        let mut d = vec![TAG];
        d.extend_from_slice(&min.to_le_bytes());
        d
    }

    fn run(buf: &[u8]) -> u64 {
        unsafe { process(buf.as_ptr()) }
    }

    #[test]
    fn it_passes_when_one_of_the_accounts_holds() {
        // Three traders: two have no token account yet, the third has bought.
        let buf = input(&[None, None, Some(token(851_809_863_080))], &[], &ix(1));
        assert_eq!(run(&buf), OK);
        // The holder first, an odd-length account between (the padding to eight is walked).
        let buf = input(&[Some(token(5)), Some(vec![1u8; 13]), None], &[], &ix(5));
        assert_eq!(run(&buf), OK);
    }

    #[test]
    fn it_fails_when_none_holds_enough() {
        let buf = input(&[None, None], &[], &ix(1));
        assert_eq!(run(&buf), ERR_NONE_HOLDS);
        // An account that exists with nothing in it, and one under the minimum.
        let buf = input(&[Some(token(0)), Some(token(9))], &[], &ix(10));
        assert_eq!(run(&buf), ERR_NONE_HOLDS);
        // Data too short to carry an amount counts as not holding.
        let buf = input(&[Some(vec![0xFF; 71])], &[], &ix(1));
        assert_eq!(run(&buf), ERR_NONE_HOLDS);
    }

    #[test]
    fn a_repeated_account_is_walked_past() {
        let buf = input(&[None, Some(token(2))], &[0, 1], &ix(2));
        assert_eq!(run(&buf), OK);
        let buf = input(&[None], &[0, 0], &ix(1));
        assert_eq!(run(&buf), ERR_NONE_HOLDS);
    }

    #[test]
    fn bad_instruction_data_and_no_accounts_are_refused() {
        let holder = [Some(token(1))];
        assert_eq!(run(&input(&holder, &[], &[])), ERR_DATA);
        assert_eq!(run(&input(&holder, &[], &[2, 1, 0, 0, 0, 0, 0, 0, 0])), ERR_DATA);
        assert_eq!(run(&input(&holder, &[], &ix(0))), ERR_DATA);
        assert_eq!(run(&input(&holder, &[], &[TAG, 1, 0])), ERR_DATA);
        assert_eq!(run(&input(&[], &[], &ix(1))), ERR_NO_ACCOUNTS);
    }
}
