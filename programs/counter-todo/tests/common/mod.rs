#![allow(dead_code)]
use anchor_lang::{
    prelude::Pubkey, solana_program::instruction::Instruction, AccountDeserialize,
};
use litesvm::{types::FailedTransactionMetadata, LiteSVM};
use solana_keypair::Keypair;
use solana_message::{Message, VersionedMessage};
use solana_signer::Signer;
use solana_transaction::versioned::VersionedTransaction;

pub const LAMPORTS: u64 = 10_000_000_000;

pub fn svm() -> LiteSVM {
    let mut svm = LiteSVM::new();
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/deploy/counter_todo.so"
    ));
    svm.add_program(counter_todo::id(), bytes).unwrap();
    svm
}

pub fn funded(svm: &mut LiteSVM) -> Keypair {
    let kp = Keypair::new();
    svm.airdrop(&kp.pubkey(), LAMPORTS).unwrap();
    kp
}

pub fn send(
    svm: &mut LiteSVM,
    ixs: &[Instruction],
    payer: &Keypair,
    signers: &[&Keypair],
) -> Result<(), FailedTransactionMetadata> {
    svm.expire_blockhash();
    let bh = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(ixs, Some(&payer.pubkey()), &bh);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), signers).unwrap();
    svm.send_transaction(tx).map(|_| ())
}

/// Custom error code from a failed transaction (Anchor errors are `Custom(n)`).
pub fn custom_code(err: &FailedTransactionMetadata) -> Option<u32> {
    use solana_transaction_error::TransactionError;
    match &err.err {
        TransactionError::InstructionError(_, solana_instruction::error::InstructionError::Custom(c)) => {
            Some(*c)
        }
        _ => None,
    }
}

pub fn fetch<T: AccountDeserialize>(svm: &LiteSVM, key: &Pubkey) -> T {
    let acc = svm.get_account(key).expect("account exists");
    T::try_deserialize(&mut acc.data.as_slice()).unwrap()
}

/// Anchor framework error codes used in assertions.
pub const ERR_HAS_ONE: u32 = 2001;
pub const ERR_SEEDS: u32 = 2006;
pub const ERR_NOT_INITIALIZED: u32 = 3012;
pub const ERR_WRONG_OWNER: u32 = 3007;
pub const ERR_CUSTOM_BASE: u32 = 6000;
