use crate::{errors::WagerError, state::*, TOKEN_ID};
use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{Token, TokenAccount};

pub fn refund_wager_handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, RefundWager<'info>>,
    session_id: String,
) -> Result<()> {
    let game_session = &ctx.accounts.game_session;
    msg!("Starting Refund for session: {}", session_id);

    let players = game_session.get_all_players();
    msg!("Number of players: {}", players.len());
    msg!(
        "Number of remaining accounts: {}",
        ctx.remaining_accounts.len()
    );

    // We need at least one player and their token account
    require!(
        !ctx.remaining_accounts.is_empty(),
        WagerError::InvalidRemainingAccounts
    );

    // Make sure remaining accounts are in pairs
    require!(
        ctx.remaining_accounts.len() % 2 == 0,
        WagerError::InvalidRemainingAccounts
    );

    // ---
    // Remaining accounts order documentation:
    // For each player, the remaining_accounts must be:
    // [player_1, player_1_token_account, player_2, player_2_token_account, ...]
    // This order is required for correct refund distribution.
    // ---
    require!(
        ctx.remaining_accounts.len() == 2 * players.len(),
        WagerError::InvalidRemainingAccounts
    );
    for i in 0..players.len() {
        let player_acc = &ctx.remaining_accounts[i * 2];
        let token_acc = &ctx.remaining_accounts[i * 2 + 1];
        // Defensive: Ensure token_acc is a token account (checked by try_from below)
        let _ = Account::<TokenAccount>::try_from(token_acc)?;
    }

    // Defensive: Check for duplicate players
    let mut seen = std::collections::HashSet::new();
    for player in &players {
        // Check for zero address
        require!(*player != Pubkey::default(), WagerError::InvalidPlayer);
        // Check for duplicates
        require!(seen.insert(*player), WagerError::DuplicatePlayer);
    }
    // Defensive: Check vault has enough balance for all refunds
    let total_refund = game_session.session_bet.checked_mul(players.len() as u64)
        .ok_or(WagerError::TotalPotCalculationError)?;
    require!(ctx.accounts.vault_token_account.amount >= total_refund, WagerError::InsufficientVaultBalance);

    for player in players {
        // Skip default player
        if player == Pubkey::default() {
            continue;
        }

        let refund = game_session.session_bet.checked_add(0)
            .ok_or(WagerError::WinningsCalculationError)?;
        msg!("Earnings for player {}: {}", player, refund);

        // Find the player's account and token account in remaining_accounts
        let player_index = ctx
            .remaining_accounts
            .iter()
            .step_by(2) // Skip token accounts to only look at player accounts
            .position(|acc| acc.key() == player)
            .ok_or(WagerError::InvalidPlayer)?;

        // Get player and token account from remaining accounts
        let player_account = &ctx.remaining_accounts[player_index * 2];
        let player_token_account_info = &ctx.remaining_accounts[player_index * 2 + 1];
        let player_token_account = Account::<TokenAccount>::try_from(player_token_account_info)?;

        // Verify player token account constraints
        require!(
            player_token_account.owner == player_account.key(),
            WagerError::InvalidPlayerTokenAccount
        );

        // Verify token account mint
        require!(
            player_token_account.mint == TOKEN_ID,
            WagerError::InvalidTokenMint
        );

        // Get vault balance before transfer
        let vault_balance = ctx.accounts.vault_token_account.amount;
        msg!("Vault balance before transfer: {}", vault_balance);

        // Transfer tokens from vault to player
        anchor_spl::token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                anchor_spl::token::Transfer {
                    from: ctx.accounts.vault_token_account.to_account_info(),
                    to: player_token_account.to_account_info(),
                    authority: ctx.accounts.vault.to_account_info(),
                },
                &[&[
                    b"vault",
                    session_id.as_bytes(),
                    &[ctx.accounts.game_session.vault_bump],
                ]],
            ),
            refund,
        )?;
    }

    // Mark session as completed
    let game_session = &mut ctx.accounts.game_session;
    game_session.status = GameStatus::Completed;

    Ok(())
}
#[derive(Accounts)]
#[instruction(session_id: String)]
pub struct RefundWager<'info> {
    /// The game server authority that created the session
    pub game_server: Signer<'info>,

    #[account(
        mut,
        seeds = [b"game_session", session_id.as_bytes()],
        bump = game_session.bump,
        constraint = game_session.authority == game_server.key() @ WagerError::UnauthorizedDistribution,
    )]
    pub game_session: Account<'info, GameSession>,

    /// CHECK: Vault PDA that holds the funds
    #[account(
        mut,
        seeds = [b"vault", session_id.as_bytes()],
        bump = game_session.vault_bump,
    )]
    pub vault: AccountInfo<'info>,

    #[account(
        mut,
        associated_token::mint = TOKEN_ID,
        associated_token::authority = vault
    )]
    pub vault_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}
