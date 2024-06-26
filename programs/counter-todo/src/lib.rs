//! counter-todo: two tiny Anchor programs in one.
//!
//! 1. A per-user counter (PDA `["counter", authority]`) with checked arithmetic.
//! 2. A per-user to-do list: a list header PDA (`["todo_list", owner]`) and one
//!    PDA per item (`["todo", owner, id.to_le_bytes()]`). Item titles are
//!    variable length, so editing a title resizes the account with `realloc`.
use anchor_lang::prelude::*;

declare_id!("PxRoBhMed1ZevhbvatiJqo7dLkujr73QKyCArd6YusY");

pub mod constants {
    use super::*;

    #[constant]
    pub const COUNTER_SEED: &[u8] = b"counter";
    #[constant]
    pub const TODO_LIST_SEED: &[u8] = b"todo_list";
    #[constant]
    pub const TODO_SEED: &[u8] = b"todo";
    /// Hard cap on a title, in bytes (UTF-8), so an item can never grow unbounded.
    #[constant]
    pub const MAX_TITLE_LEN: u32 = 200;
}

pub mod error {
    use super::*;

    #[error_code]
    pub enum TodoError {
        #[msg("Counter would overflow u64")]
        Overflow,
        #[msg("Counter would go below zero")]
        Underflow,
        #[msg("Title is empty")]
        EmptyTitle,
        #[msg("Title is longer than MAX_TITLE_LEN bytes")]
        TitleTooLong,
    }
}

pub mod state {
    use super::*;

    #[account]
    #[derive(InitSpace)]
    pub struct Counter {
        pub authority: Pubkey,
        pub count: u64,
        pub bump: u8,
    }

    #[account]
    #[derive(InitSpace)]
    pub struct TodoList {
        pub owner: Pubkey,
        /// Next id to hand out. Ids are never reused, so deleted items leave gaps.
        pub next_id: u64,
        /// Number of live (not deleted) items.
        pub open_items: u32,
        pub bump: u8,
    }

    #[account]
    pub struct TodoItem {
        pub owner: Pubkey,
        pub id: u64,
        pub done: bool,
        pub bump: u8,
        pub title: String,
    }

    impl TodoItem {
        /// Bytes needed for an item whose title is `title_len` bytes long,
        /// including the 8-byte Anchor discriminator.
        pub const fn space(title_len: usize) -> usize {
            8 // discriminator
            + 32 // owner
            + 8 // id
            + 1 // done
            + 1 // bump
            + 4 + title_len // borsh String = u32 length prefix + bytes
        }
    }
}

use constants::*;
use error::TodoError;
use state::*;

const MAX_TITLE: usize = MAX_TITLE_LEN as usize;

fn validate_title(title: &str) -> Result<()> {
    require!(!title.is_empty(), TodoError::EmptyTitle);
    require!(title.len() <= MAX_TITLE, TodoError::TitleTooLong);
    Ok(())
}

#[program]
pub mod counter_todo {
    use super::*;

    // ---------------------------------------------------------------- counter

    pub fn init_counter(ctx: Context<InitCounter>, start: u64) -> Result<()> {
        let c = &mut ctx.accounts.counter;
        c.authority = ctx.accounts.authority.key();
        c.count = start;
        c.bump = ctx.bumps.counter;
        Ok(())
    }

    pub fn increment(ctx: Context<UpdateCounter>, by: u64) -> Result<()> {
        let c = &mut ctx.accounts.counter;
        c.count = c.count.checked_add(by).ok_or(TodoError::Overflow)?;
        Ok(())
    }

    pub fn decrement(ctx: Context<UpdateCounter>, by: u64) -> Result<()> {
        let c = &mut ctx.accounts.counter;
        c.count = c.count.checked_sub(by).ok_or(TodoError::Underflow)?;
        Ok(())
    }

    // ------------------------------------------------------------------- todo

    pub fn init_list(ctx: Context<InitList>) -> Result<()> {
        let l = &mut ctx.accounts.list;
        l.owner = ctx.accounts.owner.key();
        l.next_id = 0;
        l.open_items = 0;
        l.bump = ctx.bumps.list;
        Ok(())
    }

