mod common;

use anchor_lang::{
    prelude::Pubkey,
    solana_program::{instruction::Instruction, system_program},
    InstructionData, ToAccountMetas,
};
use common::*;
use counter_todo::{constants::COUNTER_SEED, error::TodoError, state::Counter};
use solana_signer::Signer;

fn counter_pda(authority: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[COUNTER_SEED, authority.as_ref()], &counter_todo::id()).0
}

fn init_ix(authority: &Pubkey, start: u64) -> Instruction {
    Instruction::new_with_bytes(
        counter_todo::id(),
        &counter_todo::instruction::InitCounter { start }.data(),
        counter_todo::accounts::InitCounter {
            authority: *authority,
            counter: counter_pda(authority),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn update_ix(signer: &Pubkey, counter: Pubkey, inc: bool, by: u64) -> Instruction {
    let data = if inc {
        counter_todo::instruction::Increment { by }.data()
    } else {
        counter_todo::instruction::Decrement { by }.data()
    };
    Instruction::new_with_bytes(
        counter_todo::id(),
        &data,
        counter_todo::accounts::UpdateCounter { authority: *signer, counter }.to_account_metas(None),
    )
}

#[test]
fn increment_and_decrement() {
    let mut svm = svm();
    let alice = funded(&mut svm);
    send(&mut svm, &[init_ix(&alice.pubkey(), 5)], &alice, &[&alice]).unwrap();
    let pda = counter_pda(&alice.pubkey());

    send(&mut svm, &[update_ix(&alice.pubkey(), pda, true, 10)], &alice, &[&alice]).unwrap();
    send(&mut svm, &[update_ix(&alice.pubkey(), pda, false, 3)], &alice, &[&alice]).unwrap();

    let c: Counter = fetch(&svm, &pda);
    assert_eq!(c.count, 12);
    assert_eq!(c.authority, alice.pubkey());
}

#[test]
fn overflow_is_rejected() {
    let mut svm = svm();
    let alice = funded(&mut svm);
    send(&mut svm, &[init_ix(&alice.pubkey(), u64::MAX - 1)], &alice, &[&alice]).unwrap();
    let pda = counter_pda(&alice.pubkey());
    let err = send(&mut svm, &[update_ix(&alice.pubkey(), pda, true, 2)], &alice, &[&alice])
        .unwrap_err();
    assert_eq!(custom_code(&err), Some(ERR_CUSTOM_BASE + TodoError::Overflow as u32));
    let c: Counter = fetch(&svm, &pda);
    assert_eq!(c.count, u64::MAX - 1, "state untouched after failed tx");
}

#[test]
fn underflow_is_rejected() {
    let mut svm = svm();
    let alice = funded(&mut svm);
    send(&mut svm, &[init_ix(&alice.pubkey(), 0)], &alice, &[&alice]).unwrap();
    let pda = counter_pda(&alice.pubkey());
    let err = send(&mut svm, &[update_ix(&alice.pubkey(), pda, false, 1)], &alice, &[&alice])
        .unwrap_err();
    assert_eq!(custom_code(&err), Some(ERR_CUSTOM_BASE + TodoError::Underflow as u32));
}

#[test]
fn wrong_signer_cannot_touch_someone_elses_counter() {
    let mut svm = svm();
    let alice = funded(&mut svm);
    let mallory = funded(&mut svm);
    send(&mut svm, &[init_ix(&alice.pubkey(), 0)], &alice, &[&alice]).unwrap();
    let pda = counter_pda(&alice.pubkey());
    // Mallory signs and passes Alice's counter: has_one = authority fails.
    let err = send(&mut svm, &[update_ix(&mallory.pubkey(), pda, true, 1)], &mallory, &[&mallory])
        .unwrap_err();
    assert_eq!(custom_code(&err), Some(ERR_HAS_ONE));
}

#[test]
fn authority_must_sign() {
    let mut svm = svm();
    let alice = funded(&mut svm);
    let payer = funded(&mut svm);
    send(&mut svm, &[init_ix(&alice.pubkey(), 0)], &alice, &[&alice]).unwrap();
    let pda = counter_pda(&alice.pubkey());
    // Build the instruction but strip the signer flag from Alice's meta.
    let mut ix = update_ix(&alice.pubkey(), pda, true, 1);
    ix.accounts[0].is_signer = false;
    let err = send(&mut svm, &[ix], &payer, &[&payer]).unwrap_err();
    // Anchor's Signer<'info> check: AccountNotSigner = 3010.
    assert_eq!(custom_code(&err), Some(3010));
}

#[test]
fn account_not_owned_by_program_is_rejected() {
    let mut svm = svm();
    let alice = funded(&mut svm);
    // Pass a plain system-owned wallet where a Counter is expected.
    let fake = funded(&mut svm);
    let err = send(&mut svm, &[update_ix(&alice.pubkey(), fake.pubkey(), true, 1)], &alice, &[&alice])
        .unwrap_err();
    assert_eq!(custom_code(&err), Some(ERR_WRONG_OWNER));
}
