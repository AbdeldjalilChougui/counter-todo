mod common;

use anchor_lang::{
    prelude::Pubkey,
    solana_program::{instruction::Instruction, system_program},
    InstructionData, ToAccountMetas,
};
use common::*;
use counter_todo::{
    constants::{MAX_TITLE_LEN, TODO_LIST_SEED, TODO_SEED},
    error::TodoError,
    state::{TodoItem, TodoList},
};
use solana_signer::Signer;

fn list_pda(owner: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[TODO_LIST_SEED, owner.as_ref()], &counter_todo::id()).0
}
fn item_pda(owner: &Pubkey, id: u64) -> Pubkey {
    Pubkey::find_program_address(&[TODO_SEED, owner.as_ref(), &id.to_le_bytes()], &counter_todo::id())
        .0
}

fn init_list_ix(owner: &Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        counter_todo::id(),
        &counter_todo::instruction::InitList {}.data(),
        counter_todo::accounts::InitList {
            owner: *owner,
            list: list_pda(owner),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}
fn add_ix(owner: &Pubkey, next_id: u64, title: &str) -> Instruction {
    Instruction::new_with_bytes(
        counter_todo::id(),
        &counter_todo::instruction::AddItem { title: title.into() }.data(),
        counter_todo::accounts::AddItem {
            owner: *owner,
            list: list_pda(owner),
            item: item_pda(owner, next_id),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}
fn rename_ix(owner: &Pubkey, item: Pubkey, id: u64, title: &str) -> Instruction {
    Instruction::new_with_bytes(
        counter_todo::id(),
        &counter_todo::instruction::RenameItem { _id: id, title: title.into() }.data(),
        counter_todo::accounts::RenameItem { owner: *owner, item, system_program: system_program::ID }
            .to_account_metas(None),
    )
}
fn toggle_ix(owner: &Pubkey, item: Pubkey, id: u64) -> Instruction {
    Instruction::new_with_bytes(
        counter_todo::id(),
        &counter_todo::instruction::ToggleItem { _id: id }.data(),
        counter_todo::accounts::ToggleItem { owner: *owner, item }.to_account_metas(None),
    )
}
fn delete_ix(owner: &Pubkey, id: u64) -> Instruction {
    Instruction::new_with_bytes(
        counter_todo::id(),
        &counter_todo::instruction::DeleteItem { _id: id }.data(),
        counter_todo::accounts::DeleteItem {
            owner: *owner,
            list: list_pda(owner),
            item: item_pda(owner, id),
        }
        .to_account_metas(None),
    )
}

#[test]
fn full_lifecycle_with_realloc() {
    let mut svm = svm();
    let alice = funded(&mut svm);
    let a = alice.pubkey();
    send(&mut svm, &[init_list_ix(&a)], &alice, &[&alice]).unwrap();
    send(&mut svm, &[add_ix(&a, 0, "buy milk")], &alice, &[&alice]).unwrap();
    send(&mut svm, &[add_ix(&a, 1, "ship it")], &alice, &[&alice]).unwrap();

    let list: TodoList = fetch(&svm, &list_pda(&a));
    assert_eq!((list.next_id, list.open_items), (2, 2));

    let item0 = item_pda(&a, 0);
    assert_eq!(svm.get_account(&item0).unwrap().data.len(), TodoItem::space(8));

    // Grow: the account is resized and the owner pays the extra rent.
    let long = "buy oat milk, eggs, and coffee beans";
    let before = svm.get_balance(&a).unwrap();
    send(&mut svm, &[rename_ix(&a, item0, 0, long)], &alice, &[&alice]).unwrap();
    let acc = svm.get_account(&item0).unwrap();
    assert_eq!(acc.data.len(), TodoItem::space(long.len()));
    assert_eq!(acc.lamports, svm.minimum_balance_for_rent_exemption(acc.data.len()));
    assert!(svm.get_balance(&a).unwrap() < before);
    let it: TodoItem = fetch(&svm, &item0);
    assert_eq!(it.title, long);

    // Shrink: excess lamports go back to the owner.
    send(&mut svm, &[rename_ix(&a, item0, 0, "milk")], &alice, &[&alice]).unwrap();
    let acc = svm.get_account(&item0).unwrap();
    assert_eq!(acc.data.len(), TodoItem::space(4));
    assert_eq!(acc.lamports, svm.minimum_balance_for_rent_exemption(acc.data.len()));

    send(&mut svm, &[toggle_ix(&a, item0, 0)], &alice, &[&alice]).unwrap();
    let it: TodoItem = fetch(&svm, &item0);
    assert!(it.done);

    // Delete closes the account and refunds rent.
    send(&mut svm, &[delete_ix(&a, 0)], &alice, &[&alice]).unwrap();
    assert!(svm.get_account(&item0).map(|a| a.lamports == 0).unwrap_or(true));
    let list: TodoList = fetch(&svm, &list_pda(&a));
    assert_eq!((list.next_id, list.open_items), (2, 1));
}

#[test]
fn title_validation() {
    let mut svm = svm();
    let alice = funded(&mut svm);
    let a = alice.pubkey();
    send(&mut svm, &[init_list_ix(&a)], &alice, &[&alice]).unwrap();

    let err = send(&mut svm, &[add_ix(&a, 0, "")], &alice, &[&alice]).unwrap_err();
    assert_eq!(custom_code(&err), Some(ERR_CUSTOM_BASE + TodoError::EmptyTitle as u32));

    let too_long = "x".repeat(MAX_TITLE_LEN as usize + 1);
    let err = send(&mut svm, &[add_ix(&a, 0, &too_long)], &alice, &[&alice]).unwrap_err();
    assert_eq!(custom_code(&err), Some(ERR_CUSTOM_BASE + TodoError::TitleTooLong as u32));

    let max = "x".repeat(MAX_TITLE_LEN as usize);
    send(&mut svm, &[add_ix(&a, 0, &max)], &alice, &[&alice]).unwrap();
    let err = send(&mut svm, &[rename_ix(&a, item_pda(&a, 0), 0, &too_long)], &alice, &[&alice])
        .unwrap_err();
    assert_eq!(custom_code(&err), Some(ERR_CUSTOM_BASE + TodoError::TitleTooLong as u32));
}

#[test]
fn other_user_cannot_edit_or_delete() {
    let mut svm = svm();
    let alice = funded(&mut svm);
    let mallory = funded(&mut svm);
    let a = alice.pubkey();
    let m = mallory.pubkey();
    send(&mut svm, &[init_list_ix(&a)], &alice, &[&alice]).unwrap();
    send(&mut svm, &[add_ix(&a, 0, "secret")], &alice, &[&alice]).unwrap();
    let item0 = item_pda(&a, 0);

    // Mallory signs but targets Alice's item: the seeds derive from the
    // signer, so the PDA does not match.
    let err = send(&mut svm, &[toggle_ix(&m, item0, 0)], &mallory, &[&mallory]).unwrap_err();
    assert_eq!(custom_code(&err), Some(ERR_SEEDS));
    let err = send(&mut svm, &[rename_ix(&m, item0, 0, "pwned")], &mallory, &[&mallory]).unwrap_err();
    assert_eq!(custom_code(&err), Some(ERR_SEEDS));

    // Mallory cannot delete either (her list does not exist).
    let mut ix = delete_ix(&m, 0);
    ix.accounts[2].pubkey = item0;
    let err = send(&mut svm, &[ix], &mallory, &[&mallory]).unwrap_err();
    assert_eq!(custom_code(&err), Some(ERR_NOT_INITIALIZED));

    let it: TodoItem = fetch(&svm, &item0);
    assert_eq!((it.title.as_str(), it.done), ("secret", false));
}

#[test]
fn each_user_has_an_independent_list() {
    let mut svm = svm();
    let alice = funded(&mut svm);
    let bob = funded(&mut svm);
    for kp in [&alice, &bob] {
        send(&mut svm, &[init_list_ix(&kp.pubkey())], kp, &[kp]).unwrap();
        send(&mut svm, &[add_ix(&kp.pubkey(), 0, "hello")], kp, &[kp]).unwrap();
    }
    assert_ne!(item_pda(&alice.pubkey(), 0), item_pda(&bob.pubkey(), 0));
    // Re-initialising an existing list fails (account already in use).
    assert!(send(&mut svm, &[init_list_ix(&alice.pubkey())], &alice, &[&alice]).is_err());
}