    pub fn add_item(ctx: Context<AddItem>, title: String) -> Result<()> {
        validate_title(&title)?;
        let list = &mut ctx.accounts.list;
        let item = &mut ctx.accounts.item;
        item.owner = ctx.accounts.owner.key();
        item.id = list.next_id;
        item.done = false;
        item.bump = ctx.bumps.item;
        item.title = title;
        list.next_id = list.next_id.checked_add(1).ok_or(TodoError::Overflow)?;
        list.open_items = list.open_items.checked_add(1).ok_or(TodoError::Overflow)?;
        Ok(())
    }

    /// Change the title. The account is resized to fit exactly; growing it
    /// charges `owner` the extra rent, shrinking it refunds the difference.
    pub fn rename_item(ctx: Context<RenameItem>, _id: u64, title: String) -> Result<()> {
        validate_title(&title)?;
        ctx.accounts.item.title = title;
        Ok(())
    }

    pub fn toggle_item(ctx: Context<ToggleItem>, _id: u64) -> Result<()> {
        let item = &mut ctx.accounts.item;
        item.done = !item.done;
        Ok(())
    }

    /// Close the item account and return its rent to the owner.
    pub fn delete_item(ctx: Context<DeleteItem>, _id: u64) -> Result<()> {
        let list = &mut ctx.accounts.list;
        list.open_items = list.open_items.checked_sub(1).ok_or(TodoError::Underflow)?;
        Ok(())
    }
}

// -------------------------------------------------------------------- accounts

#[derive(Accounts)]
pub struct InitCounter<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        init,
        payer = authority,
        space = 8 + Counter::INIT_SPACE,
        seeds = [COUNTER_SEED, authority.key().as_ref()],
        bump
    )]
    pub counter: Account<'info, Counter>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateCounter<'info> {
    pub authority: Signer<'info>,
    #[account(
        mut,
        seeds = [COUNTER_SEED, counter.authority.as_ref()],
        bump = counter.bump,
        has_one = authority
    )]
    pub counter: Account<'info, Counter>,
}

#[derive(Accounts)]
pub struct InitList<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        init,
        payer = owner,
        space = 8 + TodoList::INIT_SPACE,
        seeds = [TODO_LIST_SEED, owner.key().as_ref()],
        bump
    )]
    pub list: Account<'info, TodoList>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(title: String)]
pub struct AddItem<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        mut,
        seeds = [TODO_LIST_SEED, owner.key().as_ref()],
        bump = list.bump,
        has_one = owner
    )]
    pub list: Account<'info, TodoList>,
    #[account(
        init,
        payer = owner,
        // Size the account for exactly this title (capped, see validate_title).
        space = TodoItem::space(title.len().min(MAX_TITLE)),
        seeds = [TODO_SEED, owner.key().as_ref(), &list.next_id.to_le_bytes()],
        bump
    )]
    pub item: Account<'info, TodoItem>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(id: u64, title: String)]
pub struct RenameItem<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        mut,
        seeds = [TODO_SEED, owner.key().as_ref(), &id.to_le_bytes()],
        bump = item.bump,
        has_one = owner,
        realloc = TodoItem::space(title.len().min(MAX_TITLE)),
        realloc::payer = owner,
        realloc::zero = false
    )]
    pub item: Account<'info, TodoItem>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(id: u64)]
pub struct ToggleItem<'info> {
    pub owner: Signer<'info>,
    #[account(
        mut,
        seeds = [TODO_SEED, owner.key().as_ref(), &id.to_le_bytes()],
        bump = item.bump,
        has_one = owner
    )]
    pub item: Account<'info, TodoItem>,
}

#[derive(Accounts)]
#[instruction(id: u64)]
pub struct DeleteItem<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        mut,
        seeds = [TODO_LIST_SEED, owner.key().as_ref()],
        bump = list.bump,
        has_one = owner
    )]
    pub list: Account<'info, TodoList>,
    #[account(
        mut,
        seeds = [TODO_SEED, owner.key().as_ref(), &id.to_le_bytes()],
        bump = item.bump,
        has_one = owner,
        close = owner
    )]
    pub item: Account<'info, TodoItem>,
}
