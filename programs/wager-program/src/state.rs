//! State accounts for the betting program
use crate::errors::WagerError;
use anchor_lang::prelude::*;

/// Game mode defining the team sizes and payout logic
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq)]
pub enum GameMode {
    /// 1v1 winner-takes-all
    WinnerTakesAllOneVsOne,
    /// 3v3 winner-takes-all
    WinnerTakesAllThreeVsThree,
    /// 5v5 winner-takes-all
    WinnerTakesAllFiveVsFive,
    /// 1v1 pay-to-spawn
    PayToSpawnOneVsOne,
    /// 3v3 pay-to-spawn
    PayToSpawnThreeVsThree,
    /// 5v5 pay-to-spawn
    PayToSpawnFiveVsFive,
}

impl GameMode {
    /// Returns the required number of players per team for this game mode
    pub fn players_per_team(&self) -> usize {
        match self {
            Self::WinnerTakesAllOneVsOne => 1,
            Self::WinnerTakesAllThreeVsThree => 3,
            Self::WinnerTakesAllFiveVsFive => 5,
            Self::PayToSpawnOneVsOne => 1,
            Self::PayToSpawnThreeVsThree => 3,
            Self::PayToSpawnFiveVsFive => 5,
        }
    }
}

/// Status of a game session
#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq)]
pub enum GameStatus {
    /// Waiting for players to join
    WaitingForPlayers,
    /// Game is active with all players joined
    InProgress,
    /// Game has finished and rewards distributed
    Completed,
}

impl Default for GameStatus {
    fn default() -> Self {
        Self::WaitingForPlayers
    }
}

/// Represents a team in the game
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct Team {
    /// Array of player public keys
    pub players: [Pubkey; 5],
    /// Total amount bet by team (in lamports)
    pub total_bet: u64,
    /// Number of spawns remaining for each player
    pub player_spawns: [u16; 5],
    /// Number of kills for each player
    pub player_kills: [u16; 5],
}

impl Team {
    /// Finds the first empty slot in the team, if available
    pub fn get_empty_slot(&self, player_count: usize) -> Result<usize> {
        self.players
            .iter()
            .enumerate()
            .find(|(i, player)| **player == Pubkey::default() && *i < player_count)
            .map(|(i, _)| i)
            .ok_or_else(|| error!(WagerError::TeamIsFull))
    }
}

/// Represents a game session between teams with its own pool
#[account]
pub struct GameSession {
    /// Unique identifier for the game
    pub session_id: String,
    /// Creator of the game session
    pub authority: Pubkey,
    /// Required bet amount per player
    pub session_bet: u64,
    /// Game configuration (1v1, 3v3, 5v5, etc.)
    pub game_mode: GameMode,
    /// First team
    pub team_a: Team,
    /// Second team
    pub team_b: Team,
    /// Current game state
    pub status: GameStatus,
    /// Creation timestamp
    pub created_at: i64,
    /// PDA bump
    pub bump: u8,
    /// Vault PDA bump
    pub vault_bump: u8,
    pub vault_token_bump: u8,
}

impl GameSession {
    /// Gets an empty slot for a player in the specified team
    pub fn get_player_empty_slot(&self, team: u8) -> Result<usize> {
        let player_count = self.game_mode.players_per_team();
        match team {
            0 => self.team_a.get_empty_slot(player_count),
            1 => self.team_b.get_empty_slot(player_count),
            _ => Err(error!(WagerError::InvalidTeam)),
        }
    }

    /// Checks if both teams are completely filled
    pub fn check_all_filled(&self) -> Result<bool> {
        let player_count = self.game_mode.players_per_team();

        Ok(matches!(
            (
                self.team_a.get_empty_slot(player_count),
                self.team_b.get_empty_slot(player_count)
            ),
            (Err(e1), Err(e2)) if is_team_full_error(&e1) && is_team_full_error(&e2)
        ))
    }

    /// Returns true if the game mode is pay-to-spawn
    pub fn is_pay_to_spawn(&self) -> bool {
        matches!(
            self.game_mode,
            GameMode::PayToSpawnOneVsOne
                | GameMode::PayToSpawnThreeVsThree
                | GameMode::PayToSpawnFiveVsFive
        )
    }

    /// Returns all player pubkeys in both teams
    pub fn get_all_players(&self) -> Vec<Pubkey> {
        let mut players = self.team_a.players.to_vec();
        players.extend(self.team_b.players.to_vec());
        players
    }

    /// Gets the index of a player in a team
    pub fn get_player_index(&self, team: u8, player: Pubkey) -> Result<usize> {
        match team {
            0 => self
                .team_a
                .players
                .iter()
                .position(|p| *p == player)
                .ok_or(error!(WagerError::PlayerNotFound)),
            1 => self
                .team_b
                .players
                .iter()
                .position(|p| *p == player)
                .ok_or(error!(WagerError::PlayerNotFound)),
            _ => return Err(error!(WagerError::InvalidTeam)),
        }
    }

    /// Gets the kill and spawn sum for a player in a team
    pub fn get_kills_and_spawns(&self, player_pubkey: Pubkey) -> Result<u16> {
        // search in both teams and return the kill and death difference
        let team_a_index = self.team_a.players.iter().position(|p| *p == player_pubkey);
        let team_b_index = self.team_b.players.iter().position(|p| *p == player_pubkey);
        if let Some(team_a_index) = team_a_index {
            Ok(self.team_a.player_kills[team_a_index] as u16
                + self.team_a.player_spawns[team_a_index] as u16)
        } else if let Some(team_b_index) = team_b_index {
            Ok(self.team_b.player_kills[team_b_index] as u16
                + self.team_b.player_spawns[team_b_index] as u16)
        } else {
            return Err(error!(WagerError::PlayerNotFound));
        }
    }

    /// Adds a kill to the killer and decrements spawns for the victim
    pub fn add_kill(
        &mut self,
        killer_team: u8,
        killer: Pubkey,
        victim_team: u8,
        victim: Pubkey,
    ) -> Result<()> {
        let killer_player_index: usize = self.get_player_index(killer_team, killer)?;
        let victim_player_index: usize = self.get_player_index(victim_team, victim)?;

        require!(
            self.status == GameStatus::InProgress,
            WagerError::GameNotInProgress
        );

        match killer_team {
            0 => self.team_a.player_kills[killer_player_index] += 1,
            1 => self.team_b.player_kills[killer_player_index] += 1,
            _ => return Err(error!(WagerError::InvalidTeam)),
        }

        match victim_team {
            0 => self.team_a.player_spawns[victim_player_index] -= 1,
            1 => self.team_b.player_spawns[victim_player_index] -= 1,
            _ => return Err(error!(WagerError::InvalidTeam)),
        }

        Ok(())
    }

    /// Adds spawns to a player in a team
    pub fn add_spawns(&mut self, team: u8, player_index: usize) -> Result<()> {
        match team {
            0 => self.team_a.player_spawns[player_index] += 10u16,
            1 => self.team_b.player_spawns[player_index] += 10u16,
            _ => return Err(error!(WagerError::InvalidTeam)),
        }
        Ok(())
    }
}

/// Helper function to check if an error is TeamIsFull
fn is_team_full_error(error: &Error) -> bool {
    error.to_string().contains("TeamIsFull")
}
