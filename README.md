# counter-todo

> **Not audited. Devnet / localnet only.** Learning project: do not deploy to mainnet or use with real funds.

Two small Anchor programs in one crate:

1. **Counter**: one PDA per user (`["counter", authority]`), with `increment` / `decrement`
   using checked arithmetic.
2. **To-do list**: a list header PDA per user (`["todo_list", owner]`) and one PDA per item
   (`["todo", owner, id.to_le_bytes()]`). Item titles are variable length. Renaming an item
   resizes its account with `realloc`, and deleting it closes the account and refunds the rent.

## Quick start

```bash
# Anchor 1.2 + Agave 4.x CLI on PATH
anchor build                 # builds target/deploy/counter_todo.so + IDL
cargo test                   # LiteSVM in-process tests (no validator)
```

Anchor 1.2 defaults to SBF platform-tools v1.57. If you only have another version installed,
pass it explicitly, for example `anchor build --tools-version v1.58`. The tests use LiteSVM 0.16
(Agave 4.2 runtime, which supports the SBPF v3 binaries Anchor 1.2 emits) and need Rust ≥ 1.93,
so `rust-toolchain.toml` pins 1.96.0.

The tests load `target/deploy/counter_todo.so`, so run `anchor build` before `cargo test`.

## Instructions

| instruction | accounts | notes |
|---|---|---|
| `init_counter(start)` | authority (signer, payer), counter PDA | |
| `increment(by)` / `decrement(by)` | authority (signer), counter | `has_one = authority`, `checked_add` / `checked_sub` |
| `init_list()` | owner (signer, payer), list PDA | |
| `add_item(title)` | owner, list, item PDA (`next_id`) | account sized to the title |
| `rename_item(id, title)` | owner, item | `realloc` to the new size; owner pays or is refunded |
| `toggle_item(id)` | owner, item | |
| `delete_item(id)` | owner, list, item | `close = owner` |

## Account sizing

Every Anchor account starts with an **8-byte discriminator** (the first 8 bytes of
`sha256("account:<Name>")`). After that comes the Borsh-serialized struct. Borsh has no padding:

| type | bytes |
|---|---|
| `bool`, `u8` | 1 |
| `u32` | 4 |
| `u64`, `i64` | 8 |
| `Pubkey` | 32 |
| `String` | 4 (u32 length prefix) + UTF-8 bytes |
| `Vec<T>` | 4 + n × size(T) |
| `Option<T>` | 1 + size(T) |

Fixed-size structs use `#[derive(InitSpace)]`, and you allocate `8 + Counter::INIT_SPACE`:

```
Counter  = 8 (disc) + 32 (authority) + 8 (count) + 1 (bump)                 = 49 bytes
TodoList = 8 (disc) + 32 (owner) + 8 (next_id) + 4 (open_items) + 1 (bump) = 53 bytes
```

`TodoItem` holds a `String`, so its size depends on the title. Rather than reserving the
maximum (200 bytes) for every item, the program computes the exact size:

```rust
pub const fn space(title_len: usize) -> usize {
    8 + 32 + 8 + 1 + 1 + 4 + title_len
}
```

`"buy milk"` therefore takes 62 bytes instead of 254. When the title changes, the
`realloc = TodoItem::space(new_len)` constraint:

- **grows** the account and transfers the extra rent-exempt lamports from `realloc::payer`;
- **shrinks** the account and sends the surplus lamports back to the payer.

`realloc::zero = false` skips zeroing new memory. That is safe here because Anchor
re-serializes the whole struct on exit. Set it to `true` if you shrink and grow in the same
transaction and read raw bytes.

Two more rules apply. A single realloc can grow an account by at most 10 KiB
(`MAX_PERMITTED_DATA_INCREASE`), and the title is capped at `MAX_TITLE_LEN = 200` so an
item can never grow without bound. Rent is about 6.96 lamports per byte-year × 2 years, plus a
128-byte account overhead. The tests check that `lamports == minimum_balance_for_rent_exemption(len)`
after each resize.

## Tests

`programs/counter-todo/tests/` (LiteSVM):

- happy path: counter inc/dec, and the to-do lifecycle with grow/shrink realloc, toggle and close
- overflow (`u64::MAX`) and underflow are rejected and the state is unchanged
- **wrong signer**: another wallet signing for someone else's counter fails `has_one`, and on items fails seed derivation
- **authority must sign**: stripping the signer flag gives `AccountNotSigner`
- **wrong owner**: passing a system-owned account as a `Counter` gives `AccountOwnedByWrongProgram`
- title validation: empty, too long, and an exact-max title

## Status / limitations

- Learning code: no pagination or indexing of items. Clients derive item PDAs from `0..next_id`
  and skip closed ones.
- Ids are never reused after delete.

## License

MIT © Abdeldjalil Chougui
